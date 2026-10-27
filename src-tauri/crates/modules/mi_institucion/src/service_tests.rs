use super::*;
use serde_json::json;

const ROSA: &str = "usr_01JB3M7Q8ZK4X2N6V9R5T0W1YD";

fn db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    kernel::migrate(&mut conn, &kernel::Modules::discover().unwrap()).unwrap();
    conn
}

fn person() -> Origin {
    Origin::Person { account: ROSA.into() }
}

fn window(id: &str, values: Value) -> WindowInput {
    WindowInput { window: id.into(), values: values.as_object().unwrap().clone() }
}

fn save(conn: &mut Connection, id: &str, values: Value) -> Result<Applied, ApplyError> {
    apply(conn, &window(id, values), None, &person())
}

fn identity() -> Value {
    json!({ "name": "  Casa Hogar Esperanza ", "mission": "Cuidar", "populations": ["older_adults"], "modalities": ["residential"] })
}

#[test]
fn a_window_saved_is_the_profile_with_its_history() {
    let mut conn = db();
    let applied = save(&mut conn, "identity", identity()).unwrap();
    assert_eq!((applied.version, applied.entity.to_string().as_str()), (1, "mi_institucion.profile#profile"));
    let now = profile(&conn).unwrap();
    assert_eq!(now.values["name"], json!("Casa Hogar Esperanza"), "texts are kept trimmed");
    assert_eq!(now.values["populations"], json!(["older_adults"]));
    let history = kernel::history(&conn, &profile_ref()).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].origin, person());
    let facts: i64 = conn.query_row("SELECT count(*) FROM kernel_outbox WHERE fact = ?1", [PROFILE_SAVED], |r| r.get(0)).unwrap();
    assert_eq!(facts, 1);
}

#[test]
fn a_window_changes_only_its_fields_and_empties_what_it_leaves_out() {
    let mut conn = db();
    save(&mut conn, "identity", identity()).unwrap();
    save(&mut conn, "capacity", json!({ "capacity_total": 40, "served_estimate": 35, "notes": "Dos pisos" })).unwrap();
    save(&mut conn, "capacity", json!({ "capacity_total": 42, "served_estimate": 35 })).unwrap();
    let now = profile(&conn).unwrap();
    assert_eq!((now.version, &now.values["name"], &now.values["capacity_total"]), (3, &json!("Casa Hogar Esperanza"), &json!(42)));
    assert!(!now.values.contains_key("notes"), "a field of the window left out is emptied");
    assert!(matches!(save(&mut conn, "capacity", json!({ "capacity_total": 42, "served_estimate": 35 })), Err(ApplyError::NothingToChange)));
}

/// A field that no longer applies is emptied in the proposal, and the module writes the proposal: what is hidden is
/// not kept, even when it belongs to another window (the first finding of ERP-2).
#[test]
fn what_no_longer_applies_is_not_kept_whatever_its_window() {
    let mut conn = db();
    save(&mut conn, "identity", identity()).unwrap();
    save(&mut conn, "capacity", json!({ "capacity_total": 40, "served_estimate": 35 })).unwrap();
    let community = json!({ "name": "Casa Hogar Esperanza", "mission": "Cuidar", "populations": ["general_public"], "modalities": ["community"] });
    let p = prepare(&conn, &window("identity", community.clone())).unwrap();
    let emptied: Vec<&str> = p.changes.iter().filter(|c| c.after.is_null()).map(|c| c.field.as_str()).collect();
    assert_eq!(emptied, ["capacity_total", "served_estimate"]);
    save(&mut conn, "identity", community).unwrap();
    let now = profile(&conn).unwrap();
    assert!(!now.values.contains_key("capacity_total") && !now.values.contains_key("served_estimate"));
    assert!(PROFILE.missing(&now.values).contains(&"served_last_year"), "and what applies now is asked");
}

fn fields(values: Value) -> ChangeInput {
    ChangeInput { values: values.as_object().unwrap().clone() }
}

fn change(conn: &mut Connection, values: Value) -> Result<Applied, ApplyError> {
    apply_change(conn, &fields(values), None, &person())
}

