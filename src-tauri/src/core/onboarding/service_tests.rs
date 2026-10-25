//! The first start with the real database (ADR-042): from an empty start to a finished institution, step by step,
//! each step saved through its window of «Mi institución» (checks, scanner, history), the records of the modules
//! counting instead of the quick figures, and what the AI reads.

use super::*;
use crate::storage::open_encrypted;

const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

fn conn() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let c = open_encrypted(&dir.path().join("t.db"), KEY).unwrap();
    (dir, c)
}

fn user(c: &Connection, id: &str, role: Role) -> CurrentUser {
    c.execute(
        "INSERT INTO app_user (id,username,display_name,role,password_hash,created_at,updated_at) VALUES (?1,?1,?1,?2,'x','t','t')",
        [id, role.as_db()],
    )
    .unwrap();
    CurrentUser { id: id.into(), username: id.into(), display_name: id.into(), role, must_change_password: false }
}

fn values(v: serde_json::Value) -> Values {
    serde_json::from_value(v).unwrap()
}

fn saved(out: OnboardingOutcome) -> OnboardingStatus {
    match out {
        OnboardingOutcome::Saved { onboarding } => onboarding,
        other => panic!("not saved: {other:?}"),
    }
}

fn missing(s: &OnboardingStatus) -> Vec<(&'static str, Vec<&'static str>)> {
    s.steps.iter().filter(|x| !x.complete).map(|x| (x.key, x.missing.clone())).collect()
}

fn fields_of(s: &OnboardingStatus, step: &str) -> Vec<&'static str> {
    s.steps.iter().find(|x| x.key == step).unwrap().form.sections[0].fields.iter().map(|f| f.id).collect()
}

#[test]
fn an_empty_start_shows_every_step_to_fill_and_the_setup_to_the_administrator() {
    let (_d, c) = conn();
    let admin = user(&c, "admin", Role::Admin);
    let s = status(&c, &admin).unwrap();
    assert!(!s.done && !s.ready && !s.welcomed && s.can_postpone);
    assert_eq!(s.steps.iter().map(|x| x.key).collect::<Vec<_>>(), vec!["who", "attention", "place", "constitution", "figures"]);
    assert!(s.steps.iter().all(|x| !x.complete));
    // each step is drawn from the catalog, as its window draws it
    let place = &s.steps[2].form;
    assert_eq!((place.id.as_str(), place.sections[0].columns), ("onboarding.place", 2));
    assert_eq!(fields_of(&s, "place"), vec!["institution.state", "institution.municipality"]);
    assert_eq!(place.sections[0].fields[0].options.len(), 32);
    assert_eq!(s.setup.unwrap().managers, 0);

    let director = user(&c, "rosa", Role::Manager);
    let s = status(&c, &director).unwrap();
    assert!(!s.can_postpone && s.setup.is_none(), "the direction cannot leave it for later and does not see the setup");
}

