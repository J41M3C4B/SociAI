//! Opening the SQLCipher database.

use super::{migrations, StorageError};
use rusqlite::Connection;
use std::path::Path;

/// Registers `sqlite-vec` for every connection opened afterwards (idempotent).
fn register_vec_extension() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut std::os::raw::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> std::os::raw::c_int,
        >(sqlite_vec::sqlite3_vec_init as *const ())));
    });
}

/// Opens (or creates) the encrypted database and applies pending migrations.
/// `key_hex` is the 256-bit key as 64 hex characters.
pub fn open_encrypted(path: &Path, key_hex: &str) -> Result<Connection, StorageError> {
    register_vec_extension();
    let mut conn = Connection::open(path)?;
    conn.execute_batch(&format!("PRAGMA key = \"x'{key_hex}'\";"))?;
    // Reading the schema fails if the key is wrong.
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0))
        .map_err(|_| StorageError::WrongKey)?;
    // secure_delete zeroes freed pages so deleted text does not linger in the file
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA secure_delete = ON;")?;
    migrations::run(&mut conn)?;
    // then the modules of the ERP, each with its own migrations (ERP-8)
    let modules = kernel::Modules::discover()
        .map_err(|errors| StorageError::Modules(errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")))?;
    kernel::migrate(&mut conn, &modules)?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    const OTHER_KEY: &str = "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100";

    #[test]
    fn creates_schema_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        drop(open_encrypted(&path, KEY).unwrap());
        let conn = open_encrypted(&path, KEY).unwrap();
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name IN ('institution','project','audit_log','document_chunk_fts')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 4);
        let versions: i64 = conn
            .query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(versions, 34);
    }

    /// A database from before the modules (ERP-8) keeps all it had: the migrations of the app stay as its history,
    /// and it gains the kernel's tables and the tables of each module.
    #[test]
    fn a_database_from_before_the_modules_keeps_its_data_and_gains_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let conn = open_encrypted(&path, KEY).unwrap();
        conn.execute_batch(
            "DROP TABLE kernel_migrations; DROP TABLE kernel_audit; DROP TABLE kernel_outbox; DROP TABLE kernel_history;
             DROP TABLE kernel_change; DROP TABLE muestra_note; DROP TABLE mi_institucion_profile;
             INSERT INTO institution (id,name,kind,created_at,updated_at) VALUES ('i','Asilo','elderly_home','t','t');",
        )
        .unwrap();
        drop(conn);
        let conn = open_encrypted(&path, KEY).unwrap();
        let name: String = conn.query_row("SELECT name FROM institution WHERE id = 'i'", [], |r| r.get(0)).unwrap();
        assert_eq!(name, "Asilo");
        let history: i64 = conn.query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0)).unwrap();
        assert_eq!(history, 34);
        let owners: Vec<(String, i64)> = conn
            .prepare("SELECT owner, version FROM kernel_migrations ORDER BY owner, version")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(owners, [("kernel".into(), 1), ("kernel".into(), 2), ("mi_institucion".into(), 1), ("muestra".into(), 1), ("muestra".into(), 2)]);
        conn.execute("INSERT INTO muestra_note (id, title, created_at) VALUES ('n', 'Nota', 't')", []).unwrap();
    }

    #[test]
    fn wrong_key_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        drop(open_encrypted(&path, KEY).unwrap());
        assert!(matches!(
            open_encrypted(&path, OTHER_KEY),
            Err(StorageError::WrongKey)
        ));
    }

    #[test]
    fn file_is_not_readable_without_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        drop(open_encrypted(&path, KEY).unwrap());
        let plain = Connection::open(&path).unwrap();
        assert!(plain
            .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0))
            .is_err());
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.starts_with(b"SQLite format 3"));
    }

    #[test]
    fn sqlite_vec_works_on_encrypted_db() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let conn = open_encrypted(&path, KEY).unwrap();
            let v: String = conn.query_row("SELECT vec_version()", [], |r| r.get(0)).unwrap();
            assert!(v.starts_with('v'));
            conn.execute_batch(
                "CREATE VIRTUAL TABLE chunk_vec USING vec0(embedding float[3]);
                 INSERT INTO chunk_vec(rowid, embedding) VALUES (1, '[1,0,0]'), (2, '[0,1,0]'), (3, '[0.9,0.1,0]');",
            )
            .unwrap();
        }
        // reopen: vector data persisted inside the encrypted file
        let conn = open_encrypted(&path, KEY).unwrap();
        let mut stmt = conn
            .prepare("SELECT rowid FROM chunk_vec WHERE embedding MATCH '[1,0,0]' AND k = 2 ORDER BY distance")
            .unwrap();
        let ids: Vec<i64> = stmt.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(ids, vec![1, 3]);
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.windows(9).any(|w| w == b"chunk_vec"));
    }

    #[test]
    fn fts5_works() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_encrypted(&dir.path().join("t.db"), KEY).unwrap();
        conn.execute_batch(
            "INSERT INTO document (id,kind,display_name,mime,data_level,clean_hash,created_at)
               VALUES ('doc_1','call','c','text/plain','green','h','2026-01-01T00:00:00Z');
             INSERT INTO document_chunk (id,document_id,ordinal,text) VALUES ('c1','doc_1',0,'monto maximo por proyecto');
             INSERT INTO document_chunk_fts(rowid,text) SELECT rowid,text FROM document_chunk;",
        )
        .unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM document_chunk_fts WHERE document_chunk_fts MATCH 'monto'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }
}
