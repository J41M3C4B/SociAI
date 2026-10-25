//! The table of Mi institución and all of its SQL. The profile is one row, with a column per field of the catalog,
//! named as the field: what is read and written goes by the catalog, never by a list kept here.

use kernel::{Kind, Proposal};
use rusqlite::types::{Value as Sql, ValueRef};
use rusqlite::{params_from_iter, Connection, OptionalExtension};
use serde_json::{json, Map, Value};

use crate::catalog::PROFILE;

/// The profile as it is kept: its version and its filled fields, by id.
#[derive(Debug, Clone, PartialEq)]
pub struct Stored {
    pub version: i64,
    pub values: Map<String, Value>,
}

fn columns() -> String {
    PROFILE.fields.iter().map(|f| f.id).collect::<Vec<_>>().join(", ")
}

/// The profile, if there is one. An empty column is left out.
pub fn find(conn: &Connection, id: &str) -> rusqlite::Result<Option<Stored>> {
    conn.query_row(&format!("SELECT version, {} FROM mi_institucion_profile WHERE id = ?1", columns()), [id], |r| {
        let mut values = Map::new();
        for (n, f) in PROFILE.fields.iter().enumerate() {
            let value = match r.get_ref(n + 1)? {
                ValueRef::Integer(i) => json!(i),
                ValueRef::Text(t) => {
                    let text = String::from_utf8_lossy(t).into_owned();
                    match f.kind {
                        Kind::MultiSelect => serde_json::from_str(&text).unwrap_or(Value::Null),
                        _ => Value::String(text),
                    }
                }
                _ => Value::Null,
            };
            if kernel::is_filled(Some(&value)) {
                values.insert(f.id.to_string(), value);
            }
        }
        Ok(Stored { version: r.get(0)?, values })
    })
    .optional()
}

/// A value as its column keeps it: a list of codes as JSON, a text trimmed, nothing for an empty one.
fn sql(v: &Value) -> Sql {
    match v {
        Value::String(s) if !s.trim().is_empty() => Sql::Text(s.trim().to_string()),
        Value::Number(n) => n.as_i64().map_or(Sql::Null, Sql::Integer),
        Value::Bool(b) => Sql::Integer(i64::from(*b)),
        Value::Array(a) if !a.is_empty() => Sql::Text(v.to_string()),
        _ => Sql::Null,
    }
}

/// Writes what the proposal says, field by field, at the next version: a new profile at version 1, or the one that is
/// still at `base`. A field the proposal empties (it does not apply any more) is emptied.
pub fn save(conn: &Connection, id: &str, proposal: &Proposal) -> rusqlite::Result<()> {
    let base = proposal.base_version;
    let changes: Vec<_> = proposal.changes.iter().filter(|c| PROFILE.field(&c.field).is_some()).collect();
    if base == 0 {
        conn.execute(
            "INSERT INTO mi_institucion_profile (id, version, created_at, updated_at)
             VALUES (?1, 0, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
            [id],
        )?;
    }
    let sets: String = changes.iter().map(|c| format!("{} = ?, ", c.field)).collect();
    let mut values: Vec<Sql> = changes.iter().map(|c| sql(&c.after)).collect();
    values.extend([Sql::Text(id.to_string()), Sql::Integer(base)]);
    conn.execute(
        &format!(
            "UPDATE mi_institucion_profile SET {sets}version = version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ? AND version = ?"
        ),
        params_from_iter(values),
    )?;
    Ok(())
}

/// Writes the profile a database already had, at the version its history reached (`kernel::adopt`).
pub fn adopt(conn: &Connection, id: &str, version: i64, values: &Map<String, Value>) -> rusqlite::Result<()> {
    let fields: Vec<_> = PROFILE.fields.iter().filter(|f| values.contains_key(f.id)).collect();
    let names: String = fields.iter().map(|f| format!(", {}", f.id)).collect();
    let marks: String = fields.iter().map(|_| ", ?").collect();
    let mut params: Vec<Sql> = vec![Sql::Text(id.to_string()), Sql::Integer(version)];
    params.extend(fields.iter().map(|f| sql(&values[f.id])));
    conn.execute(
        &format!(
            "INSERT INTO mi_institucion_profile (id, version{names}, created_at, updated_at)
             VALUES (?, ?{marks}, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))"
        ),
        params_from_iter(params),
    )?;
    Ok(())
}
