//! The staff screens with the real database: what reaches the profile and the AI, the covered identifiers, the
//! scanner on the positions, and the rules of the catalog.

use super::*;
use crate::core::profile::domain::{InstitutionInput, InstitutionKind, ProfileInput};
use crate::modules::hr::domain::person::EmergencyContact;
use crate::storage::open_encrypted;
use crate::core::profile::storage as profile_store;

const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

fn conn() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let c = open_encrypted(&dir.path().join("t.db"), KEY).unwrap();
    (dir, c)
}

fn with_profile(c: &mut Connection) {
    let input = ProfileInput { institution: InstitutionInput { name: "Asilo Ficticio".into(), kind: InstitutionKind::ElderlyHome, ..Default::default() }, ..Default::default() };
    profile_store::save(c, &input, &profile_store::fixtures()).unwrap();
}

fn position(c: &Connection, title: &str) -> String {
    overview(c).unwrap().hr.positions.into_iter().find(|p| p.position.input.title == title).map(|p| p.position.id).expect(title)
}

fn person(first: &str, position_id: &str, modality: &str, pay: Option<i64>) -> PersonData {
    PersonData { first_names: first.into(), position_id: Some(position_id.into()), modality: modality.into(), status: "active".into(), pay_amount_mxn: pay, pay_period: Some("monthly".into()), ..Default::default() }
}

fn saved(out: PersonOutcome) -> (PersonView, StaffChange) {
    match out {
        PersonOutcome::Saved { person, change } => (person, change),
        PersonOutcome::Invalid { issues } => panic!("not saved: {issues:?}"),
    }
}

#[test]
fn the_catalog_starts_with_the_positions_of_the_kind_of_institution() {
    let (_d, mut c) = conn();
    with_profile(&mut c);
    let titles: Vec<String> = overview(&c).unwrap().hr.positions.into_iter().map(|p| p.position.input.title).collect();
    assert!(titles.contains(&"Medicina".to_string()) && titles.contains(&"Pastoral".to_string()), "{titles:?}");
    assert!(!titles.contains(&"Educadora o educador".to_string()), "that one is for a children's home");
    assert_eq!(overview(&c).unwrap().hr.positions.len(), titles.len(), "written once");
}

#[test]
fn the_profile_and_the_ai_only_get_aggregates_never_names_identifiers_or_pay() {
    let (_d, mut c) = conn();
    with_profile(&mut c);
    let kitchen = position(&c, "Cocina");
    let mut ana = person("Ana Secreta", &kitchen, "indefinite", Some(7_000));
    ana.last_name_1 = Some("Pérez".into());
    ana.curp = Some("HEGG560427MVZRRL04".into());
    ana.phone = Some("55 1234 5678".into());
    ana.emergency_contacts = vec![EmergencyContact { full_name: "Luis Contacto".into(), relationship: Some("sibling".into()), phone: Some("55 8765 4321".into()), phone_alt: None }];
    saved(save_person(&mut c, None, ana).unwrap());
    let (_, change) = saved(save_person(&mut c, None, person("Rosa Oculta", &kitchen, "indefinite", Some(7_000))).unwrap());

    let profile = change.profile.expect("the profile exists");
    assert_eq!(profile.input.staff.len(), 1);
    assert_eq!((profile.input.staff[0].role.as_str(), profile.input.staff[0].count), ("Cocina", 2));
    assert_eq!(profile.totals.payroll_monthly_mxn, 14_000);
    assert_eq!(change.overview.totals.payroll_monthly_mxn, 14_000);

    let everything = serde_json::to_string(&profile.input).unwrap();
    let sheet = crate::core::ai_sheet::profile_context(&c).unwrap();
    for secret in ["Secreta", "Oculta", "Pérez", "HEGG", "55 1234", "Luis Contacto", "55 8765"] {
        assert!(!everything.contains(secret), "{secret} leaked into the profile");
        assert!(!sheet.contains(secret), "{secret} leaked into what the AI reads");
    }
    assert!(!sheet.contains("7000") && !sheet.contains("7,000") && !sheet.contains("14,000"), "{sheet}");
    assert!(sheet.contains("Personal: Cocina (cocina) — 2 personas (2 con sueldo)."), "{sheet}");
}

#[test]
fn a_record_with_a_blocking_problem_is_refused_and_nothing_is_saved() {
    let (_d, mut c) = conn();
    let kitchen = position(&c, "Cocina");
    let mut bad = person("Ana", &kitchen, "indefinite", None);
    bad.curp = Some("HEGG560427MVZRRL05".into());
    let PersonOutcome::Invalid { issues } = save_person(&mut c, None, bad).unwrap() else { panic!("saved") };
    assert_eq!(issues.iter().map(|i| i.code).collect::<Vec<_>>(), vec!["curp_invalid"]);
    let PersonOutcome::Invalid { issues } = save_person(&mut c, None, person("Ana", "pos_none", "nope", None)).unwrap() else { panic!("saved") };
    let codes: Vec<_> = issues.iter().map(|i| i.code).collect();
    assert!(codes.contains(&"modality_unknown") && codes.contains(&"position_missing"), "{codes:?}");
    assert!(overview(&c).unwrap().hr.people.is_empty());
}

