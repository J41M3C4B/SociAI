//! The profile of the institution on the kernel: read it, save a window of it or change only some of its fields (each
//! prepared and then applied, ERP-9), and take over the profile a database of before the modules already had. The kernel validates it with the catalog
//! (ERP-2), protects what is not public nor internal and keeps its history with the origin of each change; only what
//! the dictionary cannot say is written here.

use std::collections::BTreeMap;

use kernel::{Applied, ApplyError, Changeable, Draft, Earlier, FieldChange, Invalid, Origin, Proposal, Ref};
use rusqlite::Connection;
use serde_json::{Map, Value};

use crate::catalog::PROFILE;
use crate::contract::{ChangeInput, WindowInput, LATEST_YEAR};
use crate::storage;
use crate::windows;

/// What happened, for whoever reacts to it.
pub const PROFILE_SAVED: &str = "mi_institucion.perfil_guardado";

/// The id of the profile: one per database, which is one per institution (D4).
pub const PROFILE_ID: &str = "profile";

pub fn profile_ref() -> Ref {
    Ref::new("mi_institucion", PROFILE.id, PROFILE_ID)
}

/// The profile as it is now: its version (0 when it does not exist yet) and its filled fields, by id.
pub fn profile(conn: &Connection) -> rusqlite::Result<storage::Stored> {
    Ok(storage::find(conn, PROFILE_ID)?.unwrap_or(storage::Stored { version: 0, values: Map::new() }))
}

/// What is missing of the whole profile, in the order of the catalog.
pub fn missing(conn: &Connection) -> rusqlite::Result<Vec<&'static str>> {
    Ok(PROFILE.missing(&profile(conn)?.values))
}

/// Values as the kernel compares them: texts trimmed, an RFC in capitals as the SAT writes it, and nothing for what is
/// empty.
fn clean(values: &Map<String, Value>) -> Map<String, Value> {
    values
        .iter()
        .filter_map(|(k, v)| {
            let rfc = PROFILE.field(k).is_some_and(|f| f.rule == Some(kernel::Rule::RfcMoral));
            let v = match v {
                Value::String(s) if rfc => Value::String(s.trim().to_uppercase()),
                Value::String(s) => Value::String(s.trim().to_string()),
                other => other.clone(),
            };
            kernel::is_filled(Some(&v)).then(|| (k.clone(), v))
        })
        .collect()
}

/// What the dictionary cannot say: the youngest age served is not over the oldest, and the institution was not
/// founded in a year still to come.
fn own_rules(conn: &Connection, after: &Map<String, Value>) -> rusqlite::Result<Vec<Invalid>> {
    let mut shown = after.clone();
    PROFILE.clear_hidden(&mut shown);
    let number = |f: &str| shown.get(f).and_then(Value::as_i64);
    let mut out = Vec::new();
    if let (Some(min), Some(max)) = (number("age_min"), number("age_max")) {
        if min > max {
            out.push(Invalid::new("age_max", "age_range"));
        }
    }
    let year: i64 = conn.query_row("SELECT CAST(strftime('%Y','now') AS INTEGER)", [], |r| r.get(0))?;
    // a year out of the range of its field is the dictionary's to refuse
    if number("founded_year").is_some_and(|y| y > year && y <= LATEST_YEAR) {
        out.push(Invalid::new("founded_year", "year_invalid"));
    }
    Ok(out)
}

/// What the kernel needs to save one window of the profile.
struct SaveWindow<'a>(&'a WindowInput);

impl Changeable for SaveWindow<'_> {
    fn prepare(&self, conn: &Connection) -> rusqlite::Result<Proposal> {
        let input = self.0;
        let now = profile(conn)?;
        let sent = clean(&input.values);
        let mut after = now.values.clone();
        let mut invalid = Vec::new();
        match windows::window(&input.window) {
            Some(window) => {
                for f in window.fields() {
                    match sent.get(f) {
                        Some(v) => after.insert(f.to_string(), v.clone()),
                        None => after.remove(f),
                    };
                }
                invalid.extend(input.values.keys().filter(|k| !window.has(k)).map(|k| Invalid::new(k, "not_in_window")));
            }
            None => invalid.push(Invalid::new("window", "unknown_window")),
        }
        propose(conn, now, after, invalid, input)
    }

    fn version(&self, conn: &Connection) -> rusqlite::Result<i64> {
        Ok(profile(conn)?.version)
    }

    fn write(&self, conn: &Connection, proposal: &Proposal) -> rusqlite::Result<()> {
        storage::save(conn, PROFILE_ID, proposal)
    }
}

/// What the kernel needs to change the fields an input names, and only those (ERP-11, asked by IA-9): what a person
/// says in the chat changes what they said, and the platform never has to send back what is saved.
struct ChangeFields<'a>(&'a ChangeInput);

