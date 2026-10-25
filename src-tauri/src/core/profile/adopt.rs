//! A database of before the modules keeps its profile and its history (ERP-1). The first time it opens with the module
//! Mi institución, this reads the profile as the app kept it (`institution`, the latest row of `institution_profile`)
//! and the lines of its history (`core_change`, with the origin of each datum in `core_field`), and gives them to the
//! module, which writes them in its table and in the history of the kernel. Once: a database that has the profile in
//! the module is left as it is. The old columns keep what they had; nothing is deleted.

use std::collections::BTreeMap;

use crate::core::error::ServiceError;
use crate::core::institution::catalog::attention_of_kind;
use crate::core::institution::forms::module_id;
use kernel::{Earlier, FieldChange, Origin};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Map, Value};

/// The columns of `institution` that hold a datum of the profile, named as the field of the module.
const INSTITUTION: &[&str] = &[
    "name", "mission", "legal_rfc", "contact_phone", "contact_email", "legal_rep_name", "state", "municipality", "founded_year", "legal_form",
    "authorized_donee", "cluni", "populations", "sex_served", "modalities", "care_areas", "legal_name", "purpose", "services", "age_min",
    "age_max", "admission_criteria", "street", "ext_number", "int_number", "neighborhood", "postal_code", "tax_regime", "fiscal_postal_code",
    "junta_folio", "donee_category", "donee_letter_number", "donee_letter_date", "cluni_key", "legal_rep_valid_until", "served_last_year",
];

/// The columns of `institution_profile` that hold a datum of the profile.
const PROFILE: &[&str] = &["capacity_total", "notes", "served_estimate", "staff_paid_estimate", "staff_volunteer_estimate"];

/// The lists of codes, kept as a JSON array.
const LISTS: &[&str] = &["populations", "modalities", "care_areas"];

fn row(conn: &Connection, sql: &str, columns: &[&str]) -> Result<Option<Map<String, Value>>, ServiceError> {
    use rusqlite::types::ValueRef;
    Ok(conn
        .query_row(sql, [], |r| {
            let mut v = Map::new();
            for (n, c) in columns.iter().enumerate() {
                let value = match r.get_ref(n)? {
                    ValueRef::Integer(i) => json!(i),
                    ValueRef::Text(t) => {
                        let text = String::from_utf8_lossy(t).into_owned();
                        if LISTS.contains(c) { serde_json::from_str(&text).unwrap_or(Value::Null) } else { Value::String(text) }
                    }
                    _ => Value::Null,
                };
                v.insert(c.to_string(), value);
            }
            Ok(v)
        })
        .optional()?)
}

/// The profile as the app kept it, by the module's ids. An institution of before the attention profile reads its kind
/// as one (`attention_of_kind`), as the app did.
fn values(conn: &Connection) -> Result<Option<Map<String, Value>>, ServiceError> {
    let Some(mut v) = row(conn, &format!("SELECT {}, kind FROM institution LIMIT 1", INSTITUTION.join(", ")), &[INSTITUTION, &["kind"]].concat())? else {
        return Ok(None);
    };
    let latest = format!("SELECT {} FROM institution_profile ORDER BY version DESC LIMIT 1", PROFILE.join(", "));
    v.extend(row(conn, &latest, PROFILE)?.unwrap_or_default());
    let kind = v.remove("kind").and_then(|k| k.as_str().map(str::to_string)).unwrap_or_default();
    if !crate::common::forms::is_filled(v.get("populations")) {
        let (populations, modalities, areas) = attention_of_kind(&kind);
        for (field, codes) in [("populations", populations), ("modalities", modalities), ("care_areas", areas)] {
            if !crate::common::forms::is_filled(v.get(field)) {
                v.insert(field.into(), json!(codes));
            }
        }
    }
    Ok(Some(v))
}

/// One line of the history as the app kept it.
struct Line {
    field: String,
    value: Value,
    origin: String,
    at: String,
    by: Option<String>,
}