#[test]
fn identifiers_are_covered_shown_only_on_purpose_and_kept_unless_changed() {
    let (_d, mut c) = conn();
    let kitchen = position(&c, "Cocina");
    let mut ana = person("Ana", &kitchen, "indefinite", Some(9_000));
    ana.curp = Some("hegg 560427 mvzrrl04".into());
    ana.clabe = Some("002010077777777771".into());
    ana.pay_method = Some("transfer".into());
    let (view, _) = saved(save_person(&mut c, None, ana).unwrap());
    assert_eq!((view.data.curp.as_deref(), view.data.clabe.as_deref()), (None, None), "never in clear in a view");
    assert!(view.secrets.curp && view.secrets.clabe && !view.secrets.rfc);
    assert_eq!((view.masked.curp.as_deref(), view.masked.clabe.as_deref()), (Some("HEGG••••••••••••04"), Some("••••••••••••••7771")));
    assert_eq!(view.data.bank.as_deref(), Some("Banamex"), "the bank comes from the CLABE");

    assert_eq!(reveal(&c, &view.id, "curp").unwrap(), "HEGG560427MVZRRL04", "stored clean");
    let (event, details): (String, String) = c.query_row("SELECT event, details_json FROM audit_log WHERE event='hr.sensitive_viewed'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(event, "hr.sensitive_viewed");
    assert!(details.contains("\"field\":\"curp\"") && !details.contains("HEGG"), "{details}");

    // sent back as it came (covered: None), the identifiers stay; an empty one is cleared
    let mut again = view.data.clone();
    again.first_names = "Ana María".into();
    again.clabe = Some(String::new());
    let (after, _) = saved(save_person(&mut c, Some(&view.id), again).unwrap());
    assert!(after.secrets.curp && !after.secrets.clabe);
    assert_eq!(reveal(&c, &view.id, "curp").unwrap(), "HEGG560427MVZRRL04");
    assert!(matches!(reveal(&c, &view.id, "clabe"), Err(ServiceError::NotFound)));
    assert!(matches!(reveal(&c, &view.id, "first_names"), Err(ServiceError::Hr(crate::modules::hr::HrError::UnknownField))));
}

#[test]
fn deleting_a_person_removes_everything_and_updates_the_payroll() {
    let (_d, mut c) = conn();
    with_profile(&mut c);
    let kitchen = position(&c, "Cocina");
    let mut ana = person("Ana", &kitchen, "indefinite", Some(5_000));
    ana.emergency_contacts = vec![EmergencyContact { full_name: "Luis".into(), ..Default::default() }];
    let (view, change) = saved(save_person(&mut c, None, ana).unwrap());
    assert_eq!(change.overview.totals.payroll_annual_mxn, 60_000);
    let gone = delete_person(&mut c, &view.id).unwrap();
    assert_eq!(gone.overview.totals.payroll_monthly_mxn, 0);
    let left: i64 = c.query_row("SELECT (SELECT count(*) FROM hr_job) + (SELECT count(*) FROM hr_emergency_contact)", [], |r| r.get(0)).unwrap();
    assert_eq!(left, 0);
    assert!(matches!(delete_person(&mut c, &view.id), Err(ServiceError::Hr(crate::modules::hr::HrError::NotFound))));
    let ev: i64 = c.query_row("SELECT count(*) FROM audit_log WHERE event='hr.person_deleted'", [], |r| r.get(0)).unwrap();
    assert_eq!(ev, 1);
}

#[test]
fn modalities_decide_where_the_money_counts() {
    let (_d, mut c) = conn();
    with_profile(&mut c);
    let pastoral = position(&c, "Pastoral");
    let security = position(&c, "Vigilancia");
    saved(save_person(&mut c, None, person("Hermana", &pastoral, "religious", Some(1_000))).unwrap());
    saved(save_person(&mut c, None, person("Guardia", &security, "external", Some(9_000))).unwrap());
    // an institution's own modality behaves as a built-in one
    let modalities = create_modality(&c, "Capellán por convenio", "fees").unwrap();
    let own = modalities.iter().find(|m| !m.builtin).unwrap();
    assert_eq!((own.behaves_as, own.rules.lft_benefits), ("fees", false));
    let (_, change) = saved(save_person(&mut c, None, person("Padre", &pastoral, &own.code, Some(3_000))).unwrap());
    let t = change.overview.totals;
    assert_eq!((t.payroll_monthly_mxn, t.payroll_benefits_annual_mxn), (3_000, 0), "fees: payroll without benefits");
    assert_eq!((t.staff_support_annual_mxn, t.external_staff_annual_mxn), (12_000, 108_000));
    assert!(matches!(create_modality(&c, "Otra", "nothing"), Err(ServiceError::Hr(crate::modules::hr::HrError::UnknownModality))));
}

#[test]
fn a_position_goes_through_the_scanner_before_it_is_saved() {
    let (_d, mut c) = conn();
    let input = PositionInput { title: "Enfermería de noche".into(), area: Some("health".into()), duties: Some("Cubre a HEGG560427MVZRRL04 los domingos".into()), authorized_seats: Some(2), ..Default::default() };
    let PositionOutcome::Quarantine { .. } = save_position(&mut c, None, input.clone(), None).unwrap() else { panic!("the CURP must stop it") };
    let PositionOutcome::Saved { change } = save_position(&mut c, None, input, Some(Decision::Redact)).unwrap() else { panic!("saved") };
    let night = change.overview.hr.positions.iter().find(|p| p.position.input.title == "Enfermería de noche").unwrap();
    assert!(!night.position.input.duties.as_deref().unwrap().contains("HEGG"));

}

#[test]
fn a_duplicate_title_or_an_archive_in_use_is_refused() {
    let (_d, mut c) = conn();
    let kitchen = position(&c, "Cocina");
    assert!(matches!(
        save_position(&mut c, None, PositionInput { title: "cocina".into(), ..Default::default() }, None),
        Err(ServiceError::Hr(crate::modules::hr::HrError::DuplicateTitle))
    ));
    saved(save_person(&mut c, None, person("Ana", &kitchen, "indefinite", None)).unwrap());
    assert!(matches!(set_position_active(&mut c, &kitchen, false), Err(ServiceError::Hr(crate::modules::hr::HrError::PositionInUse))));
    let laundry = position(&c, "Lavandería");
    let change = set_position_active(&mut c, &laundry, false).unwrap();
    assert!(!change.overview.hr.positions.iter().find(|p| p.position.id == laundry).unwrap().position.active);
}

#[test]
fn own_fields_keep_their_values_and_leave_with_the_field() {
    let (_d, mut c) = conn();
    let kitchen = position(&c, "Cocina");
    let fields = save_field(&c, None, "Talla de uniforme", "select", &["Chica".into(), "Mediana".into()]).unwrap();
    let key = fields[0].key.clone();
    let mut ana = person("Ana", &kitchen, "indefinite", None);
    ana.extra.insert(key.clone(), "Mediana".into());
    ana.extra.insert("unknown".into(), "x".into());
    let (view, _) = saved(save_person(&mut c, None, ana).unwrap());
    assert_eq!(view.data.extra.get(&key).map(String::as_str), Some("Mediana"));
    assert!(!view.data.extra.contains_key("unknown"), "only the institution's fields are kept");
    delete_field(&c, &key).unwrap();
    assert!(person_view(&c, &view.id).data.extra.is_empty());
}

fn person_view(c: &Connection, id: &str) -> PersonView {
    super::person(c, id).unwrap()
}

#[test]
fn saving_the_profile_form_does_not_wipe_the_staff_lines() {
    let (_d, mut c) = conn();
    with_profile(&mut c);
    let kitchen = position(&c, "Cocina");
    saved(save_person(&mut c, None, person("Ana", &kitchen, "indefinite", None)).unwrap());
    let input = ProfileInput { institution: InstitutionInput { name: "Asilo Ficticio".into(), ..Default::default() }, ..Default::default() };
    let crate::core::profile::service::SaveProfileOutcome::Saved { profile } = crate::core::profile::service::save_profile(&mut c, input, None).unwrap() else { panic!("saved") };
    assert_eq!(profile.input.staff.len(), 1);
    assert_eq!(profile.input.staff[0].relation.as_deref(), Some("employee"));
}

#[test]
fn a_change_in_the_staff_keeps_the_confirmed_profile_and_its_version() {
    // audit D1: registering a person used to open an unconfirmed version, so the sheet of the AI read «BORRADOR»
    let (_d, mut c) = conn();
    with_profile(&mut c);
    profile_store::finish_first_start(&c);
    let before = profile_store::load_current(&c).unwrap().unwrap();
    let kitchen = position(&c, "Cocina");
    saved(save_person(&mut c, None, person("Ana", &kitchen, "indefinite", Some(7_000))).unwrap());
    saved(save_person(&mut c, None, person("Rosa", &kitchen, "indefinite", Some(7_000))).unwrap());
    let after = profile_store::load_current(&c).unwrap().unwrap();
    assert_eq!((after.version, &after.confirmed_at), (before.version, &before.confirmed_at), "same version, still confirmed");
    assert_eq!(after.input.staff.iter().map(|s| s.count).sum::<i64>(), 2, "the lines follow the module");
    assert!(!crate::core::ai_sheet::profile_context(&c).unwrap().contains("BORRADOR"));
    let origins: Vec<String> = c.prepare("SELECT origin FROM staff_group UNION SELECT origin FROM population_group").unwrap()
        .query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
    assert!(origins.iter().all(|o| o == "computed"), "{origins:?}");
}
