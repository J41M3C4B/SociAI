//! The profile of the institution (ADR-031): scan -> save -> audit. Its data are the module Mi institución (ERP-1),
//! which validates them on the kernel and keeps their history. The scanner still reads the free texts before they are
//! saved, here in the app: the module does not know the AI exists (D2 of ERP-1).

use crate::audit::{self, AuditKind};
use crate::core::error::ServiceError;
use crate::core::profile::domain::{ProfileInput, ProfileIssue, ProfileTotals};
use crate::core::profile::storage::{self as store, StoredProfile};
use crate::core::screen::scanner_for;
use crate::scanner::guard::{counts_json, guard_fields, Decision, GuardOutcome, QuarantineReport};
use crate::storage::StorageError;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize)]
pub struct ProfileView {
    pub institution_id: String,
    pub version: i64,
    /// When the first start was finished.
    pub confirmed_at: Option<String>,
    /// The first start is not finished yet.
    pub is_draft: bool,
    pub input: ProfileInput,
    pub totals: ProfileTotals,
    /// Only heads-ups ("algo no cuadra"); blocking problems stop the save.
    pub issues: Vec<ProfileIssue>,
}

impl From<StoredProfile> for ProfileView {
    fn from(p: StoredProfile) -> Self {
        ProfileView {
            institution_id: p.institution_id,
            version: p.version,
            is_draft: p.confirmed_at.is_none(),
            confirmed_at: p.confirmed_at,
            totals: p.input.totals(p.as_of_year),
            issues: p.input.validate(p.as_of_year).into_iter().filter(|i| !i.blocking).collect(),
            input: p.input,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SaveProfileOutcome {
    Saved { profile: ProfileView },
    Quarantine { report: QuarantineReport },
    Invalid { issues: Vec<ProfileIssue> },
}

pub fn get_profile(conn: &Connection) -> Result<Option<ProfileView>, ServiceError> {
    Ok(store::load_current(conn)?.map(Into::into))
}

/// Who writes: the person of the session. Only the examples of `fixtures/` and the tests write with no person.
pub fn origin(conn: &Connection) -> Result<kernel::Origin, ServiceError> {
    conn.execute_batch("CREATE TEMP TABLE IF NOT EXISTS session_actor (user_id TEXT);")?;
    let actor: Option<String> = conn.query_row("SELECT user_id FROM temp.session_actor LIMIT 1", [], |r| r.get(0)).optional()?.flatten();
    Ok(match actor {
        Some(account) => kernel::Origin::Person { account },
        None => kernel::Origin::Calculated { rule: "fixtures".into() },
    })
}

/// The free texts the scanner reads before saving: the long texts that may reach the AI. The address, the folios and
/// the keys are the institution's own and are not scanned, like its contact; nor is the municipality, the name of a
/// place, though many are named after a person («Benito Juárez»).
fn scanned(field: &str) -> bool {
    mi_institucion::catalog::PROFILE
        .field(field)
        .is_some_and(|f| f.kind == kernel::Kind::LongText && kernel::KEPT_IN_HISTORY.contains(&f.class))
}

/// Reads the free texts of these values (by the module's ids) with the scanner, before they are saved: covers what the
/// person chose to cover, and leaves a line in the log, with counts only. `Some` when the person has to decide first.
pub fn scan(conn: &Connection, values: &mut Map<String, Value>, decision: Option<Decision>) -> Result<Option<QuarantineReport>, ServiceError> {
    let found: Vec<String> = values.iter().filter(|(k, v)| scanned(k) && v.is_string()).map(|(k, _)| k.clone()).collect();
    // each one by the id the screen gives it, so the person is told where it was found
    let fields: Vec<(String, String)> =
        found.iter().map(|k| (crate::core::institution::forms::screen_id(k), values[k].as_str().unwrap_or_default().to_string())).collect();
    // with the current institution: its own phone, mail and name are not flagged
    let scanner = scanner_for(conn)?;
    let event = match guard_fields(&scanner, &fields, decision)? {
        GuardOutcome::Clean => None,
        GuardOutcome::Quarantine(report) => return Ok(Some(report)),
        GuardOutcome::Redacted { texts, counts } => {
            for (field, text) in found.iter().zip(texts) {
                values.insert(field.clone(), Value::String(text));
            }
            Some((AuditKind::ScannerQuarantine, counts_json(&counts, "redacted")))
        }
        GuardOutcome::Overridden { counts } => Some((AuditKind::ScannerOverride, counts_json(&counts, "not_personal"))),
    };
    if let Some((kind, details)) = event {
        audit::record(conn, kind, Some("mi_institucion_profile"), None, details)?;
    }
    Ok(None)
}

/// The code of what does not pass, as the screen knows it. The name of the institution has its own words.
pub fn issue(field: &str, reason: &str) -> ProfileIssue {
    const KNOWN: &[&str] = &[
        "required", "wrong_type", "code_unknown", "repeated", "year_invalid", "negative_number", "number_too_small", "number_too_large",
        "date_invalid", "too_long", "not_an_email", "not_a_phone", "postal_code_invalid", "rfc_moral_invalid", "clabe_invalid",
        "place_name_invalid", "age_range",
    ];
    let code = match (field, reason) {
        ("name", "required") => "name_missing",
        (_, r) => KNOWN.iter().copied().find(|k| *k == r).unwrap_or("wrong_type"),
    };
    ProfileIssue { code, field: crate::core::institution::forms::screen_id(field), blocking: true }
}

/// What a save that did not pass says to the screen.
pub fn invalid(e: StorageError) -> Result<SaveProfileOutcome, ServiceError> {
    match e {
        StorageError::Invalid(found) => Ok(SaveProfileOutcome::Invalid { issues: found.iter().map(|i| issue(&i.field, &i.reason)).collect() }),
        other => Err(other.into()),
    }
}

/// Saves a whole profile, window by window (the examples of `fixtures/` load this way): its free texts through the
/// scanner first. The staff and the people served are never written here: the profile keeps what their modules add
/// up to (ADR-020).
pub fn save_profile(conn: &mut Connection, mut input: ProfileInput, decision: Option<Decision>) -> Result<SaveProfileOutcome, ServiceError> {
    let (staff, population) = crate::core::profile::sync::derive(conn)?;
    input.staff = staff;
    input.population = population;
    let mut values = store::values_of(&input);
    if let Some(report) = scan(conn, &mut values, decision)? {
        return Ok(SaveProfileOutcome::Quarantine { report });
    }
    let input = ProfileInput { institution: crate::core::profile::storage::input_of(&values).institution, ..input };
    let origin = origin(conn)?;
    match store::save(conn, &input, &origin) {
        Ok(saved) => Ok(SaveProfileOutcome::Saved { profile: saved.into() }),
        Err(e) => invalid(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::profile::domain::*;
    use crate::storage::open_encrypted;

    const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn conn() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let c = open_encrypted(&dir.path().join("t.db"), KEY).unwrap();
        (dir, c)
    }

    fn input() -> ProfileInput {
        ProfileInput {
            institution: InstitutionInput { name: "Casa Hogar Ficticia".into(), ..Default::default() },
            population: vec![PopulationGroupInput { label: "Adultos mayores".into(), count: 18, ..Default::default() }],
            ..Default::default()
        }
    }

    fn audit_events(c: &Connection) -> Vec<(String, String)> {
        let mut s = c.prepare("SELECT event, details_json FROM audit_log ORDER BY id").unwrap();
        let v = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|r| r.unwrap()).collect();
        v
    }

    #[test]
    fn clean_profile_is_saved() {
        let (_d, mut c) = conn();
        let out = save_profile(&mut c, input(), None).unwrap();
        assert!(matches!(out, SaveProfileOutcome::Saved { .. }));
        assert!(audit_events(&c).is_empty());
    }

    #[test]
    fn invalid_profile_is_not_saved() {
        let (_d, mut c) = conn();
        let mut p = input();
        p.institution.name.clear();
        let SaveProfileOutcome::Invalid { issues } = save_profile(&mut c, p, None).unwrap() else { panic!() };
        assert_eq!((issues[0].code, issues[0].field.as_str()), ("name_missing", "institution.name"));
        assert!(get_profile(&c).unwrap().is_none());
    }

    #[test]
    fn pasting_a_curp_triggers_quarantine_and_saves_nothing() {
        let (_d, mut c) = conn();
        let mut p = input();
        p.notes = Some("Contacto de la familia: CURP LOPM800101MDFRZN09".into());
        let SaveProfileOutcome::Quarantine { report } = save_profile(&mut c, p, None).unwrap() else { panic!() };
        assert!(report.has_blocking);
        assert_eq!(report.fields[0].path, "institution.notes");
        assert!(report.fields[0].redacted_preview.contains("[CURP OCULTA]"));
        assert!(!serde_json::to_string(&report).unwrap().contains("LOPM8"));
        assert!(get_profile(&c).unwrap().is_none(), "nothing may be saved before the decision");
    }

    #[test]
    fn redacting_saves_only_the_covered_text_and_logs_counts() {
        let (_d, mut c) = conn();
        let mut p = input();
        p.notes = Some("CURP LOPM800101MDFRZN09".into());
        let SaveProfileOutcome::Saved { profile } = save_profile(&mut c, p, Some(Decision::Redact)).unwrap() else { panic!() };
        assert_eq!(profile.input.notes.as_deref(), Some("CURP [CURP OCULTA]"));
        let ev = audit_events(&c);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].0, "scanner.quarantine");
        assert!(ev[0].1.contains("\"curp\":1") && ev[0].1.contains("redacted"));
        assert!(!ev[0].1.contains("LOPM"));
        // the original is nowhere: not in the profile, not in its history
        for (table, column) in [("mi_institucion_profile", "notes"), ("kernel_history", "after")] {
            let n: i64 = c.query_row(&format!("SELECT count(*) FROM {table} WHERE {column} LIKE '%LOPM8%'"), [], |r| r.get(0)).unwrap();
            assert_eq!(n, 0, "{table}");
        }
    }

    #[test]
    fn blocking_cannot_be_overridden_but_warnings_can() {
        let (_d, mut c) = conn();
        let mut p = input();
        p.notes = Some("CURP LOPM800101MDFRZN09".into());
        assert!(matches!(
            save_profile(&mut c, p, Some(Decision::NotPersonal)),
            Err(ServiceError::Guard(crate::scanner::guard::GuardError::BlockCannotBeIgnored))
        ));
        let mut p = input();
        p.notes = Some("Llamar al 55 1234 5678 para visitas".into());
        let out = save_profile(&mut c, p, Some(Decision::NotPersonal)).unwrap();
        assert!(matches!(out, SaveProfileOutcome::Saved { .. }));
        assert_eq!(audit_events(&c)[0].0, "scanner.override");
    }

    #[test]
    fn institutional_contact_is_not_scanned_and_not_flagged_elsewhere() {
        let (_d, mut c) = conn();
        let mut p = input();
        p.institution.contact_phone = Some("55 1234 5678".into());
        p.institution.legal_rep_name = Some("Rosa Hernández López".into());
        assert!(matches!(save_profile(&mut c, p.clone(), None).unwrap(), SaveProfileOutcome::Saved { .. }));
        // now the institution's own phone in a note is fine
        p.notes = Some("Informes al 55 1234 5678".into());
        assert!(matches!(save_profile(&mut c, p, None).unwrap(), SaveProfileOutcome::Saved { .. }));
    }

    /// Who wrote it is the person of the session; with no session, only the examples and the tests write.
    #[test]
    fn a_change_says_who_made_it() {
        let (_d, mut c) = conn();
        assert_eq!(origin(&c).unwrap(), kernel::Origin::Calculated { rule: "fixtures".into() });
        c.execute_batch("INSERT INTO temp.session_actor VALUES ('usr_01JB3M7Q8ZK4X2N6V9R5T0W1YD');").unwrap();
        save_profile(&mut c, input(), None).unwrap();
        let history = kernel::history(&c, &mi_institucion::service::profile_ref()).unwrap();
        assert_eq!(history[0].origin, kernel::Origin::Person { account: "usr_01JB3M7Q8ZK4X2N6V9R5T0W1YD".into() });
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::*;
    use crate::core::profile::domain::ProfileInput;
    use crate::storage::open_encrypted;

    /// Both fictitious profiles must load clean (no quarantine, no blocking issues).
    #[test]
    fn dev_fixtures_load_without_quarantine() {
        for (raw, padron) in [
            (include_str!("../../../../fixtures/institucion-asilo.json"), include_str!("../../../../fixtures/padron-asilo.json")),
            (include_str!("../../../../fixtures/institucion-casa-hogar.json"), include_str!("../../../../fixtures/padron-casa-hogar.json")),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut c = open_encrypted(&dir.path().join("t.db"), "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff").unwrap();
            let input: ProfileInput = serde_json::from_str(raw).unwrap();
            // as the example loader does: the institution, then its people, and the profile adds them up
            save_profile(&mut c, input.clone(), None).unwrap();
            crate::core::profile::sync::seed_examples(&mut c, padron).unwrap();
            let out = save_profile(&mut c, input, None).unwrap();
            let SaveProfileOutcome::Saved { profile } = out else { panic!("{out:?}") };
            assert!(profile.issues.is_empty(), "{:?}", profile.issues);
            assert!(profile.totals.population > 0 && profile.totals.staff_paid > 0);
            assert!(profile.totals.payroll_monthly_mxn > 0 && profile.totals.fees_monthly_mxn > 0, "the examples carry salaries and fees");
            let everything = serde_json::to_string(&profile.input).unwrap();
            assert!(!everything.contains("Esperanza Robles") && !everything.contains("Vázquez"), "no name reaches the profile");
        }
    }
}
