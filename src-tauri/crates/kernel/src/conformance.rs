//! The part of the conformance test that needs a database (the ERP blueprint §4), for any owner of tables: a module,
//! or the AI platform (ADR-003 A3). Its migrations are numbered 1, 2, 3… and apply on a database with nothing,
//! everything they make carries its prefix, and none of its tables points at a table of another owner (the kernel's
//! may be pointed at). The rest of the rules of a module's manifest (its id, its dependencies, its permissions and
//! its part of the dictionary, where every field has a class) are the registry's. The capabilities join with ERP-1.

use std::fmt;

use rusqlite::Connection;

use crate::migrate::{migrate_owner, MIGRATIONS_TABLE};
use crate::module::Migration;

/// Something an owner of tables breaks of its shape on the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Breach {
    /// The database had something already: the test needs it empty.
    NotEmpty,
    /// Its migrations are not numbered 1, 2, 3… in order, or one has no SQL.
    OutOfOrder,
    /// A migration does not apply.
    MigrationFails { version: i64, error: String },
    /// A table, index, view or trigger whose name does not start with the owner's id and `_`.
    Unprefixed { kind: String, name: String },
    /// A foreign key towards a table that is neither the owner's nor the kernel's.
    ForeignKey { table: String, target: String },
}

impl fmt::Display for Breach {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Breach::NotEmpty => write!(f, "the database was not empty"),
            Breach::OutOfOrder => write!(f, "the migrations are not numbered 1, 2, 3… in order"),
            Breach::MigrationFails { version, error } => write!(f, "migration {version} fails: {error}"),
            Breach::Unprefixed { kind, name } => write!(f, "{kind} `{name}` does not carry the owner's prefix"),
            Breach::ForeignKey { table, target } => write!(f, "`{table}` points at `{target}`, which is not its own"),
        }
    }
}

/// What the owner breaks, migrating it on `conn`, which must be empty.
pub fn conform(owner: &str, migrations: &[Migration], conn: &mut Connection) -> Vec<Breach> {
    let objects = |conn: &Connection| -> rusqlite::Result<Vec<(String, String)>> {
        conn.prepare(&format!("SELECT type, name FROM sqlite_master WHERE name NOT LIKE 'sqlite\\_%' ESCAPE '\\' AND name <> '{MIGRATIONS_TABLE}' ORDER BY name"))?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect()
    };
    match objects(conn) {
        Ok(before) if before.is_empty() => {}
        _ => return vec![Breach::NotEmpty],
    }
    let mut out = Vec::new();
    if !migrations.iter().enumerate().all(|(i, mig)| mig.version == i as i64 + 1 && !mig.sql.trim().is_empty()) {
        out.push(Breach::OutOfOrder);
    }
    if let Err(e) = migrate_owner(conn, owner, migrations) {
        let done: i64 = conn
            .query_row(&format!("SELECT coalesce(max(version), 0) FROM {MIGRATIONS_TABLE} WHERE owner = ?1"), [owner], |r| r.get(0))
            .unwrap_or(0);
        out.push(Breach::MigrationFails { version: done + 1, error: e.to_string() });
    }
    let prefix = format!("{owner}_");
    let made = objects(conn).unwrap_or_default();
    for (kind, name) in &made {
        if !name.starts_with(&prefix) {
            out.push(Breach::Unprefixed { kind: kind.clone(), name: name.clone() });
        }
    }
    for (_, table) in made.iter().filter(|(kind, _)| kind == "table") {
        let targets: Vec<String> = conn
            .prepare("SELECT DISTINCT \"table\" FROM pragma_foreign_key_list(?1) ORDER BY 1")
            .and_then(|mut s| s.query_map([table], |r| r.get(0))?.collect())
            .unwrap_or_default();
        for target in targets.into_iter().filter(|t| !t.starts_with(&prefix) && !t.starts_with("kernel_")) {
            out.push(Breach::ForeignKey { table: table.clone(), target });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_module_that_keeps_to_its_tables_conforms() {
        const OK: &[Migration] = &[
            Migration { version: 1, sql: "CREATE TABLE notas_note (id TEXT PRIMARY KEY, title TEXT UNIQUE);" },
            Migration {
                version: 2,
                sql: "CREATE TABLE notas_tag (note_id TEXT NOT NULL REFERENCES notas_note(id), tag TEXT NOT NULL);
                      CREATE INDEX notas_tag_by_note ON notas_tag(note_id);
                      CREATE VIRTUAL TABLE notas_search USING fts5(title);",
            },
        ];
        let mut conn = Connection::open_in_memory().unwrap();
        assert_eq!(conform("notas", OK, &mut conn), []);
    }

    #[test]
    fn what_a_module_breaks_on_the_database_is_named() {
        const BAD: &[Migration] = &[
            Migration {
                version: 1,
                sql: "CREATE TABLE notas_note (id TEXT PRIMARY KEY, donor_id TEXT REFERENCES donantes_donor(id));
                      CREATE TABLE note (id TEXT);
                      CREATE INDEX by_donor ON notas_note(donor_id);",
            },
            Migration { version: 2, sql: "ALTER TABLE notas_nothing ADD COLUMN x TEXT;" },
        ];
        let mut conn = Connection::open_in_memory().unwrap();
        let breaches = conform("notas", BAD, &mut conn);
        assert!(matches!(&breaches[0], Breach::MigrationFails { version: 2, error } if error.contains("notas_nothing")), "{breaches:?}");
        assert_eq!(
            breaches[1..],
            [
                Breach::Unprefixed { kind: "index".into(), name: "by_donor".into() },
                Breach::Unprefixed { kind: "table".into(), name: "note".into() },
                Breach::ForeignKey { table: "notas_note".into(), target: "donantes_donor".into() },
            ]
        );
    }

    #[test]
    fn the_test_needs_an_empty_database() {
        const OK: &[Migration] = &[Migration { version: 1, sql: "CREATE TABLE notas_note (id TEXT);" }];
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE institution (id TEXT);").unwrap();
        assert_eq!(conform("notas", OK, &mut conn), [Breach::NotEmpty]);
    }

    /// The platform is checked like a module (ADR-003 A3), and its numbers too, which no registry checked before.
    #[test]
    fn the_platform_keeps_to_its_tables_too() {
        const PLATFORM: &[Migration] = &[
            Migration { version: 1, sql: "CREATE TABLE ai_task (id TEXT PRIMARY KEY);" },
            Migration { version: 3, sql: "CREATE TABLE ai_step (task_id TEXT REFERENCES ai_task(id), account TEXT REFERENCES app_user(id));" },
        ];
        let mut conn = Connection::open_in_memory().unwrap();
        assert_eq!(
            conform("ai", PLATFORM, &mut conn),
            [Breach::OutOfOrder, Breach::ForeignKey { table: "ai_step".into(), target: "app_user".into() }]
        );
    }
}
