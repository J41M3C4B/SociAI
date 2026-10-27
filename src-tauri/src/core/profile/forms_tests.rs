use super::*;
use crate::core::profile::domain::{InstitutionInput, InstitutionKind};
use crate::storage::open_encrypted;
use serde_json::json;

const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
const ROSA: &str = "usr_01JB3M7Q8ZK4X2N6V9R5T0W1YD";

fn conn() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let c = open_encrypted(&dir.path().join("t.db"), KEY).unwrap();
    (dir, c)
}

/// The person of the session, who saves.
fn signed_in(c: &Connection) {
    c.execute_batch(&format!("CREATE TEMP TABLE IF NOT EXISTS session_actor (user_id TEXT); INSERT INTO session_actor VALUES ('{ROSA}');")).unwrap();
}

fn values(v: Value) -> Values {
    serde_json::from_value(v).unwrap()
}

fn saved(out: SaveProfileOutcome) -> crate::core::profile::service::ProfileView {
    match out {
        SaveProfileOutcome::Saved { profile } => profile,
        other => panic!("not saved: {other:?}"),
    }
}

fn invalid(out: SaveProfileOutcome) -> Vec<(&'static str, String)> {
    match out {
        SaveProfileOutcome::Invalid { issues } => issues.into_iter().map(|i| (i.code, i.field)).collect(),
        other => panic!("not refused: {other:?}"),
    }
}

fn history(c: &Connection) -> Vec<kernel::Record> {
    kernel::history(c, &mi_institucion::service::profile_ref()).unwrap()
}

#[test]
fn an_older_institution_reads_its_kind_as_an_attention_profile() {
    let (_d, mut c) = conn();
    let old = ProfileInput { institution: InstitutionInput { name: "Asilo Ficticio".into(), kind: InstitutionKind::ElderlyHome, ..Default::default() }, ..Default::default() };
    store::save(&mut c, &old, &store::fixtures()).unwrap();
    let view = get(&c, "institution.identity").unwrap();
    assert_eq!(view.values["institution.populations"], json!(["older_adults"]));
    assert_eq!(view.values["institution.modalities"], json!(["residential"]));
    assert_eq!(view.missing, vec!["institution.mission"]);
}

#[test]
fn a_mixed_institution_is_saved_whole_and_its_kind_follows() {
    let (_d, mut c) = conn();
    let v = values(json!({
        "institution.name": "Casa Ficticia", "institution.mission": "Acompañar.",
        "institution.populations": ["childhood", "older_adults"], "institution.sex_served": "all",
        "institution.modalities": ["day_care"], "institution.care_areas": ["care", "food"],
    }));
    let p = saved(save(&mut c, "institution.identity", v, None).unwrap());
    assert_eq!(p.input.institution.kind, InstitutionKind::Other);
    assert_eq!(crate::core::institution::kind(&c).as_deref(), Some("other"));
    let a = p.input.institution.attention.unwrap();
    assert_eq!((a.populations.len(), a.sex_served.as_deref(), a.care_areas.len()), (2, Some("all"), 2));
    assert!(get(&c, "institution.identity").unwrap().missing.is_empty());
    let sheet = crate::core::ai_sheet::profile_context(&c).unwrap();
    assert!(sheet.contains("A quién atiende: niñez, personas mayores (mujeres y hombres)."), "{sheet}");
    assert!(sheet.contains("Cómo atiende: estancia de día."), "{sheet}");
}

#[test]
fn a_code_out_of_its_list_is_refused_and_nothing_is_saved() {
    let (_d, mut c) = conn();
    let v = values(json!({ "institution.name": "Casa", "institution.populations": ["aliens"] }));
    assert_eq!(invalid(save(&mut c, "institution.identity", v, None).unwrap()), vec![("code_unknown", "institution.populations".into())]);
    assert!(store::load_current(&c).unwrap().is_none());
    assert!(matches!(get(&c, "nope"), Err(ServiceError::NotFound)));
    assert!(matches!(get(&c, "institution.board_member"), Err(ServiceError::NotFound)), "a list is not a window of the profile");
}

