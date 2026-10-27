//! Encrypted backup and restore (ADR-019). A backup is a copy of the whole database encrypted with a password the
//! person chooses (SQLCipher derives the key from it), so it can be restored on another computer; the key of the
//! working database stays in the keychain of this one. Nothing is ever deleted: a restore keeps the previous file.

use super::{db::open_encrypted, StorageError};
use rusqlite::Connection;
use std::path::Path;

/// The shortest password a backup accepts.
pub const MIN_PASSWORD_CHARS: usize = 8;

fn quote(s: &str) -> String {
    s.replace('\'', "''")
}

fn path_text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Writes a copy of the database to `dest`, encrypted with `password`. `dest` must not exist.
pub fn create(conn: &Connection, dest: &Path, password: &str) -> Result<(), StorageError> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(StorageError::WeakPassword);
    }
    if dest.exists() {
        return Err(StorageError::BackupExists);
    }
    let result = (|| -> rusqlite::Result<()> {
        conn.execute_batch(&format!("ATTACH DATABASE '{}' AS backup KEY '{}';", quote(&path_text(dest)), quote(password)))?;
        let exported = conn.query_row("SELECT sqlcipher_export('backup')", [], |_| Ok(()));
        // detach even if the export failed, so the working connection is left as it was
        let detached = conn.execute_batch("DETACH DATABASE backup;");
        exported.and(detached)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    Ok(result?)
}

/// Opens a backup with its password. A wrong password (or a file that is not a backup) is `WrongPassword`.
fn open_backup(path: &Path, password: &str) -> Result<Connection, StorageError> {
    let conn = Connection::open(path)?;
    conn.execute_batch(&format!("PRAGMA key = '{}';", quote(password)))?;
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0)).map_err(|_| StorageError::WrongPassword)?;
    Ok(conn)
}

/// Reads the backup and writes its content to `target`, encrypted with the working key and brought up to date
/// with the migrations of this version. `target` must not exist.
pub fn restore_to(backup: &Path, password: &str, target: &Path, key_hex: &str) -> Result<(), StorageError> {
    if target.exists() {
        return Err(StorageError::BackupExists);
    }
    let result = (|| -> Result<(), StorageError> {
        let conn = open_backup(backup, password)?;
        conn.execute_batch(&format!("ATTACH DATABASE '{}' AS restored KEY \"x'{key_hex}'\";", quote(&path_text(target))))?;
        conn.query_row("SELECT sqlcipher_export('restored')", [], |_| Ok(()))?;
        conn.execute_batch("DETACH DATABASE restored;")?;
        drop(conn);
        // opening it with the working key applies the migrations of this version and proves the key is right
        drop(open_encrypted(target, key_hex)?);
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(target);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::profile::domain::*;
    use crate::core::profile::storage as profile;

    const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    const OTHER_KEY: &str = "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100";

    fn database(dir: &Path) -> Connection {
        let mut c = open_encrypted(&dir.join("work.db"), KEY).unwrap();
        let input = ProfileInput { institution: InstitutionInput { name: "Asilo Ficticio".into(), ..Default::default() }, ..Default::default() };
        profile::save_finished(&mut c, &input);
        c.execute(
            "INSERT INTO project (id,institution_id,profile_id,title,initial_request,stage,kind,created_at,updated_at)
             SELECT 'proj_test', institution_id, id, 'Proyecto de prueba', 'Cambiar las tuberías de la cocina', 'PROFILE', 'internal', 't', 't'
             FROM institution_profile ORDER BY version DESC LIMIT 1",
            [],
        )
        .unwrap();
        // a page of a call, searchable like any other
        c.execute("INSERT INTO document (id,kind,display_name,mime,data_level,clean_hash,extracted_text,redactions_count,created_at) VALUES ('d','call','a.pdf','application/pdf','green','h','x',0,'t')", []).unwrap();
        crate::core::archive::storage::add_call_document(&c, "bases.pdf", "application/pdf", &["Convocatoria zanahoria 2027.".to_string()], 0, &Default::default()).unwrap();
        c
    }

    #[test]
    fn a_backup_restores_everything_even_with_another_working_key() {
        let dir = tempfile::tempdir().unwrap();
        let c = database(dir.path());
        let file = dir.path().join("respaldo.sociai");
        create(&c, &file, "una contraseña larga").unwrap();
        assert!(file.exists());
        // the working database is still fine after the backup
        assert_eq!(c.query_row("SELECT count(*) FROM project", [], |r| r.get::<_, i64>(0)).unwrap(), 1);

        // another computer: another working key
        let target = dir.path().join("restored.db");
        restore_to(&file, "una contraseña larga", &target, OTHER_KEY).unwrap();
        let r = open_encrypted(&target, OTHER_KEY).unwrap();
        let title: String = r.query_row("SELECT title FROM project", [], |r| r.get(0)).unwrap();
        assert_eq!(title, "Proyecto de prueba");
        let name: String = r.query_row("SELECT name FROM mi_institucion_profile", [], |r| r.get(0)).unwrap();
        assert_eq!(name, "Asilo Ficticio");
        // the search index came along
        let found: i64 = r.query_row("SELECT count(*) FROM document_chunk_fts WHERE document_chunk_fts MATCH 'zanahoria'", [], |r| r.get(0)).unwrap();
        assert_eq!(found, 1);
        let versions: i64 = r.query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0)).unwrap();
        assert!(versions >= 7);
        // the old key does not open the restored file
        assert!(matches!(open_encrypted(&target, KEY), Err(StorageError::WrongKey)));
    }

    #[test]
    fn the_backup_is_encrypted_with_the_password_and_nothing_else_opens_it() {
        let dir = tempfile::tempdir().unwrap();
        let c = database(dir.path());
        let file = dir.path().join("respaldo.sociai");
        create(&c, &file, "una contraseña larga").unwrap();
        let bytes = std::fs::read(&file).unwrap();
        assert!(!bytes.windows(15).any(|w| w == b"Asilo Ficticio\0") && !bytes.starts_with(b"SQLite format 3"), "the file is not readable as it is");
        assert!(!String::from_utf8_lossy(&bytes).contains("Asilo Ficticio"));
        let target = dir.path().join("r.db");
        assert!(matches!(restore_to(&file, "otra contraseña", &target, KEY), Err(StorageError::WrongPassword)));
        assert!(!target.exists(), "a failed restore leaves nothing behind");
        // the working key is not the password either
        assert!(matches!(restore_to(&file, KEY, &target, KEY), Err(StorageError::WrongPassword)));
        // a file that is not a backup
        let junk = dir.path().join("basura.sociai");
        std::fs::write(&junk, b"esto no es un respaldo, solo texto suelto de prueba").unwrap();
        assert!(matches!(restore_to(&junk, "una contraseña larga", &target, KEY), Err(StorageError::WrongPassword)));
    }

    #[test]
    fn a_short_password_is_refused_and_a_backup_never_overwrites_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let c = database(dir.path());
        let file = dir.path().join("respaldo.sociai");
        assert!(matches!(create(&c, &file, "corta"), Err(StorageError::WeakPassword)));
        assert!(!file.exists());
        create(&c, &file, "contraseña de 8+").unwrap();
        assert!(matches!(create(&c, &file, "contraseña de 8+"), Err(StorageError::BackupExists)));
        // a password with quotes works too
        let q = dir.path().join("q.sociai");
        create(&c, &q, "it's \"quoted\" ok").unwrap();
        restore_to(&q, "it's \"quoted\" ok", &dir.path().join("q.db"), KEY).unwrap();
    }
}
