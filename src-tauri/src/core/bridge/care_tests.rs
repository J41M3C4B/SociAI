//! The people served with the real database: what reaches the profile and the AI, the approximate dates, the covered
//! CURP, discharges, the waiting list, groups of their own and the board.

use super::*;
use crate::modules::care::domain::person::ResponsibleContact;
use crate::core::profile::domain::{InstitutionInput, InstitutionKind, ProfileInput};
use crate::modules::finance::domain::lines::{ExpenseItemInput, FinanceInput};
use crate::modules::finance::domain::money::Period;
use crate::storage::open_encrypted;

const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

fn conn() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let c = open_encrypted(&dir.path().join("t.db"), KEY).unwrap();
    (dir, c)
}

fn with_profile(c: &mut Connection, kind: InstitutionKind) {
    let input = ProfileInput {
        institution: InstitutionInput { name: "Casa Ficticia".into(), kind, ..Default::default() },
        capacity_total: Some(4),
        ..Default::default()
    };
    crate::core::profile::storage::save(c, &input, &crate::core::profile::storage::fixtures()).unwrap();
    let money = FinanceInput { expenses: vec![ExpenseItemInput { label: "Alimentos".into(), amount_mxn: Some(24_000), period: Period::Monthly }], ..Default::default() };
    crate::modules::finance::storage::save(c, &money).unwrap();
    // two bathrooms a wheelchair cannot use, in the facilities module (ADR-030)
    let bathrooms = crate::modules::facilities::domain::group::SpaceData { kind: "bathroom".into(), count: 2, accessible: Some(false), ..Default::default() };
    crate::modules::facilities::service::save_space(c, None, bathrooms).unwrap();
}

fn person(first: &str, sex: &str, age: i64) -> BeneficiaryData {
    BeneficiaryData { first_names: first.into(), sex: Some(sex.into()), approx_age: Some(age), status: "active".into(), ..Default::default() }
}

fn saved(out: PersonOutcome) -> (PersonView, CareChange) {
    match out {
        PersonOutcome::Saved { person, change } => (person, change),
        PersonOutcome::Invalid { issues } => panic!("not saved: {issues:?}"),
    }
}

#[test]
fn the_profile_and_the_ai_only_get_counts_never_names_curp_or_responsible_people() {
    let (_d, mut c) = conn();
    with_profile(&mut c, InstitutionKind::ElderlyHome);
    let mut luz = person("Luz Secreta", "female", 84);
    luz.curp = Some("HEGG560427MVZRRL04".into());
    luz.monthly_fee_mxn = Some(3_000);
    luz.fee_payer = Some("family".into());
    luz.contacts = vec![ResponsibleContact { full_name: "Hijo Oculto".into(), phone: Some("55 8765 4321".into()), ..Default::default() }];
    saved(save_person(&mut c, None, luz).unwrap());
    let (_, change) = saved(save_person(&mut c, None, person("Rosa Reservada", "female", 86)).unwrap());

    let profile = change.profile.expect("profile");
    let lines: Vec<_> = profile.input.population.iter().map(|g| (g.label.as_str(), g.count, g.paying_count)).collect();
    assert_eq!(lines, vec![("Mujeres de 80 a 89 años", 1, Some(1)), ("Mujeres de 80 a 89 años", 1, None)]);
    assert_eq!((profile.totals.population, profile.totals.fees_monthly_mxn), (2, 3_000));

    let everything = serde_json::to_string(&profile.input).unwrap();
    let sheet = crate::core::ai_sheet::profile_context(&c).unwrap();
    for secret in ["Secreta", "Reservada", "HEGG", "Oculto", "8765"] {
        assert!(!everything.contains(secret), "{secret} leaked into the profile");
        assert!(!sheet.contains(secret), "{secret} leaked into what the AI reads");
    }
    assert!(sheet.contains("Población: Mujeres de 80 a 89 años — 2 personas."), "{sheet}");
}

#[test]
fn an_age_becomes_an_approximate_birth_date_until_someone_writes_the_real_one() {
    let (_d, mut c) = conn();
    with_profile(&mut c, InstitutionKind::ElderlyHome);
    let (v, _) = saved(save_person(&mut c, None, person("Luz", "female", 84)).unwrap());
    assert!(v.data.birth_date_approx);
    assert_eq!(v.age, Some(84));
    let mut again = v.data.clone();
    again.first_names = "Luz María".into();
    let (v2, _) = saved(save_person(&mut c, Some(&v.id), again).unwrap());
    assert!(v2.data.birth_date_approx, "untouched, it stays approximate");
    let mut real = v2.data.clone();
    real.birth_date = Some("1942-03-15".into());
    let (v3, _) = saved(save_person(&mut c, Some(&v.id), real).unwrap());
    assert!(!v3.data.birth_date_approx);
}