#[test]
fn step_by_step_to_a_finished_institution_that_the_ai_knows() {
    let (_d, mut c) = conn();
    let rosa = user(&c, "rosa", Role::Manager);
    assert!(matches!(finish(&mut c, &rosa), Err(ServiceError::OnboardingIncomplete)));

    let s = saved(save(&mut c, &rosa, "who", values(serde_json::json!({ "institution.name": "Asilo Ficticio", "institution.mission": "Un hogar digno para adultos mayores." })), None).unwrap());
    assert!(s.steps[0].complete);
    assert_eq!(s.values["institution.name"], "Asilo Ficticio", "it comes back to go on where it was");

    let attention = values(serde_json::json!({ "institution.populations": ["older_adults"], "institution.modalities": ["residential", "day_care"] }));
    saved(save(&mut c, &rosa, "attention", attention, None).unwrap());
    saved(save(&mut c, &rosa, "place", values(serde_json::json!({ "institution.state": "jal", "institution.municipality": "Zapopan" })), None).unwrap());
    let constitution = values(serde_json::json!({ "institution.legal_form": "ac", "institution.founded_year": 1987, "institution.authorized_donee": "yes", "institution.cluni": "in_progress" }));
    saved(save(&mut c, &rosa, "constitution", constitution, None).unwrap());
    let figures = values(serde_json::json!({ "institution.capacity_total": 25, "institution.served_estimate": 22, "institution.staff_paid_estimate": 6 }));
    let s = saved(save(&mut c, &rosa, "figures", figures, None).unwrap());
    assert_eq!(missing(&s), vec![("figures", vec!["institution.staff_volunteer_estimate"])]);

    assert!(!s.done);
    // 0 volunteers is an answer, and with the last indispensable datum the first start is finished (ADR-042)
    let figures = values(serde_json::json!({ "institution.capacity_total": 25, "institution.served_estimate": 22, "institution.staff_paid_estimate": 6, "institution.staff_volunteer_estimate": 0 }));
    let s = saved(save(&mut c, &rosa, "figures", figures, None).unwrap());
    assert!(s.ready && s.done, "{:?}", missing(&s));

    // nothing of money or of the building was asked: they belong to their modules (ADR-042)
    assert!(crate::modules::facilities::api::summaries(&c).unwrap().is_empty());
    assert_eq!(crate::modules::finance::api::lines(&c).unwrap().annual_budget_mxn, None);

    let s = finish(&mut c, &rosa).unwrap();
    assert!(s.done);
    let p = profile_store::load_current(&c).unwrap().unwrap();
    assert!(p.confirmed_at.is_some(), "the first version is confirmed");
    let events: i64 = c.query_row("SELECT count(*) FROM audit_log WHERE event = 'institution.onboarded'", [], |r| r.get(0)).unwrap();
    assert_eq!(events, 1);
    assert!(finish(&mut c, &rosa).unwrap().done, "closing twice changes nothing");

    // what the first start asked is in the windows of «Mi institución», and nothing indispensable is missing there
    let o = crate::core::overview::institution_overview(&c).unwrap();
    let codes: Vec<_> = o.completion.gaps.iter().map(|g| g.code).collect();
    for asked in ["name", "mission", "populations", "modalities", "state", "municipality", "legal_form", "founded_year", "authorized_donee", "cluni", "capacity_total", "served", "staff"] {
        assert!(!codes.contains(&asked), "{asked} was asked and is still missing: {codes:?}");
    }

    let sheet = crate::core::ai_sheet::profile_context(&c).unwrap();
    for fact in [
        "Ubicación: Zapopan, Jalisco.",
        "Fundada en 1987 (",
        "Figura jurídica: asociación civil (A.C.).",
        "Donataria autorizada por el SAT: sí.",
        "CLUNI (registro federal de organizaciones de la sociedad civil): en trámite.",
        "Personas atendidas (cifra aproximada que dio la persona; todavía sin registros): 22.",
        "Personal (cifra aproximada que dio la persona; todavía sin registros): 6 con sueldo y 0 de voluntariado.",
    ] {
        assert!(sheet.contains(fact), "missing «{fact}» in:\n{sheet}");
    }
}

/// Each step goes the way of its window: the same checks, the scanner on free text, and the history of each datum.
#[test]
fn a_step_goes_the_way_of_its_window() {
    let (_d, mut c) = conn();
    let rosa = user(&c, "rosa", Role::Manager);
    saved(save(&mut c, &rosa, "who", values(serde_json::json!({ "institution.name": "Casa Ficticia" })), None).unwrap());

    let wrong = values(serde_json::json!({ "institution.legal_form": "ac", "institution.founded_year": 3000 }));
    let OnboardingOutcome::Invalid { issues } = save(&mut c, &rosa, "constitution", wrong, None).unwrap() else { panic!("a year to come") };
    assert_eq!(issues.iter().map(|i| i.field.as_str()).collect::<Vec<_>>(), vec!["institution.founded_year"]);
    let OnboardingOutcome::Invalid { issues } = save(&mut c, &rosa, "place", values(serde_json::json!({ "institution.state": "atlantis" })), None).unwrap() else { panic!("no such state") };
    assert_eq!(issues[0].field, "institution.state");

    let mission = values(serde_json::json!({ "institution.name": "Casa Ficticia", "institution.mission": "Cuidamos niñas. Informes con la directora al 55 1234 5678." }));
    assert!(matches!(save(&mut c, &rosa, "who", mission.clone(), None).unwrap(), OnboardingOutcome::Quarantine { .. }));
    let s = saved(save(&mut c, &rosa, "who", mission, Some(Decision::Redact)).unwrap());
    assert!(!s.values["institution.mission"].as_str().unwrap().contains("5678"));

    // a municipality named after a person is a place: no scanner asks about it, and only a place name fits
    for place in ["Benito Juárez", "Miguel Hidalgo"] {
        let s = saved(save(&mut c, &rosa, "place", values(serde_json::json!({ "institution.state": "cdmx", "institution.municipality": place })), None).unwrap());
        assert_eq!(s.values["institution.municipality"], place);
    }
    let OnboardingOutcome::Invalid { issues } = save(&mut c, &rosa, "place", values(serde_json::json!({ "institution.municipality": "Llamar al 55 1234 5678" })), None).unwrap() else { panic!("a phone is not a place") };
    assert_eq!((issues[0].code, issues[0].field.as_str()), ("place_name_invalid", "institution.municipality"));

    // the history of each datum is the kernel's (ERP-1)
    let lines = kernel::history(&c, &mi_institucion::service::profile_ref()).unwrap();
    assert!(lines.iter().flat_map(|r| &r.fields).any(|f| f.field == "name"), "{lines:?}");
    assert!(matches!(save(&mut c, &rosa, "nope", Values::new(), None), Err(ServiceError::NotFound)));
}