#[test]
fn the_sex_served_is_not_kept_without_populations_and_the_mission_goes_through_the_scanner() {
    let (_d, mut c) = conn();
    let v = values(json!({ "institution.name": "Casa", "institution.sex_served": "women", "institution.mission": "Llamar a maria.lopez@example.com" }));
    assert!(matches!(save(&mut c, "institution.identity", v.clone(), None).unwrap(), SaveProfileOutcome::Quarantine { .. }));
    let p = saved(save(&mut c, "institution.identity", v, Some(Decision::Redact)).unwrap());
    assert_eq!(p.input.institution.attention.unwrap().sex_served, None, "it does not apply without populations");
    assert!(!p.input.institution.mission.unwrap().contains("maria.lopez"));
}

/// Each window of «Mi institución» saves its own fields, the new ones included, and leaves the others as they were.
#[test]
fn every_window_saves_its_fields_and_keeps_the_rest() {
    let (_d, mut c) = conn();
    let identity = values(json!({
        "institution.name": "Asilo Ficticio", "institution.mission": "Cuidar.", "institution.populations": ["older_adults"],
        "institution.modalities": ["residential"], "institution.age_min": 60, "institution.services": "Residencia y comedor.",
    }));
    saved(save(&mut c, "institution.identity", identity, None).unwrap());
    let contact = values(json!({
        "institution.contact_phone": "55 5555 0101", "institution.street": "Calle Ficticia", "institution.ext_number": "12",
        "institution.postal_code": "45010", "institution.state": "jal", "institution.municipality": "Zapopan",
    }));
    saved(save(&mut c, "institution.contact", contact, None).unwrap());
    let legal = values(json!({
        "institution.legal_name": "Asilo Ficticio, I.A.P.", "institution.purpose": "La asistencia a personas mayores.",
        "institution.legal_form": "iap", "institution.founded_year": 1987, "institution.junta_folio": "JAP-0001",
        "institution.legal_rfc": "afi870101ab1", "institution.tax_regime": "non_profit", "institution.authorized_donee": "yes",
        "institution.donee_category": "assistance", "institution.cluni": "no", "institution.cluni_key": "kept only with yes",
    }));
    saved(save(&mut c, "institution.legal", legal, None).unwrap());
    let capacity = values(json!({ "institution.capacity_total": 25, "institution.served_estimate": 18, "institution.notes": "Notas." }));
    let p = saved(save(&mut c, "institution.capacity", capacity, None).unwrap());

    // every window kept what the others saved
    let i = &p.input.institution;
    assert_eq!((i.name.as_str(), i.contact_phone.as_deref(), i.legal_rfc.as_deref()), ("Asilo Ficticio", Some("55 5555 0101"), Some("AFI870101AB1")));
    assert_eq!((p.input.capacity_total, p.input.served_estimate), (Some(25), Some(18)));
    let d = i.details.as_ref().unwrap();
    assert_eq!(d["institution.age_min"], json!(60));
    assert_eq!(d["institution.street"], json!("Calle Ficticia"));
    assert_eq!(d["institution.junta_folio"], json!("JAP-0001"));
    assert!(!d.contains_key("institution.cluni_key"), "it does not apply without a CLUNI");

    // what each window shows, and what is still missing in it
    assert!(get(&c, "institution.legal").unwrap().missing.is_empty());
    let contact = get(&c, "institution.contact").unwrap();
    assert_eq!(contact.values["institution.postal_code"], json!("45010"));
    assert!(contact.missing.is_empty());
    assert_eq!(get(&c, "institution.identity").unwrap().values["institution.services"], json!("Residencia y comedor."));

    // emptying a field of a window empties it, and only that one
    let contact = values(json!({ "institution.state": "jal", "institution.municipality": "Zapopan", "institution.street": "Calle Ficticia" }));
    saved(save(&mut c, "institution.contact", contact, None).unwrap());
    assert_eq!(get(&c, "institution.contact").unwrap().missing, vec!["institution.postal_code"]);
    assert_eq!(get(&c, "institution.legal").unwrap().values["institution.junta_folio"], json!("JAP-0001"));
    assert_eq!(get(&c, "institution.identity").unwrap().values["institution.age_min"], json!(60));
}

