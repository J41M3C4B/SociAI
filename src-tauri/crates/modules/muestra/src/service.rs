//! Saving a note, prepared and then applied on the kernel (ERP-9): the same path a person and a task of the platform
//! take. The kernel validates it and protects its phone with the module's part of the dictionary (ERP-2): nothing of
//! that is written here.

use kernel::{Applied, ApplyError, Changeable, Draft, Origin, Proposal, Ref};
use rusqlite::Connection;
use serde_json::{json, Map, Value};

use crate::catalog::NOTE;
use crate::contract::{NoteInput, NoteRef};
use crate::storage;

/// What happened, for whoever reacts to it.
pub const NOTE_SAVED: &str = "muestra.nota_guardada";

pub fn note_ref(id: &str) -> Ref {
    Ref::new("muestra", "note", id)
}

/// What the kernel needs to save one note.
struct SaveNote<'a>(&'a NoteInput);

fn fields(title: &str, body: &Option<String>, phone: &Option<String>) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("title".into(), json!(title));
    m.insert("body".into(), json!(body));
    m.insert("phone".into(), json!(phone));
    m
}

impl Changeable for SaveNote<'_> {
    fn prepare(&self, conn: &Connection) -> rusqlite::Result<Proposal> {
        let input = self.0;
        let now = storage::find(conn, &NoteRef(input.id.clone()))?;
        let as_json = serde_json::to_value(input).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        Ok(kernel::propose(
            &NOTE,
            Draft {
                entity: note_ref(&input.id),
                version: now.as_ref().map_or(0, |n| n.version),
                before: now.as_ref().map(|n| fields(&n.title, &n.body, &n.phone)),
                after: fields(&input.title, &input.body, &input.phone),
                invalid: vec![],
                input: &as_json,
            },
        ))
    }

    fn version(&self, conn: &Connection) -> rusqlite::Result<i64> {
        Ok(storage::find(conn, &NoteRef(self.0.id.clone()))?.map_or(0, |n| n.version))
    }

    fn write(&self, conn: &Connection, proposal: &Proposal) -> rusqlite::Result<()> {
        storage::save(conn, self.0, proposal.base_version)
    }
}

/// What saving this note would do. Writes nothing.
pub fn prepare(conn: &Connection, input: &NoteInput) -> rusqlite::Result<Proposal> {
    SaveNote(input).prepare(conn)
}

