//! The facilities with the real database: saving and checking, the scanner on names and notes, the board crossed
//! with the people served, what reaches the AI, and the examples.

use super::*;
use crate::modules::facilities::domain::group::States;
use crate::scanner::guard::Decision;
use crate::storage::open_encrypted;

const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

fn conn() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let c = open_encrypted(&dir.path().join("t.db"), KEY).unwrap();
    (dir, c)
}

fn saved(out: FacilitiesOutcome) -> FacilitiesOverview {
    match out {
        FacilitiesOutcome::Saved { overview } => overview,
        other => panic!("not saved: {other:?}"),
    }
}

fn bathrooms() -> SpaceData {
    SpaceData {
        kind: "bathroom".into(),
        floor: 1,
        count: 4,
        states: States { good: 3, poor: 1, ..Default::default() },
        problems: vec!["leaks".into(), "grab_bars".into()],
        grab_bars: Some(false),
        ..Default::default()
    }
}

#[test]
fn an_empty_database_shows_a_site_to_fill_without_writing_anything() {
    let (_d, c) = conn();
    let o = overview(&c).unwrap();
    assert_eq!((o.facilities.site.id.as_deref(), o.facilities.site.data.name.as_str()), (None, "Inmueble principal"));
    assert!(o.facilities.spaces.is_empty() && o.board.insights.is_empty());
    assert_eq!(o.facilities.space_kinds.last(), Some(&"other"));
    let sites: i64 = c.query_row("SELECT count(*) FROM fac_site", [], |r| r.get(0)).unwrap();
    assert_eq!(sites, 0, "looking does not write");
}

#[test]
fn four_bathrooms_one_poor_are_saved_counted_and_named() {
    let (_d, c) = conn();
    let o = saved(save_space(&c, None, bathrooms(), None).unwrap());
    assert!(o.facilities.site.id.is_some(), "the first space makes the main site");
    let row = &o.facilities.spaces[0];
    assert_eq!((row.space.data.count, row.space.data.states.poor, row.space.data.grab_bars), (4, 1, Some(false)));
    assert_eq!(o.facilities.indicators.bathrooms, 4);
    let broken = o.board.insights.iter().find(|i| i.code == "broken_spaces").unwrap();
    assert_eq!(broken.items, vec!["1 de 4 baños (primer piso)"]);

    // edit it: the fourth one was fixed
    let mut fixed = bathrooms();
    fixed.states = States { good: 4, ..Default::default() };
    fixed.problems.clear();
    let o = saved(save_space(&c, Some(&row.space.id), fixed, None).unwrap());
    assert_eq!(o.facilities.spaces.len(), 1);
    assert!(o.board.insights.iter().all(|i| i.code != "broken_spaces"));

    let o = delete_space(&c, &o.facilities.spaces[0].space.id).unwrap();
    assert!(o.facilities.spaces.is_empty());
    assert!(matches!(delete_space(&c, "spc_nobody"), Err(ServiceError::Facilities(crate::modules::facilities::FacilitiesError::NotFound))));
}

#[test]
fn what_does_not_add_up_is_not_saved_and_the_heads_ups_come_back() {
    let (_d, c) = conn();
    let mut too_many = bathrooms();
    too_many.states.good = 4;
    let FacilitiesOutcome::Invalid { issues } = save_space(&c, None, too_many, None).unwrap() else { panic!("five of four") };
    assert_eq!(issues.iter().map(|i| i.code).collect::<Vec<_>>(), vec!["states_exceed_count"]);
    let spaces: i64 = c.query_row("SELECT count(*) FROM fac_space", [], |r| r.get(0)).unwrap();
    assert_eq!(spaces, 0);

    // a site of one floor and a space on the first floor: saved, with a heads-up
    saved(save_site(&c, SiteData { name: "Casa".into(), floors: Some(1), ..Default::default() }, None).unwrap());
    let o = saved(save_space(&c, None, bathrooms(), None).unwrap());
    assert_eq!(o.facilities.spaces[0].issues.iter().map(|i| i.code).collect::<Vec<_>>(), vec!["floor_above_building"]);
    let FacilitiesOutcome::Invalid { issues } = save_site(&c, SiteData { name: "Casa".into(), built_year: Some(3000), ..Default::default() }, None).unwrap() else { panic!() };
    assert_eq!(issues[0].field, "built_year");
}