/// ERP-2 (A1, A2, A3): «Capacidad y cifras» reads how it serves from «Su institución». Where people stay it asks
/// the places and how many are there today; where they come and go, how many it served in the last year; a mix,
/// both. ERP-1: a count that stops applying is emptied, wherever it comes from, as the dictionary says; it is asked
/// again if it applies again.
#[test]
fn the_capacity_window_reads_how_it_serves_from_another_window() {
    let (_d, mut c) = conn();
    let identity = |modalities: Value| values(json!({ "institution.name": "Red Ficticia", "institution.populations": ["general_public"], "institution.modalities": modalities, "institution.sex_served": "all", "institution.age_min": 5 }));
    let p = saved(save(&mut c, "institution.identity", identity(json!(["community", "support"])), None).unwrap());
    assert_eq!(p.input.institution.attention.as_ref().unwrap().sex_served, None, "the public at large has no sex nor ages to say");
    assert!(p.input.institution.details.as_ref().is_none_or(|d| !d.contains_key("institution.age_min")));

    let view = get(&c, "institution.capacity").unwrap();
    assert_eq!(view.context, values(json!({ "institution.modalities": ["community", "support"] })), "only what decides a field here");
    assert_eq!(view.missing, vec!["institution.served_last_year", "institution.staff_paid_estimate", "institution.staff_volunteer_estimate"]);
    let figures = values(json!({ "institution.capacity_total": 40, "institution.served_estimate": 18, "institution.served_last_year": 350, "institution.modalities": ["residential"] }));
    let p = saved(save(&mut c, "institution.capacity", figures.clone(), None).unwrap());
    let last_year = |p: &ProfileInput| p.institution.details.as_ref().and_then(|d| d.get("institution.served_last_year").cloned());
    assert_eq!((p.input.capacity_total, p.input.served_estimate, last_year(&p.input)), (None, None, Some(json!(350))), "what the window sends of another one does not decide");
    let o = crate::core::overview::institution_overview(&c).unwrap();
    assert_eq!((o.people.value, o.people_period, o.capacity), (350, "last_year", None));

    // a residence with its community programs: the places and both counts
    saved(save(&mut c, "institution.identity", identity(json!(["residential", "community"])), None).unwrap());
    assert_eq!(get(&c, "institution.capacity").unwrap().missing, vec!["institution.capacity_total", "institution.served_estimate", "institution.staff_paid_estimate", "institution.staff_volunteer_estimate"]);
    let p = saved(save(&mut c, "institution.capacity", figures.clone(), None).unwrap());
    assert_eq!((p.input.capacity_total, p.input.served_estimate, last_year(&p.input)), (Some(40), Some(18), Some(json!(350))));
    let o = crate::core::overview::institution_overview(&c).unwrap();
    assert_eq!((o.people.value, o.people_period, o.occupied_percent), (18, "today", Some(45)));

    // only the community again: the places and the people of today no longer apply, and they are not kept
    saved(save(&mut c, "institution.identity", identity(json!(["community"])), None).unwrap());
    let kept = store::load_current(&c).unwrap().unwrap().input;
    assert_eq!((kept.capacity_total, kept.served_estimate, last_year(&kept)), (None, None, Some(json!(350))));
    let o = crate::core::overview::institution_overview(&c).unwrap();
    assert_eq!((o.people.value, o.people_period, o.capacity, o.occupied_percent), (350, "last_year", None, None));
    // a residence again: they are asked again
    saved(save(&mut c, "institution.identity", identity(json!(["residential"])), None).unwrap());
    let missing = get(&c, "institution.capacity").unwrap().missing;
    assert!(missing.contains(&"institution.capacity_total") && !missing.contains(&"institution.served_last_year"));
}

