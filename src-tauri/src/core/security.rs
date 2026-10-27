//! Security of the app on the computer (ADR-019): the optional PIN and the scan of the whole database.
//!
//! The PIN is a lock for the screen against someone who sits at the computer; it is not what protects the data (the
//! database is encrypted with a key in the keychain of the system). It is kept as a salted hash, never as the PIN.
//! The scan only counts: it never returns a piece of what it finds.

use crate::scanner::{PublicDocScanner, RegexScanner, SensitiveScanner};
use crate::core::error::ServiceError;
use crate::storage::{backup, open_encrypted, SharedDb};
use crate::core::profile::storage as profile_store;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const PIN_KEY: &str = "security.pin";
pub const MIN_PIN_DIGITS: usize = 4;
pub const MAX_PIN_DIGITS: usize = 8;
#[cfg_attr(not(test), allow(dead_code))]
const ITERATIONS: u32 = 120_000;
/// Wrong tries in a row after which the lock waits.
const MAX_TRIES: u32 = 5;
const WAIT: Duration = Duration::from_secs(30);

#[derive(Serialize, Deserialize)]
struct Stored {
    salt: String,
    hash: String,
    iterations: u32,
}

fn digest(salt: &[u8], pin: &str, iterations: u32) -> Vec<u8> {
    let mut h = Sha256::digest([salt, pin.as_bytes()].concat()).to_vec();
    for _ in 0..iterations {
        h = Sha256::digest([h.as_slice(), salt, pin.as_bytes()].concat()).to_vec();
    }
    h
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn valid_pin(pin: &str) -> bool {
    (MIN_PIN_DIGITS..=MAX_PIN_DIGITS).contains(&pin.chars().count()) && pin.chars().all(|c| c.is_ascii_digit())
}

fn stored(conn: &Connection) -> Result<Option<Stored>, ServiceError> {
    let raw: Option<String> = conn.query_row("SELECT value FROM app_settings WHERE key=?1", [PIN_KEY], |r| r.get(0)).optional()?;
    Ok(raw.and_then(|r| serde_json::from_str(&r).ok()))
}

pub fn pin_enabled(conn: &Connection) -> Result<bool, ServiceError> {
    Ok(stored(conn)?.is_some())
}

/// Sets or changes the PIN. Changing it needs the current one (`current`), so someone at the screen cannot swap it.
// the screen PIN retired with the accounts (ADR-028): an old one is only read to authorize the first account
#[cfg_attr(not(test), allow(dead_code))]
pub fn pin_set(conn: &Connection, attempts: &Attempts, new_pin: &str, current: Option<&str>) -> Result<(), ServiceError> {
    if !valid_pin(new_pin) {
        return Err(ServiceError::InvalidPin);
    }
    if pin_enabled(conn)? && !matches!(verify(conn, attempts, current.unwrap_or(""), Instant::now())?, Verify::Ok) {
        return Err(ServiceError::WrongPin);
    }
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| ServiceError::Internal(e.to_string()))?;
    let value = serde_json::to_string(&Stored { salt: hex::encode(salt), hash: hex::encode(digest(&salt, new_pin, ITERATIONS)), iterations: ITERATIONS }).unwrap_or_default();
    conn.execute("INSERT INTO app_settings (key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![PIN_KEY, value])?;
    Ok(())
}

/// Takes the PIN away. It needs the current PIN.
// the screen PIN retired with the accounts (ADR-028): an old one is only read to authorize the first account
#[cfg_attr(not(test), allow(dead_code))]
pub fn pin_clear(conn: &Connection, attempts: &Attempts, current: &str) -> Result<(), ServiceError> {
    if pin_enabled(conn)? && !matches!(verify(conn, attempts, current, Instant::now())?, Verify::Ok) {
        return Err(ServiceError::WrongPin);
    }
    conn.execute("DELETE FROM app_settings WHERE key=?1", [PIN_KEY])?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Verify {
    Ok,
    Wrong,
    /// Too many wrong tries in a row: wait before trying again.
    Locked { wait_secs: u64 },
}

/// Wrong tries in a row, kept in memory: closing the program is a way to try again, as it is with any screen lock.
#[derive(Default)]
pub struct Attempts(Mutex<(u32, Option<Instant>)>);

pub fn verify(conn: &Connection, attempts: &Attempts, pin: &str, now: Instant) -> Result<Verify, ServiceError> {
    let Some(s) = stored(conn)? else { return Ok(Verify::Ok) };
    let mut guard = attempts.0.lock().map_err(|_| ServiceError::Internal("lock".into()))?;
    if let Some(until) = guard.1 {
        if now < until {
            return Ok(Verify::Locked { wait_secs: (until - now).as_secs() + 1 });
        }
        *guard = (0, None);
    }
    let salt = hex::decode(&s.salt).unwrap_or_default();
    let expected = hex::decode(&s.hash).unwrap_or_default();
    if valid_pin(pin) && same(&digest(&salt, pin, s.iterations), &expected) {
        *guard = (0, None);
        return Ok(Verify::Ok);
    }
    guard.0 += 1;
    if guard.0 >= MAX_TRIES {
        guard.1 = Some(now + WAIT);
        guard.0 = 0;
        return Ok(Verify::Locked { wait_secs: WAIT.as_secs() });
    }
    Ok(Verify::Wrong)
}

// ------------------------------------------------------------------ the scan of the whole database

/// Where text is kept: table, the columns that hold words, and how the rows are found.
const SCANNED: &[(&str, &[&str])] = &[
    ("document", &["display_name", "extracted_text"]),
    ("document_chunk", &["text"]),
    ("conversation_turn", &["text"]),
    ("conversation_root", &["text"]),
    ("project_section", &["content"]),
    ("diagnosis_summary", &["summary_json"]),
    ("diagnosis_answer", &["answer"]),
    ("need", &["title", "description"]),
    ("budget_item", &["category", "description", "unit"]),
    ("schedule_activity", &["title"]),
    ("project", &["title", "initial_request"]),
    ("call_reading", &["name", "funder"]),
    ("institution", &["mission", "purpose", "services", "admission_criteria"]),
    ("core_change", &["value_json"]),
    ("institution_profile", &["notes"]),
    // the profile in the module Mi institución, and the history of every module in the kernel (ERP-1)
    ("mi_institucion_profile", &["mission", "purpose", "services", "admission_criteria", "notes"]),
    ("kernel_history", &["before", "after"]),
    ("income_source", &["label"]),
    ("expense_item", &["label"]),
    ("fin_income", &["label"]),
    ("fin_expense", &["label"]),
    ("hr_position", &["title", "duties"]),
];

#[derive(Debug, Clone, Serialize)]
pub struct ScanTable {
    pub table: &'static str,
    pub texts: usize,
    pub findings: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanSummary {
    pub tables: Vec<ScanTable>,
    pub texts: usize,
    pub findings: usize,
}

/// Looks for data that identifies a person (CURP, personal RFC, voter key, bank accounts, cards, social security)
/// in every text the app keeps. The result only says how many and where (by table), never what.
pub fn scan_database(conn: &Connection) -> Result<ScanSummary, ServiceError> {
    let scanner = PublicDocScanner::new(RegexScanner::new(profile_store::scanner_config(conn)?));
    let mut tables = Vec::new();
    for (table, columns) in SCANNED {
        let existing: Vec<&&str> = columns.iter().filter(|c| conn.prepare(&format!("SELECT {c} FROM {table} LIMIT 0")).is_ok()).collect();
        let (mut texts, mut findings) = (0, 0);
        for column in existing {
            let mut stmt = conn.prepare(&format!("SELECT {column} FROM {table} WHERE {column} IS NOT NULL"))?;
            let mut rows = stmt.query([])?;
            while let Some(r) = rows.next()? {
                let text: String = r.get(0)?;
                texts += 1;
                findings += scanner.scan(&text).findings.len();
            }
        }
        tables.push(ScanTable { table, texts, findings });
    }
    Ok(ScanSummary { texts: tables.iter().map(|t| t.texts).sum(), findings: tables.iter().map(|t| t.findings).sum(), tables })
}

// ------------------------------------------------------------------ backup and restore

/// The first name that does not exist yet: «Respaldo.sociai», «Respaldo (2).sociai»…
fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    p
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupFile {
    pub file_name: String,
    pub path: String,
}

/// Writes an encrypted copy of everything in `dir`, protected by the password. The log keeps only the fact.
pub fn create_backup(db: &SharedDb, dir: &Path, password: &str) -> Result<BackupFile, ServiceError> {
    let conn = db.lock().map_err(|_| ServiceError::Internal("lock".into()))?;
    let today: String = conn.query_row("SELECT date('now')", [], |r| r.get(0))?;
    std::fs::create_dir_all(dir).map_err(|e| ServiceError::Internal(e.to_string()))?;
    let path = unique_path(dir, &format!("Respaldo SociAI {today}"), "sociai");
    backup::create(&conn, &path, password)?;
    crate::audit::record(&conn, crate::audit::AuditKind::BackupCreated, None, None, serde_json::json!({ "format": "sociai" }))?;
    Ok(BackupFile { file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), path: path.to_string_lossy().into_owned() })
}

/// Replaces everything with what a backup holds. The password is checked first; the current file is kept next to it
/// (never deleted) so nothing is lost if the person changes their mind or picked the wrong backup.
pub fn restore_backup(db: &SharedDb, db_path: &Path, key_hex: &str, bytes: &[u8], password: &str) -> Result<PathBuf, ServiceError> {
    let dir = db_path.parent().ok_or_else(|| ServiceError::Internal("no folder".into()))?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let incoming = dir.join(format!("respaldo-{stamp}.tmp"));
    let restored = dir.join(format!("restaurada-{stamp}.tmp"));
    std::fs::write(&incoming, bytes).map_err(|e| ServiceError::Internal(e.to_string()))?;
    let made = backup::restore_to(&incoming, password, &restored, key_hex);
    let _ = std::fs::remove_file(&incoming);
    made?;

    let mut guard = db.lock().map_err(|_| ServiceError::Internal("lock".into()))?;
    let name = db_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let kept = dir.join(format!("{name}.antes-de-restaurar-{stamp}"));
    // the working file cannot be moved while it is open: close it by putting an empty database in its place
    let placeholder = Connection::open_in_memory()?;
    drop(std::mem::replace(&mut *guard, placeholder));
    let swapped = std::fs::rename(db_path, &kept).and_then(|_| std::fs::rename(&restored, db_path).inspect_err(|_| {
        let _ = std::fs::rename(&kept, db_path); // put the original back
    }));
    // a backup of before the modules hands its profile to Mi institución, as on any opening (ERP-1)
    let reopened = open_encrypted(db_path, key_hex).map_err(ServiceError::from).and_then(|mut conn| {
        crate::core::profile::adopt::adopt(&mut conn)?;
        Ok(conn)
    });
    match (swapped, reopened) {
        (Ok(()), Ok(conn)) => {
            *guard = conn;
            Ok(kept)
        }
        (swapped, reopened) => {
            let _ = std::fs::remove_file(&restored);
            if let Ok(conn) = reopened {
                *guard = conn;
            }
            Err(ServiceError::Internal(swapped.err().map(|e| e.to_string()).unwrap_or_else(|| "could not reopen the database".into())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::open_encrypted;
    use std::sync::Arc;

    const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn conn() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let c = open_encrypted(&dir.path().join("t.db"), KEY).unwrap();
        (dir, c)
    }

    #[test]
    fn without_a_pin_nothing_is_asked_and_a_pin_is_four_to_eight_digits() {
        let (_d, c) = conn();
        let a = Attempts::default();
        assert!(!pin_enabled(&c).unwrap());
        assert_eq!(verify(&c, &a, "", Instant::now()).unwrap(), Verify::Ok);
        for bad in ["", "123", "123456789", "12a4", "12 34", "１２３４"] {
            assert!(matches!(pin_set(&c, &a, bad, None), Err(ServiceError::InvalidPin)), "{bad}");
        }
        pin_set(&c, &a, "4821", None).unwrap();
        assert!(pin_enabled(&c).unwrap());
    }

    #[test]
    fn the_pin_is_kept_as_a_salted_hash_and_only_the_right_one_opens() {
        let (_d, c) = conn();
        let a = Attempts::default();
        pin_set(&c, &a, "4821", None).unwrap();
        let raw: String = c.query_row("SELECT value FROM app_settings WHERE key='security.pin'", [], |r| r.get(0)).unwrap();
        assert!(!raw.contains("4821"), "{raw}");
        let now = Instant::now();
        assert_eq!(verify(&c, &a, "4821", now).unwrap(), Verify::Ok);
        assert_eq!(verify(&c, &a, "4822", now).unwrap(), Verify::Wrong);
        assert_eq!(verify(&c, &a, "", now).unwrap(), Verify::Wrong);
        // the same PIN gets another hash each time it is set
        pin_set(&c, &a, "4821", Some("4821")).unwrap();
        let again: String = c.query_row("SELECT value FROM app_settings WHERE key='security.pin'", [], |r| r.get(0)).unwrap();
        assert_ne!(raw, again);
        assert_eq!(verify(&c, &a, "4821", now).unwrap(), Verify::Ok);
    }

    #[test]
    fn changing_or_clearing_the_pin_needs_the_current_one() {
        let (_d, c) = conn();
        let a = Attempts::default();
        pin_set(&c, &a, "4821", None).unwrap();
        assert!(matches!(pin_set(&c, &a, "9999", None), Err(ServiceError::WrongPin)));
        assert!(matches!(pin_set(&c, &a, "9999", Some("0000")), Err(ServiceError::WrongPin)));
        assert!(matches!(pin_clear(&c, &a, "0000"), Err(ServiceError::WrongPin)));
        assert!(pin_enabled(&c).unwrap());
        pin_set(&c, &a, "9999", Some("4821")).unwrap();
        assert_eq!(verify(&c, &a, "9999", Instant::now()).unwrap(), Verify::Ok);
        pin_clear(&c, &a, "9999").unwrap();
        assert!(!pin_enabled(&c).unwrap());
    }

    #[test]
    fn five_wrong_tries_in_a_row_make_the_lock_wait_and_a_right_one_starts_over() {
        let (_d, c) = conn();
        let a = Attempts::default();
        pin_set(&c, &a, "4821", None).unwrap();
        let t0 = Instant::now();
        for _ in 0..4 {
            assert_eq!(verify(&c, &a, "0000", t0).unwrap(), Verify::Wrong);
        }
        assert_eq!(verify(&c, &a, "4821", t0).unwrap(), Verify::Ok, "a right one before the limit starts over");
        for _ in 0..4 {
            assert_eq!(verify(&c, &a, "0000", t0).unwrap(), Verify::Wrong);
        }
        assert!(matches!(verify(&c, &a, "0000", t0).unwrap(), Verify::Locked { wait_secs: 30 }));
        // even the right PIN waits
        assert!(matches!(verify(&c, &a, "4821", t0 + Duration::from_secs(10)).unwrap(), Verify::Locked { wait_secs } if wait_secs <= 21));
        assert_eq!(verify(&c, &a, "4821", t0 + Duration::from_secs(31)).unwrap(), Verify::Ok);
    }

    #[test]
    fn the_scan_counts_what_identifies_a_person_by_table_and_never_returns_it() {
        let (_d, c) = conn();
        let clean = scan_database(&c).unwrap();
        assert_eq!(clean.findings, 0);
        assert!(clean.tables.iter().any(|t| t.table == "project_section"));

        use crate::core::profile::domain::*;
        let mut c = c;
        let input = ProfileInput { institution: InstitutionInput { name: "Asilo Ficticio".into(), ..Default::default() }, ..Default::default() };
        crate::core::profile::storage::save_finished(&mut c, &input);
        let p_id = {
            c.execute(
                "INSERT INTO project (id,institution_id,profile_id,title,stage,kind,created_at,updated_at)
                 SELECT 'proj_test', institution_id, id, 'Proyecto', 'PROFILE', 'internal', 't', 't' FROM institution_profile ORDER BY version DESC LIMIT 1",
                [],
            )
            .unwrap();
            "proj_test".to_string()
        };
        // data of a person written straight into the database, as a bug or an old version might have left it
        c.execute(
            "INSERT INTO project_section (id,project_id,section_key,content,needs_review,updated_at,origin) VALUES ('s',?1,'what','La señora con CURP LOPM800101MDFRZN09 pidió ayuda.',0,'t','user')",
            [&p_id],
        )
        .unwrap();
        // the contact of a funder is public and is not a finding
        c.execute(
            "INSERT INTO project_section (id,project_id,section_key,content,needs_review,updated_at,origin) VALUES ('s2',?1,'why','Dudas: fundacion@ejemplo.org o 55 1234 5678.',0,'t','user')",
            [&p_id],
        )
        .unwrap();
        let dirty = scan_database(&c).unwrap();
        assert_eq!(dirty.findings, 1);
        let t = dirty.tables.iter().find(|t| t.table == "project_section").unwrap();
        assert_eq!((t.texts, t.findings), (2, 1));
        assert!(!serde_json::to_string(&dirty).unwrap().contains("LOPM8"), "only counts go out");
    }

    fn working_database(dir: &Path, key: &str, title: &str) -> (PathBuf, SharedDb) {
        use crate::core::profile::domain::*;
        let path = dir.join("sociai.db");
        let mut c = open_encrypted(&path, key).unwrap();
        let input = ProfileInput { institution: InstitutionInput { name: "Asilo Ficticio".into(), ..Default::default() }, ..Default::default() };
        crate::core::profile::storage::save_finished(&mut c, &input);
        c.execute(
            "INSERT INTO project (id,institution_id,profile_id,title,stage,kind,created_at,updated_at)
             SELECT 'proj_test', institution_id, id, ?1, 'PROFILE', 'internal', 't', 't' FROM institution_profile ORDER BY version DESC LIMIT 1",
            [title],
        )
        .unwrap();
        (path, Arc::new(Mutex::new(c)))
    }

    #[test]
    fn a_backup_made_here_replaces_everything_on_another_computer_and_the_old_file_is_kept() {
        let here = tempfile::tempdir().unwrap();
        let (_p, db) = working_database(here.path(), KEY, "Proyecto del respaldo");
        let out = create_backup(&db, here.path(), "una contraseña larga").unwrap();
        assert!(out.file_name.starts_with("Respaldo SociAI ") && out.file_name.ends_with(".sociai"));
        let again = create_backup(&db, here.path(), "una contraseña larga").unwrap();
        assert_ne!(out.path, again.path, "a backup never replaces another");
        let log: i64 = db.lock().unwrap().query_row("SELECT count(*) FROM audit_log WHERE event='backup.created'", [], |r| r.get(0)).unwrap();
        assert_eq!(log, 2);
        let bytes = std::fs::read(&out.path).unwrap();

        // another computer, with its own key and its own work
        const OTHER: &str = "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100";
        let there = tempfile::tempdir().unwrap();
        let (path, db2) = working_database(there.path(), OTHER, "Proyecto de la otra computadora");
        // wrong password: nothing changes
        assert!(matches!(restore_backup(&db2, &path, OTHER, &bytes, "otra contraseña"), Err(ServiceError::Storage(crate::storage::StorageError::WrongPassword))));
        let title: String = db2.lock().unwrap().query_row("SELECT title FROM project", [], |r| r.get(0)).unwrap();
        assert_eq!(title, "Proyecto de la otra computadora");
        assert_eq!(std::fs::read_dir(there.path()).unwrap().count(), 1, "no leftovers");

        let kept = restore_backup(&db2, &path, OTHER, &bytes, "una contraseña larga").unwrap();
        // the live connection now holds what the backup held, under this computer's key
        let title: String = db2.lock().unwrap().query_row("SELECT title FROM project", [], |r| r.get(0)).unwrap();
        assert_eq!(title, "Proyecto del respaldo");
        drop(open_encrypted(&path, OTHER).unwrap());
        // and what was there before is still there, untouched
        let old = open_encrypted(&kept, OTHER).unwrap();
        let before: String = old.query_row("SELECT title FROM project", [], |r| r.get(0)).unwrap();
        assert_eq!(before, "Proyecto de la otra computadora");
        let names: Vec<_> = std::fs::read_dir(there.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
    }

    #[test]
    fn a_file_that_is_not_a_backup_changes_nothing() {
        let there = tempfile::tempdir().unwrap();
        let (path, db) = working_database(there.path(), KEY, "Mi proyecto");
        assert!(matches!(restore_backup(&db, &path, KEY, b"esto no es un respaldo, solo texto suelto de prueba", "una contraseña larga"), Err(ServiceError::Storage(crate::storage::StorageError::WrongPassword))));
        let title: String = db.lock().unwrap().query_row("SELECT title FROM project", [], |r| r.get(0)).unwrap();
        assert_eq!(title, "Mi proyecto");
    }
}
