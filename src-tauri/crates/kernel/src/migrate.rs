//! The migrations of each owner of tables (K4): every module, and the AI platform (ADR-003 A3). Each owner numbers
//! its own from 1, so two owners never clash, and its tables start with its id and `_`; the kernel keeps which
//! version of each one the database has. The migrations of the app from before the modules stay in its own table,
//! `schema_migrations`, as its history.

use rusqlite::Connection;

use crate::module::Migration;
use crate::registry::Modules;

/// Where the kernel keeps the migrations it applied: (owner, version).
pub const MIGRATIONS_TABLE: &str = "kernel_migrations";

/// The kernel's own tables (`kernel_`): it is an owner like the others.
pub(crate) const KERNEL_MIGRATIONS: &[Migration] = &[
    Migration { version: 1, sql: include_str!("../migrations/0001_audit.sql") },
    Migration { version: 2, sql: include_str!("../migrations/0002_change.sql") },
];

/// Applies the migrations the database does not have yet: the kernel's first, then module by module, each after the
/// ones it depends on. Every registered module is migrated, on or off: turning a module on does not change the
/// database.
pub fn migrate(conn: &mut Connection, modules: &Modules) -> rusqlite::Result<()> {
    migrate_owner(conn, "kernel", KERNEL_MIGRATIONS)?;
    for m in modules.all() {
        migrate_owner(conn, m.id, m.migrations)?;
    }
    Ok(())
}

/// The pending migrations of one owner (a module, or the platform with `ai`), each in its own transaction with the
/// line that says it was applied. They go by their number; the conformance test checks they are 1, 2, 3…
pub fn migrate_owner(conn: &mut Connection, owner: &str, migrations: &[Migration]) -> rusqlite::Result<()> {
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS {MIGRATIONS_TABLE} (
            owner TEXT NOT NULL,
            version INTEGER NOT NULL,
            applied_at TEXT NOT NULL,
            PRIMARY KEY (owner, version)
        );"
    ))?;
    for mig in migrations {
        let applied: bool = conn.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM {MIGRATIONS_TABLE} WHERE owner = ?1 AND version = ?2)"),
            rusqlite::params![owner, mig.version],
            |r| r.get(0),
        )?;
        if applied {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(mig.sql)?;
        tx.execute(
            &format!("INSERT INTO {MIGRATIONS_TABLE} (owner, version, applied_at) VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%SZ','now'))"),
            rusqlite::params![owner, mig.version],
        )?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Manifest;

    const BASE: Manifest = Manifest {
        id: "notas",
        name: "Notas",
        version: "0.1.0",
        depends_on: &[],
        permissions: &[],
        migrations: &[
            Migration { version: 1, sql: "CREATE TABLE notas_note (id TEXT PRIMARY KEY);" },
            Migration { version: 2, sql: "ALTER TABLE notas_note ADD COLUMN title TEXT;" },
        ],
        entities: &[],
        on: true,
    };
    static NOTES: Manifest = BASE;
    // the same numbers as the other module: each one numbers its own
    static TASKS: Manifest = Manifest {
        id: "tareas",
        migrations: &[
            Migration { version: 1, sql: "CREATE TABLE tareas_task (id TEXT PRIMARY KEY);" },
            Migration { version: 2, sql: "ALTER TABLE tareas_task ADD COLUMN due TEXT;" },
        ],
        on: false,
        ..BASE
    };

    fn applied(conn: &Connection) -> Vec<(String, i64)> {
        conn.prepare(&format!("SELECT owner, version FROM {MIGRATIONS_TABLE} ORDER BY owner, version"))
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn each_module_numbers_its_own_migrations_and_they_apply_once() {
        let mut conn = Connection::open_in_memory().unwrap();
        let modules = Modules::of([&NOTES, &TASKS]).unwrap();
        migrate(&mut conn, &modules).unwrap();
        migrate(&mut conn, &modules).unwrap();
        assert_eq!(
            applied(&conn),
            [("kernel".into(), 1), ("kernel".into(), 2), ("notas".into(), 1), ("notas".into(), 2), ("tareas".into(), 1), ("tareas".into(), 2)]
        );
        conn.execute("INSERT INTO notas_note (id, title) VALUES ('n', 'Hola')", []).unwrap();
        conn.execute("INSERT INTO tareas_task (id, due) VALUES ('t', '2026-10-21')", []).unwrap();
    }

    /// The platform is one more owner (ADR-003 A3): its own numbers, next to the modules'.
    #[test]
    fn an_owner_that_is_not_a_module_migrates_the_same_way() {
        const PLATFORM: &[Migration] = &[Migration { version: 1, sql: "CREATE TABLE ai_task (id TEXT PRIMARY KEY);" }];
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, &Modules::of([&NOTES]).unwrap()).unwrap();
        migrate_owner(&mut conn, "ai", PLATFORM).unwrap();
        migrate_owner(&mut conn, "ai", PLATFORM).unwrap();
        assert_eq!(applied(&conn), [("ai".into(), 1), ("kernel".into(), 1), ("kernel".into(), 2), ("notas".into(), 1), ("notas".into(), 2)]);
    }

    #[test]
    fn a_migration_that_fails_leaves_nothing_of_itself() {
        static BROKEN: Manifest = Manifest {
            migrations: &[
                Migration { version: 1, sql: "CREATE TABLE notas_note (id TEXT PRIMARY KEY);" },
                Migration { version: 2, sql: "CREATE TABLE notas_tag (id TEXT); ALTER TABLE notas_nothing ADD COLUMN x TEXT;" },
            ],
            ..BASE
        };
        let mut conn = Connection::open_in_memory().unwrap();
        assert!(migrate(&mut conn, &Modules::of([&BROKEN]).unwrap()).is_err());
        assert_eq!(applied(&conn), [("kernel".into(), 1), ("kernel".into(), 2), ("notas".into(), 1)]);
        let tag: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE name = 'notas_tag'", [], |r| r.get(0)).unwrap();
        assert_eq!(tag, 0);
    }
}