/// A step changes only its own fields: what the window holds besides them stays.
#[test]
fn a_step_keeps_the_rest_of_its_window() {
    let (_d, mut c) = conn();
    let rosa = user(&c, "rosa", Role::Manager);
    saved(save(&mut c, &rosa, "who", values(serde_json::json!({ "institution.name": "Casa Ficticia" })), None).unwrap());
    let contact = values(serde_json::json!({ "institution.contact_phone": "55 5555 0101", "institution.street": "Calle Ficticia", "institution.state": "pue" }));
    crate::core::profile::forms::save(&mut c, "institution.contact", contact, None).unwrap();

    saved(save(&mut c, &rosa, "place", values(serde_json::json!({ "institution.state": "jal", "institution.municipality": "Zapopan" })), None).unwrap());
    let window = crate::core::profile::forms::saved_values(&c, "institution.contact").unwrap();
    assert_eq!((window["institution.state"].as_str(), window["institution.contact_phone"].as_str()), (Some("jal"), Some("55 5555 0101")));
    assert_eq!(window["institution.street"], "Calle Ficticia");

    // a field of the step left empty is emptied
    saved(save(&mut c, &rosa, "place", values(serde_json::json!({ "institution.state": "jal" })), None).unwrap());
    assert!(!crate::core::profile::forms::saved_values(&c, "institution.contact").unwrap().contains_key("institution.municipality"));
}

#[test]
fn the_records_of_the_modules_count_instead_of_the_quick_figures() {
    let (_d, mut c) = conn();
    let rosa = user(&c, "rosa", Role::Manager);
    saved(save(&mut c, &rosa, "who", values(serde_json::json!({ "institution.name": "Asilo Ficticio" })), None).unwrap());
    // a residence: the people there today are asked (ERP-2)
    saved(save(&mut c, &rosa, "attention", values(serde_json::json!({ "institution.populations": ["older_adults"], "institution.modalities": ["residential"] })), None).unwrap());
    for i in 0..3 {
        let p = crate::modules::care::domain::person::BeneficiaryData { first_names: format!("Persona {i}"), approx_age: Some(80), status: "active".into(), ..Default::default() };
        crate::modules::care::service::save_person(&mut c, crate::modules::care::domain::catalog::Flavor::ElderlyHome, None, p).unwrap();
    }
    let s = status(&c, &rosa).unwrap();
    let figures = s.steps.iter().find(|x| x.key == "figures").unwrap();
    if !crate::core::modules::shown("people") {
        // ADR-043: a hidden module is not read; its quick figure is asked like the others
        assert_eq!(s.records.served, 0);
        assert!(figures.missing.contains(&"institution.served_estimate"), "{:?}", figures.missing);
        return;
    }
    assert_eq!(s.records.served, 3);
    assert_eq!(figures.missing, vec!["institution.capacity_total", "institution.staff_paid_estimate", "institution.staff_volunteer_estimate"], "served comes from the records");
    assert!(!fields_of(&s, "figures").contains(&"institution.served_estimate"), "and it is not asked");
    // nor is it missing in its window
    assert!(!crate::core::profile::forms::get(&c, "institution.capacity").unwrap().missing.contains(&"institution.served_estimate"));
}