/// What a person says in the chat changes only what they said (ERP-11, asked by IA-9): the rest of its window and of
/// the profile stays, and the platform never sends back what is saved.
#[test]
fn a_change_touches_only_the_fields_it_names() {
    let mut conn = db();
    save(&mut conn, "identity", identity()).unwrap();
    save(&mut conn, "contact", json!({ "contact_phone": "33 1111 2222", "contact_email": "contacto@ejemplo.org", "municipality": "Zapopan" })).unwrap();
    let p = prepare_change(&conn, &fields(json!({ "contact_phone": " 55 1234 5678 " }))).unwrap();
    let changed: Vec<(&str, &Value)> = p.changes.iter().map(|c| (c.field.as_str(), &c.after)).collect();
    assert_eq!(changed, [("contact_phone", &json!("55 1234 5678"))]);
    let applied = change(&mut conn, json!({ "contact_phone": "55 1234 5678", "notes": "Dos pisos" })).unwrap();
    let now = profile(&conn).unwrap();
    assert_eq!(applied.version, 3);
    assert_eq!(
        (&now.values["contact_phone"], &now.values["contact_email"], &now.values["municipality"], &now.values["name"], &now.values["notes"]),
        (&json!("55 1234 5678"), &json!("contacto@ejemplo.org"), &json!("Zapopan"), &json!("Casa Hogar Esperanza"), &json!("Dos pisos"))
    );
    let facts: i64 = conn.query_row("SELECT count(*) FROM kernel_outbox WHERE fact = ?1", [PROFILE_SAVED], |r| r.get(0)).unwrap();
    assert_eq!(facts, 3, "the same fact as saving a window");
    assert!(matches!(change(&mut conn, json!({ "contact_phone": "55 1234 5678" })), Err(ApplyError::NothingToChange)));
}

#[test]
fn a_null_empties_its_field_and_what_no_longer_applies_shows_in_the_proposal() {
    let mut conn = db();
    save(&mut conn, "identity", identity()).unwrap();
    save(&mut conn, "capacity", json!({ "capacity_total": 40, "served_estimate": 35, "notes": "Dos pisos" })).unwrap();
    change(&mut conn, json!({ "notes": null })).unwrap();
    assert!(!profile(&conn).unwrap().values.contains_key("notes"));
    // it no longer lodges anyone: what only applied to lodging goes empty, and the person sees it before approving
    let p = prepare_change(&conn, &fields(json!({ "modalities": ["community"] }))).unwrap();
    let emptied: Vec<&str> = p.changes.iter().filter(|c| c.after.is_null()).map(|c| c.field.as_str()).collect();
    assert_eq!(emptied, ["capacity_total", "served_estimate"]);
}

#[test]
fn a_change_passes_the_same_validation_and_knows_only_the_fields_of_the_profile() {
    let mut conn = db();
    let invalid = |conn: &Connection, values: Value| -> Vec<(String, String)> {
        prepare_change(conn, &fields(values)).unwrap().invalid.into_iter().map(|i| (i.field, i.reason)).collect()
    };
    let pair = |f: &str, r: &str| (f.to_string(), r.to_string());
    assert_eq!(invalid(&conn, json!({ "contact_phone": "55 1234 5678" })), [pair("name", "required")], "a profile still empty needs its name");
    save(&mut conn, "identity", identity()).unwrap();
    assert_eq!(invalid(&conn, json!({ "postal_code": "0670", "legal_rfc": "chE850101ab1" })), [pair("postal_code", "postal_code_invalid")]);
    assert_eq!(invalid(&conn, json!({ "patronato": "Rosa" })), [pair("patronato", "unknown_field")]);
    assert_eq!(invalid(&conn, json!({ "name": null })), [pair("name", "required")]);
    // an RFC is kept in capitals, as the SAT writes it
    change(&mut conn, json!({ "legal_rfc": "che850101ab1" })).unwrap();
    assert_eq!(profile(&conn).unwrap().values["legal_rfc"], json!("CHE850101AB1"));
    assert!(prepare_change(&conn, &fields(json!({}))).unwrap().changes.is_empty(), "no field, nothing to propose");
}

#[test]
fn what_does_not_pass_is_not_saved() {
    let mut conn = db();
    let invalid = |conn: &Connection, id: &str, values: Value| -> Vec<(String, String)> {
        prepare(conn, &window(id, values)).unwrap().invalid.into_iter().map(|i| (i.field, i.reason)).collect()
    };
    let pair = |f: &str, r: &str| (f.to_string(), r.to_string());
    assert_eq!(invalid(&conn, "identity", json!({ "mission": "Sin nombre" })), [pair("name", "required")]);
    // without its name the profile is not saved, whatever the window (as before the modules)
    assert_eq!(invalid(&conn, "legal", json!({ "purpose": "Cuidar" })), [pair("name", "required")]);
    save(&mut conn, "identity", identity()).unwrap();
    assert_eq!(invalid(&conn, "contact", json!({ "postal_code": "0670", "municipality": "Zapopan 45010" })), [
        pair("postal_code", "postal_code_invalid"),
        pair("municipality", "place_name_invalid")
    ]);
    assert_eq!(invalid(&conn, "identity", json!({ "name": "Casa", "populations": ["adults"], "age_min": 60, "age_max": 18 })), [pair("age_max", "age_range")]);
    assert_eq!(invalid(&conn, "legal", json!({ "founded_year": 2099 })), [pair("founded_year", "year_invalid")]);
    assert_eq!(invalid(&conn, "legal", json!({ "name": "Casa" })), [pair("name", "not_in_window")]);
    assert_eq!(invalid(&conn, "board", json!({})), [pair("window", "unknown_window")]);
    assert!(matches!(save(&mut conn, "identity", json!({ "mission": "Sin nombre" })), Err(ApplyError::Invalid(_))));
    assert_eq!(profile(&conn).unwrap().version, 1);
}