#[test]
fn the_curp_is_covered_and_showing_it_is_recorded() {
    let (_d, mut c) = conn();
    with_profile(&mut c, InstitutionKind::ElderlyHome);
    let mut luz = person("Luz", "female", 84);
    luz.curp = Some("hegg560427mvzrrl04".into());
    let (v, _) = saved(save_person(&mut c, None, luz).unwrap());
    assert_eq!((v.data.curp.as_deref(), v.curp_masked.as_deref(), v.curp_stored), (None, Some("HEGG••••••••••••04"), true));
    assert_eq!(reveal_curp(&c, &v.id).unwrap(), "HEGG560427MVZRRL04");
    let n: i64 = c.query_row("SELECT count(*) FROM audit_log WHERE event='care.sensitive_viewed' AND details_json NOT LIKE '%HEGG%'", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
    let mut bad = person("Ana", "female", 80);
    bad.curp = Some("HEGG560427MVZRRL05".into());
    let PersonOutcome::Invalid { issues } = save_person(&mut c, None, bad).unwrap() else { panic!("saved a wrong CURP") };
    assert_eq!(issues[0].code, "curp_invalid");
}

#[test]
fn a_discharge_leaves_the_counts_but_keeps_the_record() {
    let (_d, mut c) = conn();
    with_profile(&mut c, InstitutionKind::ChildrenHome);
    let (v, _) = saved(save_person(&mut c, None, person("Ana", "female", 9)).unwrap());
    let mut gone = v.data.clone();
    gone.status = "discharged".into();
    gone.discharge_reason = Some("family_reintegration".into());
    gone.status_date = Some(crate::modules::care::storage::today(&c).unwrap());
    let (_, change) = saved(save_person(&mut c, Some(&v.id), gone).unwrap());
    assert_eq!(change.overview.board.indicators.served, 0);
    assert_eq!(change.overview.board.indicators.discharged_this_year, 1);
    assert_eq!(change.overview.care.people.len(), 1, "the record stays");
    assert_eq!(change.profile.unwrap().totals.population, 0);
}

#[test]
fn the_waiting_list_counts_as_demand_and_a_request_becomes_a_record() {
    let (_d, mut c) = conn();
    with_profile(&mut c, InstitutionKind::ElderlyHome);
    let req = WaitlistInput { requested_on: "2026-08-01".into(), name: Some("Carmen Pérez López".into()), sex: Some("female".into()), approx_age: Some(81), status: "waiting".into(), ..Default::default() };
    let WaitlistOutcome::Saved { waitlist, change } = save_waitlist(&mut c, None, &req).unwrap() else { panic!("saved") };
    assert_eq!(waitlist.len(), 1);
    assert!(change.overview.board.insights.iter().any(|i| i.code == "waitlist_vs_seats" && i.values["waiting"] == 1 && i.values["free"] == 4));
    let WaitlistOutcome::Invalid { .. } = save_waitlist(&mut c, None, &WaitlistInput { requested_on: "ayer".into(), ..Default::default() }).unwrap() else { panic!("saved a bad date") };

    let no_age = WaitlistInput { requested_on: "2026-08-02".into(), status: "waiting".into(), ..Default::default() };
    let WaitlistOutcome::Saved { waitlist, .. } = save_waitlist(&mut c, None, &no_age).unwrap() else { panic!() };
    assert!(matches!(admit_waitlist(&mut c, &waitlist[1].id), Err(ServiceError::Care(crate::modules::care::CareError::AgeNeeded))));

    let PersonOutcome::Saved { person, change } = admit_waitlist(&mut c, &waitlist[0].id).unwrap() else { panic!() };
    assert_eq!((person.data.first_names.as_str(), person.data.last_name_1.as_deref(), person.data.last_name_2.as_deref()), ("Carmen", Some("Pérez"), Some("López")));
    assert!(person.data.birth_date_approx);
    assert_eq!(change.overview.board.waiting, 1, "the one without age still waits");
    assert!(change.overview.care.waitlist.iter().any(|w| w.input.status == "admitted" && w.person_id.as_deref() == Some(person.id.as_str())));
}

#[test]
fn the_board_crosses_the_people_with_the_spaces_and_the_money() {
    let (_d, mut c) = conn();
    with_profile(&mut c, InstitutionKind::ElderlyHome);
    for (i, mobility) in ["wheelchair", "wheelchair", "bedridden"].iter().enumerate() {
        let mut p = person(&format!("Persona {i}"), "female", 80 + i as i64);
        p.mobility = Some(mobility.to_string());
        p.monthly_fee_mxn = Some(2_000);
        p.fee_payer = Some("family".into());
        saved(save_person(&mut c, None, p).unwrap());
    }
    let b = board(&c).unwrap();
    assert_eq!((b.free_seats, b.occupancy_percent, b.cost_per_person_monthly), (Some(1), Some(75), Some(8_000)));
    let access = b.insights.iter().find(|i| i.code == "mobility_vs_access").unwrap();
    assert_eq!((access.values["people"], access.items.clone(), access.for_ai), (3, vec!["2 baños (planta baja)".to_string()], true));
    let gap = b.insights.iter().find(|i| i.code == "cost_gap").unwrap();
    assert_eq!((gap.values["gap"], gap.for_ai), (6_000, false), "money never goes to the AI");
    let sheet = crate::core::ai_sheet::profile_context(&c).unwrap();
    assert!(sheet.contains("Hallazgo: 3 personas usan silla de ruedas o están en cama, y 2 espacios no se pueden usar en silla de ruedas: 2 baños (planta baja)."), "{sheet}");
    let numbers = crate::ai::figures::digit_numbers(&sheet);
    assert!(!numbers.contains("8000") && !numbers.contains("6000"), "{sheet}");
}

#[test]
fn a_group_of_their_own_names_the_line_and_goes_through_the_scanner() {
    let (_d, mut c) = conn();
    with_profile(&mut c, InstitutionKind::ElderlyHome);
    let Err(_) = save_group(&mut c, None, "Pabellón de HEGG560427MVZRRL04", true, None).unwrap() else { panic!("the CURP must stop it") };
    let Ok(groups) = save_group(&mut c, None, "Pabellón A", true, None).unwrap() else { panic!() };
    let mut p = person("Luz", "female", 84);
    p.group_id = Some(groups[0].id.clone());
    let (v, change) = saved(save_person(&mut c, None, p).unwrap());
    assert_eq!(v.group, "Pabellón A");
    assert_eq!(change.profile.unwrap().input.population[0].label, "Pabellón A");
    assert!(matches!(save_group(&mut c, None, "pabellón a", true, None), Err(ServiceError::Care(crate::modules::care::CareError::DuplicateTitle))));
}