#[test]
fn a_wrong_postal_code_rfc_date_or_age_range_is_refused() {
    let (_d, mut c) = conn();
    saved(save(&mut c, "institution.identity", values(json!({ "institution.name": "Casa" })), None).unwrap());
    assert_eq!(invalid(save(&mut c, "institution.contact", values(json!({ "institution.postal_code": "4501" })), None).unwrap()), vec![("postal_code_invalid", "institution.postal_code".into())]);
    let legal = values(json!({ "institution.legal_rfc": "AFI870101AB12", "institution.authorized_donee": "yes", "institution.donee_letter_date": "2023-02-30" }));
    assert_eq!(
        invalid(save(&mut c, "institution.legal", legal, None).unwrap()),
        vec![("rfc_moral_invalid", "institution.legal_rfc".into()), ("date_invalid", "institution.donee_letter_date".into())]
    );
    let ages = values(json!({ "institution.name": "Casa", "institution.populations": ["adults"], "institution.age_min": 70, "institution.age_max": 30 }));
    assert_eq!(invalid(save(&mut c, "institution.identity", ages, None).unwrap()), vec![("age_range", "institution.age_max".into())]);
    // without its name, the profile is not saved
    let nameless = values(json!({ "institution.mission": "Cuidar." }));
    assert_eq!(invalid(save(&mut c, "institution.identity", nameless, None).unwrap()), vec![("name_missing", "institution.name".into())]);
}

/// The folio of the Junta is asked only of the legal forms a Junta watches over.
#[test]
fn the_junta_is_asked_only_of_an_iap_ibp_or_abp() {
    let (_d, mut c) = conn();
    saved(save(&mut c, "institution.identity", values(json!({ "institution.name": "Casa" })), None).unwrap());
    let ac = values(json!({ "institution.legal_form": "ac", "institution.junta_folio": "JAP-1" }));
    saved(save(&mut c, "institution.legal", ac, None).unwrap());
    let view = get(&c, "institution.legal").unwrap();
    assert!(!view.missing.contains(&"institution.junta_folio") && !view.values.contains_key("institution.junta_folio"));
    saved(save(&mut c, "institution.legal", values(json!({ "institution.legal_form": "iap" })), None).unwrap());
    assert!(get(&c, "institution.legal").unwrap().missing.contains(&"institution.junta_folio"));
}

/// The long texts that may reach the AI go through the scanner; the address, folios and keys do not.
#[test]
fn the_objeto_social_goes_through_the_scanner_and_the_address_does_not() {
    let (_d, mut c) = conn();
    saved(save(&mut c, "institution.identity", values(json!({ "institution.name": "Casa" })), None).unwrap());
    let legal = values(json!({ "institution.purpose": "Escribir a maria.lopez@example.com" }));
    let SaveProfileOutcome::Quarantine { report } = save(&mut c, "institution.legal", legal, None).unwrap() else { panic!("held") };
    assert_eq!(report.fields[0].path, "institution.purpose");
    let contact = values(json!({ "institution.street": "Calle de María López", "institution.contact_email": "contacto@casa.org" }));
    saved(save(&mut c, "institution.contact", contact, None).unwrap());
}

/// ADR-033 §4 and audit D6, on the kernel (ERP-1): every save is a change with its origin and a line per datum that
/// changed; what is protected says only that it changed; saving the same again adds nothing; a save that does not
/// happen leaves nothing.
#[test]
fn every_save_leaves_its_history_and_the_origin_of_each_datum() {
    let (_d, mut c) = conn();
    signed_in(&c);
    saved(save(&mut c, "institution.identity", values(json!({ "institution.name": "Casa", "institution.mission": "Cuidar." })), None).unwrap());
    let contact = values(json!({ "institution.street": "Calle Ficticia", "institution.municipality": "Zapopan" }));
    saved(save(&mut c, "institution.contact", contact.clone(), None).unwrap());
    let changes = history(&c);
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[1].origin, kernel::Origin::Person { account: ROSA.into() });
    let line = |f: &str| changes.iter().flat_map(|r| &r.fields).find(|c| c.field == f).cloned().unwrap_or_else(|| panic!("no line for {f}"));
    assert_eq!(line("name").after, json!("Casa"));
    assert_eq!((line("street").after, line("street").protected), (Value::Null, true));
    assert_eq!(line("municipality").after, json!("Zapopan"));
    let view = get(&c, "institution.contact").unwrap();
    assert_eq!(view.origins["institution.street"].origin, "user");
    assert!(view.origins["institution.street"].confirmed_at.is_some());

    // the same again: nothing new; a save the scanner holds back: nothing either
    saved(save(&mut c, "institution.contact", contact, None).unwrap());
    let held = values(json!({ "institution.name": "Casa", "institution.mission": "Llamar a maria.lopez@example.com" }));
    assert!(matches!(save(&mut c, "institution.identity", held, None).unwrap(), SaveProfileOutcome::Quarantine { .. }));
    assert_eq!(history(&c).len(), 2);
}

