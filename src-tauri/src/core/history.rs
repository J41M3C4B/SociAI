//! Where each datum of the institution comes from and how it changed (ADR-033 §4, audit D6).
//!
//! The profile keeps its history in the kernel since it became the module Mi institución (ERP-1): the lines it had
//! here were taken in there once (`profile::adopt`). What stays here is the history of the lists of the institution
//! (its board, its accounts and its record): `core_change` adds one line per change and never edits nor deletes one,
//! and a list says only that it changed, never a value. It is written in the same transaction as the datum.

use crate::storage::StorageError;
use rusqlite::{params, Connection};
use serde::Serialize;

/// Where a datum comes from (principle 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Source {
    /// `user`, `document`, `ai_assumption` or `computed`.
    pub origin: &'static str,
    /// A document and its page, when it came from one.
    pub source_ref: Option<String>,
}

impl Source {
    /// A person wrote it.
    pub fn user() -> Self {
        Source { origin: "user", source_ref: None }
    }
}

/// The origin of a datum written today, as the screen reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldOrigin {
    pub origin: String,
    pub source_ref: Option<String>,
    /// `None` for a datum written before origins were kept.
    pub confirmed_at: Option<String>,
}

/// Writes that a list of the institution's own and protected changed (a board member, a bank account): one line with
/// the id of its form and no value, ever (ADR-033 §4). Who acts comes from the session of the connection, as in the
/// audit log.
pub fn record_list_change(conn: &Connection, form: &str, source: &Source) -> Result<(), StorageError> {
    conn.execute_batch("CREATE TEMP TABLE IF NOT EXISTS session_actor (user_id TEXT);")?;
    conn.execute(
        "INSERT INTO core_change (field, value_json, protected, origin, changed_at, changed_by)
         VALUES (?1, NULL, 1, ?2, strftime('%Y-%m-%dT%H:%M:%SZ','now'), (SELECT user_id FROM temp.session_actor LIMIT 1))",
        params![form, source.origin],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let c = crate::storage::open_encrypted(&dir.path().join("t.db"), "k").unwrap();
        (dir, c)
    }

    /// A list says only that it changed, with who, and its lines are never edited nor deleted.
    #[test]
    fn a_list_says_only_that_it_changed() {
        let (_d, c) = conn();
        c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS session_actor (user_id TEXT); INSERT INTO session_actor VALUES ('usr_1');").unwrap();
        record_list_change(&c, "institution.board_member", &Source::user()).unwrap();
        let line: (String, Option<String>, i64, Option<String>) = c
            .query_row("SELECT field, value_json, protected, changed_by FROM core_change", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap();
        assert_eq!(line, ("institution.board_member".into(), None, 1, Some("usr_1".into())));
        assert!(c.execute("UPDATE core_change SET value_json = '99'", []).is_err(), "a line is never edited");
        assert!(c.execute("DELETE FROM core_change", []).is_err(), "nor deleted");
    }
}