#[test]
fn names_and_notes_go_through_the_scanner_because_they_reach_the_ai() {
    let (_d, c) = conn();
    let mut b = bathrooms();
    b.notes = Some("Lo arregla el plomero, llamar al 55 1234 5678".into());
    let FacilitiesOutcome::Quarantine { report } = save_space(&c, None, b.clone(), None).unwrap() else { panic!("a phone in a note") };
    assert_eq!(report.fields[0].path, "notes");
    let o = saved(save_space(&c, None, b, Some(Decision::Redact)).unwrap());
    let notes = o.facilities.spaces[0].space.data.notes.clone().unwrap();
    assert!(!notes.contains("5678"), "{notes}");
    let event: String = c.query_row("SELECT event FROM audit_log ORDER BY rowid DESC LIMIT 1", [], |r| r.get(0)).unwrap();
    assert_eq!(event, "scanner.quarantine");

    let site = SiteData { name: "Casa de la señora María López".into(), notes: Some("Escribir a contacto@correo-ficticio.mx".into()), ..Default::default() };
    assert!(matches!(save_site(&c, site, None).unwrap(), FacilitiesOutcome::Quarantine { .. }));
}

#[test]
fn the_board_crosses_the_stairs_with_the_people_in_a_wheelchair() {
    let (_d, mut c) = conn();
    let input = crate::core::profile::domain::ProfileInput {
        institution: crate::core::profile::domain::InstitutionInput { name: "Asilo Ficticio".into(), kind: crate::core::profile::domain::InstitutionKind::ElderlyHome, ..Default::default() },
        capacity_total: Some(20),
        ..Default::default()
    };
    crate::core::profile::storage::save(&mut c, &input, &crate::core::profile::storage::fixtures()).unwrap();
    for i in 0..3 {
        let d = crate::modules::care::domain::person::BeneficiaryData { first_names: format!("Persona {i}"), approx_age: Some(85), mobility: Some("wheelchair".into()), status: "active".into(), ..Default::default() };
        crate::modules::care::service::save_person(&mut c, crate::modules::care::domain::catalog::Flavor::ElderlyHome, None, d).unwrap();
    }
    saved(save_site(&c, SiteData { name: "Casa".into(), floors: Some(2), floor_access: vec!["none".into()], built_m2: Some(300), ..Default::default() }, None).unwrap());
    let rooms = SpaceData { kind: "bedroom".into(), floor: 1, count: 4, states: States::all(4, "good"), beds: Some(12), ..Default::default() };
    let o = saved(save_space(&c, None, rooms, None).unwrap());
    let stairs = o.board.insights.iter().find(|i| i.code == "only_stairs").unwrap();
    assert_eq!((stairs.values["people"], stairs.values["spaces"], stairs.for_ai), (3, 4, true));
    let beds = o.board.insights.iter().find(|i| i.code == "beds_short").unwrap();
    assert_eq!((beds.values["beds"], beds.values["served"], beds.values["capacity"]), (12, 3, 20));
    assert_eq!(o.board.built_m2_per_person, Some(100));

    let sheet = crate::core::ai_sheet::profile_context(&c).unwrap();
    assert!(sheet.contains("Espacio: Dormitorios, primer piso: 4 (4 bien). 12 camas."), "{sheet}");
    assert!(sheet.contains("Hallazgo: El inmueble tiene varios pisos y solo escaleras entre ellos; hay 4 espacios arriba y 3 personas usan silla de ruedas o están en cama."), "{sheet}");
    assert!(!sheet.contains("las instalaciones"), "the facilities are captured:\n{sheet}");

    // a ramp takes the finding away
    let mut site = o.facilities.site.data.clone();
    site.floor_access = vec!["ramp".into()];
    let o = saved(save_site(&c, site, None).unwrap());
    assert!(o.board.insights.iter().all(|i| i.code != "only_stairs"));
}

#[test]
fn both_examples_load_valid_and_reach_the_ai() {
    for raw in [include_str!("../../../../fixtures/instalaciones-asilo.json"), include_str!("../../../../fixtures/instalaciones-casa-hogar.json")] {
        let (_d, mut c) = conn();
        seed_example(&mut c, raw).unwrap();
        let o = overview(&c).unwrap();
        assert!(o.facilities.spaces.len() >= 8 && !o.facilities.equipment.is_empty());
        assert!(o.facilities.site.issues.is_empty(), "{:?}", o.facilities.site.issues);
        assert!(o.board.insights.iter().any(|i| i.code == "broken_spaces"));
        seed_example(&mut c, raw).unwrap();
        let again = overview(&c).unwrap();
        assert_eq!(again.facilities.spaces.len(), o.facilities.spaces.len(), "loading twice replaces, it does not add");
    }
}