/// What is not public nor internal says in the history only that it changed: the address, the RFC, and the name of
/// the legal representative, a person.
#[test]
fn the_history_protects_what_the_catalog_protects() {
    let mut conn = db();
    save(&mut conn, "identity", identity()).unwrap();
    save(&mut conn, "legal", json!({ "legal_name": "Casa Hogar Esperanza, I.A.P.", "legal_rfc": "CHE850101AB1", "legal_rep_name": "Rosa Pérez" })).unwrap();
    let history = kernel::history(&conn, &profile_ref()).unwrap();
    let kept: Vec<(&str, bool, &Value)> = history[1].fields.iter().map(|c| (c.field.as_str(), c.protected, &c.after)).collect();
    assert_eq!(kept, [
        ("legal_name", false, &json!("Casa Hogar Esperanza, I.A.P.")),
        ("legal_rep_name", true, &Value::Null),
        ("legal_rfc", true, &Value::Null)
    ]);
}

#[test]
fn each_datum_says_where_it_comes_from() {
    let mut conn = db();
    save(&mut conn, "identity", identity()).unwrap();
    let document = Origin::Document { document: "doc_01JB3M7Q8ZK4X2N6V9R5T0W1YE".into(), page: Some(2), account: ROSA.into() };
    let mut legal = identity();
    legal["mission"] = json!("Cuidar con dignidad");
    apply(&mut conn, &window("identity", legal), None, &document).unwrap();
    let mut emptied = identity();
    emptied["mission"] = Value::Null;
    emptied["name"] = json!("Casa Hogar Esperanza");
    save(&mut conn, "identity", emptied).unwrap();
    let origins = origins(&conn).unwrap();
    assert_eq!(origins["name"].origin, person());
    assert!(!origins.contains_key("mission"), "an emptied datum has no origin");
    assert_eq!(missing(&conn).unwrap()[0], "mission");
}

/// A database of before the modules keeps its profile and its history: each earlier change at its version, and what
/// the history did not account for as one more change, written by a person. Once.
#[test]
fn a_database_of_before_keeps_its_profile_and_its_history() {
    let mut conn = db();
    let values = json!({ "name": "Casa Hogar Esperanza", "mission": "Cuidar", "legal_rfc": "CHE850101AB1", "street": "Hidalgo 12", "notes": "" });
    let earlier = vec![
        Earlier {
            at: "2026-05-01T10:00:00Z".into(),
            origin: Origin::Person { account: ROSA.into() },
            fields: vec![FieldChange { field: "name".into(), before: Value::Null, after: json!("Casa Hogar Esperanza"), protected: false }],
        },
        Earlier {
            at: "2026-05-02T10:00:00Z".into(),
            origin: Origin::Person { account: String::new() },
            fields: vec![FieldChange { field: "legal_rfc".into(), before: Value::Null, after: Value::Null, protected: true }],
        },
    ];
    assert_eq!(adopt(&mut conn, values.as_object().unwrap(), earlier.clone()).unwrap(), 3);
    let now = profile(&conn).unwrap();
    assert_eq!(now.version, 3);
    assert_eq!(now.values.len(), 4, "an empty text is not kept");
    let history = kernel::history(&conn, &profile_ref()).unwrap();
    let rest: Vec<&str> = history[2].fields.iter().map(|c| c.field.as_str()).collect();
    assert_eq!(rest, ["mission", "street"], "the RFC is accounted for by its line, even without its value");
    assert_eq!(history[2].origin, Origin::Person { account: String::new() });
    assert!(matches!(adopt(&mut conn, values.as_object().unwrap(), earlier), Err(ApplyError::HasHistory)));
    // and from there it changes like any profile
    save(&mut conn, "identity", identity()).unwrap();
    assert_eq!(profile(&conn).unwrap().version, 4);
    // a database with no profile has nothing to take over
    assert_eq!(adopt(&mut db(), &Map::new(), vec![]).unwrap(), 0);
}