/// Saves the note, if it is still as it was when `approved` was prepared (when there was an approval).
pub fn apply(conn: &mut Connection, input: &NoteInput, approved: Option<&str>, origin: &Origin) -> Result<Applied, ApplyError> {
    kernel::apply(conn, &SaveNote(input), approved, origin, NOTE_SAVED)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::Invalid;

    const ROSA: &str = "usr_01JB3M7Q8ZK4X2N6V9R5T0W1YD";
    const TASK: &str = "task_01JB3M7Q8ZK4X2N6V9R5T0W1YC";

    fn db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        kernel::migrate(&mut conn, &kernel::Modules::discover().unwrap()).unwrap();
        conn
    }

    fn person() -> Origin {
        Origin::Person { account: ROSA.into() }
    }

    fn input(id: &str, title: &str) -> NoteInput {
        NoteInput { id: id.into(), title: title.into(), body: None, phone: None }
    }

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }

    /// What everything a change leaves behind adds up to: the note, its change, its history and its fact.
    fn traces(conn: &Connection) -> [i64; 4] {
        ["muestra_note", "kernel_change", "kernel_history", "kernel_outbox"].map(|t| count(conn, t))
    }

    #[test]
    fn preparing_writes_nothing_and_says_what_would_change() {
        let mut conn = db();
        let id = kernel::new_id("nota");
        let new = NoteInput { phone: Some("55 1234 5678".into()), ..input(&id, "Primera") };
        let p = prepare(&conn, &new).unwrap();
        assert_eq!(traces(&conn), [0, 0, 0, 0], "preparing writes nothing");
        assert_eq!((p.entity.clone(), p.base_version), (note_ref(&id), 0));
        let changed: Vec<(&str, bool)> = p.changes.iter().map(|c| (c.field.as_str(), c.protected)).collect();
        assert_eq!(changed, [("phone", true), ("title", false)], "body stays null: it does not change");
        assert!(p.invalid.is_empty());
        assert_eq!(p.fingerprint, kernel::fingerprint(&p.entity, 0, &serde_json::to_value(&new).unwrap()));

        apply(&mut conn, &new, Some(&p.fingerprint), &person()).unwrap();
        let again = prepare(&conn, &NoteInput { title: "Segunda".into(), ..new.clone() }).unwrap();
        assert_eq!(again.base_version, 1);
        assert_eq!(again.changes.len(), 1);
        assert_eq!((again.changes[0].before.clone(), again.changes[0].after.clone()), (json!("Primera"), json!("Segunda")));
        let bad = prepare(&conn, &NoteInput { title: " ".into(), phone: Some("llámale a Rosa".into()), ..new }).unwrap();
        let reasons: Vec<(&str, &str)> = bad.invalid.iter().map(|i| (i.field.as_str(), i.reason.as_str())).collect();
        assert_eq!(reasons, [("title", "required"), ("phone", "not_a_phone")]);
    }

    #[test]
    fn applying_writes_the_datum_its_history_its_origin_and_its_fact_together() {
        let mut conn = db();
        let id = kernel::new_id("nota");
        let new = input(&id, "Primera");
        let p = prepare(&conn, &new).unwrap();
        let done = apply(&mut conn, &new, Some(&p.fingerprint), &person()).unwrap();
        assert_eq!((done.entity.clone(), done.version), (note_ref(&id), 1));
        assert_eq!(storage::find(&conn, &NoteRef(id.clone())).unwrap().map(|n| (n.title, n.version)), Some(("Primera".into(), 1)));
        assert_eq!(traces(&conn), [1, 1, 1, 1]);
        let fact: (String, String) = conn.query_row("SELECT fact, entity_id FROM kernel_outbox", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(fact, (NOTE_SAVED.into(), id.clone()), "a fact carries ids and a code");

        // a task proposes the second version and Rosa accepts it
        let second = input(&id, "Segunda");
        let p2 = prepare(&conn, &second).unwrap();
        apply(&mut conn, &second, Some(&p2.fingerprint), &Origin::Task { task: TASK.into(), account: ROSA.into() }).unwrap();
        let history = kernel::history(&conn, &note_ref(&id)).unwrap();
        assert_eq!(history.iter().map(|r| r.version).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(history[0].origin, person());
        assert_eq!(history[1].origin, Origin::Task { task: TASK.into(), account: ROSA.into() });
        assert_eq!(history[1].fields.len(), 1);
        assert_eq!((history[1].fields[0].before.clone(), history[1].fields[0].after.clone()), (json!("Primera"), json!("Segunda")));
    }

    #[test]
    fn a_change_prepared_against_an_older_datum_or_another_input_is_not_applied() {
        let mut conn = db();
        let id = kernel::new_id("nota");
        apply(&mut conn, &input(&id, "Primera"), None, &person()).unwrap();
        // Rosa prepares and approves a title; meanwhile somebody else saves another one
        let rosa = input(&id, "La de Rosa");
        let approved = prepare(&conn, &rosa).unwrap().fingerprint;
        apply(&mut conn, &input(&id, "La de otra persona"), None, &person()).unwrap();
        let before = traces(&conn);
        assert!(matches!(apply(&mut conn, &rosa, Some(&approved), &person()), Err(ApplyError::Stale)), "the datum changed");
        assert_eq!(traces(&conn), before, "nothing was written");
        // and an input other than the approved one does not pass either
        let fresh = prepare(&conn, &rosa).unwrap().fingerprint;
        let other = input(&id, "Otro título que nadie aprobó");
        assert!(matches!(apply(&mut conn, &other, Some(&fresh), &person()), Err(ApplyError::Stale)), "the input changed");
        assert_eq!(storage::find(&conn, &NoteRef(id.clone())).unwrap().unwrap().title, "La de otra persona");
        // prepared again, it is applied
        assert_eq!(apply(&mut conn, &rosa, Some(&fresh), &person()).unwrap().version, 3);
    }

    /// Whatever fails in the middle, the transaction leaves nothing: here the fact cannot be written, after the note
    /// and its history were.
    #[test]
    fn if_anything_fails_nothing_is_left() {
        let mut conn = db();
        conn.execute_batch("CREATE TEMP TRIGGER no_facts BEFORE INSERT ON kernel_outbox BEGIN SELECT RAISE(ABORT, 'no facts'); END;").unwrap();
        let new = input(&kernel::new_id("nota"), "Primera");
        assert!(matches!(apply(&mut conn, &new, None, &person()), Err(ApplyError::Db(_))));
        assert_eq!(traces(&conn), [0, 0, 0, 0]);
    }

    #[test]
    fn a_fingerprint_is_applied_once_and_the_kernel_says_so() {
        let mut conn = db();
        let new = input(&kernel::new_id("nota"), "Primera");
        let fp = prepare(&conn, &new).unwrap().fingerprint;
        assert_eq!(kernel::applied(&conn, &fp).unwrap(), None);
        let done = apply(&mut conn, &new, Some(&fp), &person()).unwrap();
        assert!(!done.already_applied);
        let earlier = Applied { already_applied: true, ..done.clone() };
        assert_eq!(kernel::applied(&conn, &fp).unwrap(), Some(earlier.clone()));
        // the platform did not hear the answer and tries again: nothing is done twice, and it is a success
        assert_eq!(apply(&mut conn, &new, Some(&fp), &person()).unwrap(), earlier);
        assert_eq!(traces(&conn), [1, 1, 1, 1]);
    }

    /// The kernel knows the note by the module's part of the dictionary: its classes, what does not pass and what is
    /// missing, with no code of the module.
    #[test]
    fn the_kernel_knows_the_note_by_its_dictionary() {
        let modules = kernel::Modules::discover().unwrap();
        assert_eq!(modules.field("muestra.note.phone").map(|f| (f.class, f.protected())), Some(("personal", true)));
        assert_eq!(modules.field("muestra.note.title").map(|f| (f.class, f.protected())), Some(("internal", false)));
        let long = prepare(&db(), &input(&kernel::new_id("nota"), &"a".repeat(121))).unwrap();
        assert_eq!(long.invalid, [Invalid::new("title", "too_long")]);
        let note = modules.entity("muestra", "note").unwrap();
        assert_eq!(note.missing(&fields(" ", &None, &None)), ["title"]);
        assert!(note.missing(&fields("Primera", &None, &None)).is_empty());
    }

    #[test]
    fn what_does_not_pass_or_does_not_change_is_not_applied() {
        let mut conn = db();
        let id = kernel::new_id("nota");
        match apply(&mut conn, &input(&id, ""), None, &person()) {
            Err(ApplyError::Invalid(invalid)) => assert_eq!(invalid, [Invalid::new("title", "required")]),
            other => panic!("{other:?}"),
        }
        apply(&mut conn, &input(&id, "Primera"), None, &person()).unwrap();
        assert!(matches!(apply(&mut conn, &input(&id, "Primera"), None, &person()), Err(ApplyError::NothingToChange)));
        let rosa_by_name = Origin::Person { account: "Rosa Hernández".into() };
        assert!(matches!(apply(&mut conn, &input(&id, "Segunda"), None, &rosa_by_name), Err(ApplyError::BadOrigin)));
        assert_eq!(traces(&conn), [1, 1, 1, 1]);
    }

    /// The history of a protected field says it changed, and keeps no value; and the history only grows.
    #[test]
    fn the_history_keeps_no_protected_value_and_only_grows() {
        let mut conn = db();
        let id = kernel::new_id("nota");
        let new = NoteInput { phone: Some("55 1234 5678".into()), ..input(&id, "Primera") };
        apply(&mut conn, &new, None, &person()).unwrap();
        let history = kernel::history(&conn, &note_ref(&id)).unwrap();
        let phone = history[0].fields.iter().find(|f| f.field == "phone").unwrap();
        assert!(phone.protected && phone.before.is_null() && phone.after.is_null());
        let kept: i64 = conn.query_row("SELECT count(*) FROM kernel_history WHERE before LIKE '%1234%' OR after LIKE '%1234%'", [], |r| r.get(0)).unwrap();
        assert_eq!(kept, 0, "the number is nowhere in the history");
        for sql in ["UPDATE kernel_history SET after = '\"otro\"'", "DELETE FROM kernel_history", "DELETE FROM kernel_change", "UPDATE kernel_change SET version = 9"] {
            let err = conn.execute(sql, []).unwrap_err().to_string();
            assert!(err.contains("the history only grows"), "{sql}: {err}");
        }
    }
}