impl Changeable for ChangeFields<'_> {
    fn prepare(&self, conn: &Connection) -> rusqlite::Result<Proposal> {
        let input = self.0;
        let now = profile(conn)?;
        let sent = clean(&input.values);
        let mut after = now.values.clone();
        let mut invalid = Vec::new();
        for f in input.values.keys() {
            if PROFILE.field(f).is_none() {
                invalid.push(Invalid::new(f, "unknown_field"));
                continue;
            }
            // a `null`, or a value that is empty once cleaned, empties the field
            match sent.get(f) {
                Some(v) => after.insert(f.clone(), v.clone()),
                None => after.remove(f),
            };
        }
        propose(conn, now, after, invalid, input)
    }

    fn version(&self, conn: &Connection) -> rusqlite::Result<i64> {
        Ok(profile(conn)?.version)
    }

    fn write(&self, conn: &Connection, proposal: &Proposal) -> rusqlite::Result<()> {
        storage::save(conn, PROFILE_ID, proposal)
    }
}

/// The kernel's proposal of the profile as it would be: the dictionary's validation, then the module's own rules.
/// What no longer applies goes empty, whatever window it is in.
fn propose(conn: &Connection, now: storage::Stored, after: Map<String, Value>, mut invalid: Vec<Invalid>, input: &impl serde::Serialize) -> rusqlite::Result<Proposal> {
    invalid.extend(own_rules(conn, &after)?);
    let as_json = serde_json::to_value(input).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    Ok(kernel::propose(
        &PROFILE,
        Draft { entity: profile_ref(), version: now.version, before: (now.version > 0).then_some(now.values), after, invalid, input: &as_json },
    ))
}

/// What saving this window would do. Writes nothing.
pub fn prepare(conn: &Connection, input: &WindowInput) -> rusqlite::Result<Proposal> {
    SaveWindow(input).prepare(conn)
}

/// Saves the window, if the profile is still as it was when `approved` was prepared (when there was an approval).
pub fn apply(conn: &mut Connection, input: &WindowInput, approved: Option<&str>, origin: &Origin) -> Result<Applied, ApplyError> {
    kernel::apply(conn, &SaveWindow(input), approved, origin, PROFILE_SAVED)
}

/// What changing these fields would do. Writes nothing.
pub fn prepare_change(conn: &Connection, input: &ChangeInput) -> rusqlite::Result<Proposal> {
    ChangeFields(input).prepare(conn)
}

/// Changes these fields, and only these, if the profile is still as it was when `approved` was prepared.
pub fn apply_change(conn: &mut Connection, input: &ChangeInput, approved: Option<&str>, origin: &Origin) -> Result<Applied, ApplyError> {
    kernel::apply(conn, &ChangeFields(input), approved, origin, PROFILE_SAVED)
}

/// Where a datum of the profile comes from: the last change that wrote it.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub origin: Origin,
    pub at: String,
}

/// The origin of each field the history says was written last, by id. A field emptied by its last change has none.
pub fn origins(conn: &Connection) -> rusqlite::Result<BTreeMap<String, Source>> {
    let mut out = BTreeMap::new();
    for record in kernel::history(conn, &profile_ref())? {
        for c in record.fields {
            if c.protected || !c.after.is_null() {
                out.insert(c.field, Source { origin: record.origin.clone(), at: record.at.clone() });
            } else {
                out.remove(&c.field);
            }
        }
    }
    Ok(out)
}

/// Takes over the profile a database of before the modules already had: its values and its history, oldest first,
/// as the app kept them. What the history does not account for (data written before it was kept) is one more change,
/// written by a person, now. Once: a database that has the profile already is left as it is. Returns the version.
pub fn adopt(conn: &mut Connection, values: &Map<String, Value>, earlier: Vec<Earlier>) -> Result<i64, ApplyError> {
    let tx = conn.transaction()?;
    if storage::find(&tx, PROFILE_ID)?.is_some() {
        return Err(ApplyError::HasHistory);
    }
    let values: Map<String, Value> = clean(values).into_iter().filter(|(k, _)| PROFILE.field(k).is_some()).collect();
    // how the profile was at the end of its history, as far as the history says it
    let mut told: BTreeMap<&str, Value> = BTreeMap::new();
    let mut mentioned: Vec<&str> = Vec::new();
    for c in earlier.iter().flat_map(|e| &e.fields) {
        mentioned.push(&c.field);
        told.insert(&c.field, c.after.clone());
    }
    let rest: Vec<FieldChange> = PROFILE
        .fields
        .iter()
        .filter_map(|f| {
            let now = values.get(f.id).cloned().unwrap_or(Value::Null);
            // the old history does not keep the value of a protected datum: it accounts for it if it names it
            let then = if f.protected() && mentioned.contains(&f.id) { now.clone() } else { told.get(f.id).cloned().unwrap_or(Value::Null) };
            (now != then).then(|| FieldChange { field: f.id.into(), before: then, after: now, protected: f.protected() })
        })
        .collect();
    let mut earlier = earlier;
    if !rest.is_empty() {
        let at: String = tx.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now')", [], |r| r.get(0))?;
        earlier.push(Earlier { at, origin: Origin::Person { account: String::new() }, fields: rest });
    }
    if earlier.is_empty() {
        return Ok(0);
    }
    let version = kernel::adopt(&tx, &PROFILE, &profile_ref(), &earlier)?;
    storage::adopt(&tx, PROFILE_ID, version, &values)?;
    tx.commit()?;
    Ok(version)
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