/// The history of the profile as the app kept it, oldest first, as the changes of the kernel: the lines written in
/// the same save (same moment, origin and person) are one change. A line says only the new value; the one before is
/// the one the line before said. A document is known only for the last line of its field (`core_field`).
fn earlier(conn: &Connection) -> Result<Vec<Earlier>, ServiceError> {
    let lines: Vec<Line> = conn
        .prepare("SELECT field, value_json, origin, changed_at, changed_by FROM core_change ORDER BY changed_at, id")?
        .query_map([], |r| {
            Ok(Line {
                field: r.get(0)?,
                value: r.get::<_, Option<String>>(1)?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null),
                origin: r.get(2)?,
                at: r.get(3)?,
                by: r.get(4)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    let documents: BTreeMap<String, Value> = conn
        .prepare("SELECT field, source_ref FROM core_field WHERE origin = 'document' AND source_ref IS NOT NULL")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(Result::ok)
        .filter_map(|(f, s)| Some((f, serde_json::from_str(&s).ok()?)))
        .collect();
    let mut told: BTreeMap<String, Value> = BTreeMap::new();
    let mut out: Vec<Earlier> = Vec::new();
    let mut key: Option<(String, String, Option<String>)> = None;
    for line in lines {
        let Some(field) = module_id(&line.field) else { continue };
        let account = line.by.clone().unwrap_or_default();
        let origin = match line.origin.as_str() {
            "document" => {
                let source = documents.get(&line.field);
                let document = source.and_then(|s| s.get("document_id")).and_then(Value::as_str).unwrap_or_default().to_string();
                Origin::Document { document, page: source.and_then(|s| s.get("page")).and_then(Value::as_i64), account }
            }
            "ai_assumption" => Origin::Task { task: String::new(), account },
            "computed" => Origin::Calculated { rule: "computed".into() },
            _ => Origin::Person { account },
        };
        let before = told.insert(field.to_string(), line.value.clone()).unwrap_or(Value::Null);
        let change = FieldChange { field: field.to_string(), before, after: line.value, protected: false };
        let this = (line.at.clone(), line.origin.clone(), line.by.clone());
        match out.last_mut() {
            Some(last) if key.as_ref() == Some(&this) => last.fields.push(change),
            _ => out.push(Earlier { at: line.at, origin, fields: vec![change] }),
        }
        key = Some(this);
    }
    Ok(out)
}

/// Takes over the profile of a database of before the modules, once. Says the version the profile has after it (0
/// when there was nothing to take over, or it was taken over already).
pub fn adopt(conn: &mut Connection) -> Result<i64, ServiceError> {
    if mi_institucion::service::profile(conn)?.version > 0 {
        return Ok(0);
    }
    let Some(values) = values(conn)? else { return Ok(0) };
    let earlier = earlier(conn)?;
    match mi_institucion::service::adopt(conn, &values, earlier) {
        Ok(version) => Ok(version),
        Err(kernel::ApplyError::Db(e)) => Err(e.into()),
        Err(e) => Err(crate::storage::StorageError::Change(e.code()).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let c = crate::storage::open_encrypted(&dir.path().join("t.db"), "k").unwrap();
        (dir, c)
    }

    /// A profile kept by the app before the modules: its institution, its latest row and the lines of its history.
    fn before_the_modules(c: &Connection) {
        c.execute_batch(
            r#"INSERT INTO institution (id, name, kind, mission, legal_rfc, populations, modalities, care_areas, legal_name, street, municipality,
                                        created_at, updated_at)
               VALUES ('inst_1', 'Asilo Ficticio', 'elderly_home', 'Cuidar.', 'AFI870101AB1', '[]', '[]', '[]', 'Asilo Ficticio, I.A.P.',
                       'Calle Ficticia', 'Zapopan', 't', 't');
               INSERT INTO institution_profile (id, institution_id, version, capacity_total, notes, created_at, confirmed_at)
               VALUES ('prof_1', 'inst_1', 1, 30, 'Viejo', 't', 't');
               INSERT INTO institution_profile (id, institution_id, version, capacity_total, notes, served_estimate, created_at)
               VALUES ('prof_2', 'inst_1', 2, 40, NULL, 35, 't');
               INSERT INTO core_change (field, value_json, protected, origin, changed_at, changed_by)
               VALUES ('institution.name', '"Asilo"', 0, 'user', '2026-05-01T10:00:00Z', 'usr_01JB3M7Q8ZK4X2N6V9R5T0W1YD'),
                      ('institution.legal_rfc', NULL, 1, 'user', '2026-05-01T10:00:00Z', 'usr_01JB3M7Q8ZK4X2N6V9R5T0W1YD'),
                      ('institution.name', '"Asilo Ficticio"', 0, 'document', '2026-06-01T10:00:00Z', NULL),
                      ('institution.board_member', NULL, 1, 'user', '2026-06-02T10:00:00Z', NULL);
               INSERT INTO core_field (field, origin, source_ref, confirmed_at, updated_at)
               VALUES ('institution.name', 'document', '{"document_id":"doc_01JB3M7Q8ZK4X2N6V9R5T0W1YE","page":3}', 't', 't');"#,
        )
        .unwrap();
    }

    #[test]
    fn a_database_of_before_keeps_its_profile_and_its_history() {
        let (_d, mut c) = conn();
        before_the_modules(&c);
        // the two saves of the history and one more for what it does not account for
        assert_eq!(adopt(&mut c).unwrap(), 3);
        let now = mi_institucion::service::profile(&c).unwrap();
        assert_eq!(now.values["name"], json!("Asilo Ficticio"));
        assert_eq!((now.values["capacity_total"].clone(), now.values["served_estimate"].clone()), (json!(40), json!(35)), "from the latest row");
        assert!(!now.values.contains_key("notes"));
        assert_eq!(now.values["populations"], json!(["older_adults"]), "the kind read as an attention profile");
        let history = kernel::history(&c, &mi_institucion::service::profile_ref()).unwrap();
        assert_eq!(history[0].origin, Origin::Person { account: "usr_01JB3M7Q8ZK4X2N6V9R5T0W1YD".into() });
        assert_eq!(history[0].fields.iter().map(|f| f.field.as_str()).collect::<Vec<_>>(), ["name", "legal_rfc"]);
        assert_eq!(history[1].origin, Origin::Document { document: "doc_01JB3M7Q8ZK4X2N6V9R5T0W1YE".into(), page: Some(3), account: String::new() });
        assert_eq!((history[1].fields[0].before.clone(), history[1].fields[0].after.clone()), (json!("Asilo"), json!("Asilo Ficticio")));
        assert!(!history[2].fields.iter().any(|f| f.field == "name" || f.field == "legal_rfc"), "what the history tells is not told again");
        // and the screen sees the same profile, with where each datum comes from
        let view = crate::core::profile::forms::get(&c, "institution.identity").unwrap();
        assert_eq!(view.values["institution.name"], json!("Asilo Ficticio"));
        assert_eq!(view.origins["institution.name"].origin, "document");
        // once
        assert_eq!(adopt(&mut c).unwrap(), 0);
        assert_eq!(kernel::history(&c, &mi_institucion::service::profile_ref()).unwrap().len(), 3);
    }

    #[test]
    fn a_new_database_has_nothing_to_take_over() {
        let (_d, mut c) = conn();
        assert_eq!(adopt(&mut c).unwrap(), 0);
        assert!(crate::core::profile::storage::load_current(&c).unwrap().is_none());
    }
}