/// The datum and its line go together: if the history cannot be written, the datum is not saved either.
#[test]
fn without_its_history_a_datum_is_not_saved() {
    let (_d, mut c) = conn();
    saved(save(&mut c, "institution.identity", values(json!({ "institution.name": "Casa" })), None).unwrap());
    c.execute_batch("CREATE TEMP TRIGGER fail_history BEFORE INSERT ON kernel_history BEGIN SELECT RAISE(ABORT, 'no'); END;").unwrap();
    assert!(save(&mut c, "institution.identity", values(json!({ "institution.name": "Casa Nueva" })), None).is_err());
    assert_eq!(store::load_current(&c).unwrap().unwrap().input.institution.name, "Casa");
}

#[test]
fn saving_another_window_keeps_the_attention_profile() {
    let (_d, mut c) = conn();
    let v = values(json!({ "institution.name": "Casa", "institution.populations": ["adults"], "institution.modalities": ["outpatient"] }));
    saved(save(&mut c, "institution.identity", v, None).unwrap());
    let p = saved(save(&mut c, "institution.contact", values(json!({ "institution.contact_email": "contacto@casa.org" })), None).unwrap());
    assert_eq!(p.input.institution.attention.unwrap().populations, vec!["adults".to_string()]);
}

/// What a document said and the person took as it came is saved with the document as its origin; what the person
/// wrote, as the person's. The suggestion is accepted, and the screen shows where each datum comes from.
#[test]
fn a_datum_taken_from_a_document_keeps_the_document_as_its_origin() {
    let (_d, mut c) = conn();
    signed_in(&c);
    saved(save(&mut c, "institution.identity", values(json!({ "institution.name": "Casa" })), None).unwrap());
    let document = "doc_01JB3M7Q8ZK4X2N6V9R5T0W1YE";
    let proposal = crate::core::proposals::propose(
        &c,
        crate::core::proposals::NewProposal {
            form_id: "institution.legal".into(),
            record_id: None,
            field_id: "institution.legal_name".into(),
            value: json!("Casa Ficticia, A.C."),
            origin: crate::core::proposals::Origin::Document,
            source_ref: Some(json!({ "document_id": document, "page": 2 }).to_string()),
            agent: "reader",
        },
    )
    .unwrap();
    let crate::core::proposals::ProposeOutcome::Kept { id } = proposal else { panic!("{proposal:?}") };
    let legal = values(json!({ "institution.legal_name": "Casa Ficticia, A.C.", "institution.purpose": "Cuidar." }));
    saved(save_using(&mut c, "institution.legal", legal, None, &[id]).unwrap());
    let changes = history(&c);
    assert_eq!(changes[1].origin, kernel::Origin::Document { document: document.into(), page: Some(2), account: ROSA.into() });
    assert_eq!(changes[1].fields.iter().map(|f| f.field.as_str()).collect::<Vec<_>>(), ["legal_name"]);
    assert_eq!(changes[2].origin, kernel::Origin::Person { account: ROSA.into() });
    assert_eq!(changes[2].fields.iter().map(|f| f.field.as_str()).collect::<Vec<_>>(), ["purpose"]);
    let view = get(&c, "institution.legal").unwrap();
    assert_eq!(view.origins["institution.legal_name"].origin, "document");
    assert!(view.origins["institution.legal_name"].source_ref.as_deref().unwrap().contains(document));
    assert!(view.proposals.is_empty(), "the suggestion was taken");
}

/// A window of «Mi institución» says what it holds and what waits in it, and nothing of any AI that fills it.
#[test]
fn a_window_view_carries_no_trace_of_an_assistant_that_fills() {
    let (_d, c) = conn();
    for form in crate::core::institution::forms::forms() {
        let view = serde_json::to_value(get(&c, form.id).unwrap()).unwrap();
        let mut keys: Vec<_> = view.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, vec!["context", "missing", "origins", "proposals", "spec", "values"], "{}", form.id);
    }
}