/// ADR-042: filling the indispensable data from the windows of «Mi institución» finishes the first start too; there is
/// one way to fill the institution, and Inicio never says «0 data missing».
#[test]
fn the_windows_finish_the_first_start_too() {
    let (_d, mut c) = conn();
    let rosa = user(&c, "rosa", Role::Manager);
    let windows = [
        ("institution.identity", serde_json::json!({ "institution.name": "Casa Ficticia", "institution.mission": "Cuidar.", "institution.populations": ["older_adults"], "institution.modalities": ["residential"] })),
        ("institution.contact", serde_json::json!({ "institution.state": "pue", "institution.municipality": "Cholula" })),
        ("institution.legal", serde_json::json!({ "institution.legal_form": "iap", "institution.founded_year": 2015, "institution.authorized_donee": "no", "institution.cluni": "no" })),
    ];
    for (form, v) in windows {
        crate::core::profile::forms::save(&mut c, form, values(v), None).unwrap();
        assert!(!status(&c, &rosa).unwrap().done);
    }
    assert!(profile_store::load_current(&c).unwrap().unwrap().confirmed_at.is_none(), "a draft while something indispensable is missing");
    let capacity = values(serde_json::json!({ "institution.capacity_total": 30, "institution.served_estimate": 28, "institution.staff_paid_estimate": 9, "institution.staff_volunteer_estimate": 4 }));
    crate::core::profile::forms::save(&mut c, "institution.capacity", capacity, None).unwrap();
    let s = status(&c, &rosa).unwrap();
    assert!(s.done && s.ready);
    assert!(profile_store::load_current(&c).unwrap().unwrap().confirmed_at.is_some());
    let events: i64 = c.query_row("SELECT count(*) FROM audit_log WHERE event = 'institution.onboarded'", [], |r| r.get(0)).unwrap();
    assert_eq!(events, 1);
}

#[test]
fn the_welcome_is_once_per_person() {
    let (_d, c) = conn();
    let rosa = user(&c, "rosa", Role::Manager);
    let lupe = user(&c, "lupe", Role::Manager);
    welcome_done(&c, &rosa).unwrap();
    assert!(status(&c, &rosa).unwrap().welcomed);
    assert!(!status(&c, &lupe).unwrap().welcomed);
}

/// D3 of ERP-1: what «Mi institución» saves is the profile at once; there is no draft to confirm. Before the first
/// start is finished the AI reads it as a draft, and from then on as the profile.
#[test]
fn what_mi_institucion_saves_is_the_profile_and_the_first_start_says_when_it_is_finished() {
    let (_d, mut c) = conn();
    let rosa = user(&c, "rosa", Role::Manager);
    saved(save(&mut c, &rosa, "who", values(serde_json::json!({ "institution.name": "Asilo Ficticio", "institution.mission": "Un hogar digno." })), None).unwrap());
    let mission = |m: &str| values(serde_json::json!({ "institution.name": "Asilo Ficticio", "institution.mission": m }));
    match crate::core::profile::forms::save(&mut c, "institution.identity", mission("Un hogar digno para adultos mayores."), None).unwrap() {
        SaveProfileOutcome::Saved { profile } => assert!(profile.is_draft, "the first start is not finished yet"),
        other => panic!("not saved: {other:?}"),
    }

    // the institution finishes (everything the steps ask for), and what is saved is read as the profile
    c.execute("UPDATE institution SET onboarded_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')", []).unwrap();
    match crate::core::profile::forms::save(&mut c, "institution.identity", mission("Un hogar digno y seguro para adultos mayores."), None).unwrap() {
        SaveProfileOutcome::Saved { profile } => {
            assert!(!profile.is_draft && profile.confirmed_at.is_some());
            assert_eq!(profile.input.institution.mission.as_deref(), Some("Un hogar digno y seguro para adultos mayores."));
        }
        other => panic!("not saved: {other:?}"),
    }
    let sheet = crate::core::ai_sheet::profile_context(&c).unwrap();
    assert!(!sheet.contains("BORRADOR"), "the AI reads it as the profile:\n{sheet}");

    // what is invalid is still not saved
    let bad = values(serde_json::json!({ "institution.founded_year": 3000 }));
    assert!(matches!(crate::core::profile::forms::save(&mut c, "institution.legal", bad, None).unwrap(), SaveProfileOutcome::Invalid { .. }));
}

/// The core does not know the AI: whether it has its key is not said here, the command that answers adds it.
#[test]
fn the_setup_of_the_core_leaves_the_ai_to_the_platform() {
    let (_d, c) = conn();
    let admin = user(&c, "admin", Role::Admin);
    let setup = status(&c, &admin).unwrap().setup.unwrap();
    assert_eq!(setup.ai_ready, None);
    assert!(serde_json::to_value(setup).unwrap()["ai_ready"].is_null());
}
