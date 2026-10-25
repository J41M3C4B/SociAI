//! The profile of the institution as the rest of the app still reads it (`ProfileInput`). Its data are the module
//! Mi institución (ERP-1): this reads them from there and writes them there, window by window, through the kernel.
//! What stays here is what the module does not own: the institution the projects are tied to (`institution`), and
//! the anonymous lines of the staff and the people served, computed by their modules, hanging from its row of
//! `institution_profile`.

use crate::core::institution::catalog::kind_of;
use crate::core::profile::domain::*;
use crate::scanner::ScannerConfig;
use crate::storage::StorageError;
use mi_institucion::contract::WindowInput;
use mi_institucion::windows::WINDOWS;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Map, Value};
use ulid::Ulid;

fn id(prefix: &str) -> String {
    format!("{prefix}_{}", Ulid::generate())
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredProfile {
    pub institution_id: String,
    pub profile_id: String,
    /// The version of the profile in the module: it rises with every change.
    pub version: i64,
    /// When the first start was finished (D3 of ERP-1: what is saved is the profile, with no draft).
    pub confirmed_at: Option<String>,
    pub input: ProfileInput,
    /// The year the sums are made in (it sets how long each person has worked, for the benefits).
    pub as_of_year: i64,
}

/// The current year, from the clock of the database.
pub fn current_year(conn: &Connection) -> Result<i64, StorageError> {
    Ok(conn.query_row("SELECT CAST(strftime('%Y','now') AS INTEGER)", [], |r| r.get(0))?)
}

/// The lists that hang from the row of the profile. `income_source` and `expense_item` hang from the rows written
/// before the money became a module of its own (migration 0020); they are history and not written any more.
const PROFILE_LISTS: [&str; 2] = ["population_group", "staff_group"];

fn text(o: &Option<String>) -> Option<&str> {
    o.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

// ------------------------------------------------------------------ the profile and the module

/// The data of a profile, by the id of each field in the module's dictionary (`legal_name`).
pub fn values_of(p: &ProfileInput) -> Map<String, Value> {
    let i = &p.institution;
    let a = i.attention.clone().unwrap_or_default();
    let mut v = Map::new();
    let mut put = |field: &str, value: Value| {
        v.insert(field.to_string(), value);
    };
    put("name", json!(i.name));
    put("mission", json!(i.mission));
    put("populations", json!(a.populations));
    put("sex_served", json!(a.sex_served));
    put("modalities", json!(a.modalities));
    put("care_areas", json!(a.care_areas));
    put("contact_phone", json!(i.contact_phone));
    put("contact_email", json!(i.contact_email));
    put("state", json!(i.state));
    put("municipality", json!(i.municipality));
    put("legal_rfc", json!(i.legal_rfc));
    put("legal_rep_name", json!(i.legal_rep_name));
    put("legal_form", json!(i.legal_form));
    put("founded_year", json!(i.founded_year));
    put("authorized_donee", json!(i.authorized_donee));
    put("cluni", json!(i.cluni));
    put("capacity_total", json!(p.capacity_total));
    put("served_estimate", json!(p.served_estimate));
    put("staff_paid_estimate", json!(p.staff_paid_estimate));
    put("staff_volunteer_estimate", json!(p.staff_volunteer_estimate));
    put("notes", json!(p.notes));
    for (id, x) in i.details.iter().flatten() {
        if let Some(f) = crate::core::institution::forms::module_id(id) {
            v.insert(f.to_string(), x.clone());
        }
    }
    v.retain(|_, x| crate::common::forms::is_filled(Some(x)));
    v
}

fn string(v: &Map<String, Value>, f: &str) -> Option<String> {
    v.get(f).and_then(Value::as_str).map(str::to_string)
}

fn number(v: &Map<String, Value>, f: &str) -> Option<i64> {
    v.get(f).and_then(Value::as_i64)
}

fn codes(v: &Map<String, Value>, f: &str) -> Vec<String> {
    v.get(f).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

/// The profile as the app reads it, from the data of the module. Every datum is also among the details, by the id the
/// screen gives it (`institution.legal_name`).
pub fn input_of(v: &Map<String, Value>) -> ProfileInput {
    let populations = codes(v, "populations");
    let institution = InstitutionInput {
        name: string(v, "name").unwrap_or_default(),
        kind: InstitutionKind::from_db(kind_of(&populations)),
        mission: string(v, "mission"),
        legal_rfc: string(v, "legal_rfc"),
        contact_phone: string(v, "contact_phone"),
        contact_email: string(v, "contact_email"),
        legal_rep_name: string(v, "legal_rep_name"),
        state: string(v, "state"),
        municipality: string(v, "municipality"),
        founded_year: number(v, "founded_year"),
        legal_form: string(v, "legal_form"),
        authorized_donee: string(v, "authorized_donee"),
        cluni: string(v, "cluni"),
        attention: Some(Attention { populations, sex_served: string(v, "sex_served"), modalities: codes(v, "modalities"), care_areas: codes(v, "care_areas") }),
        details: Some(v.iter().map(|(k, x)| (crate::core::institution::forms::screen_id(k), x.clone())).collect()),
    };
    ProfileInput {
        institution,
        capacity_total: number(v, "capacity_total"),
        notes: string(v, "notes"),
        served_estimate: number(v, "served_estimate"),
        staff_paid_estimate: number(v, "staff_paid_estimate"),
        staff_volunteer_estimate: number(v, "staff_volunteer_estimate"),
        population: Vec::new(),
        staff: Vec::new(),
    }
}

/// The institution the projects are tied to and the row the anonymous lines hang from: (institution, profile).
fn anchor(conn: &Connection) -> Result<Option<(String, String, Option<String>)>, StorageError> {
    Ok(conn
        .query_row(
            "SELECT i.id, p.id, i.onboarded_at FROM institution i JOIN institution_profile p ON p.institution_id = i.id
             ORDER BY p.version DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?)
}

/// Makes the institution and the row of its lines the first time the profile is saved, so a project can be tied to
/// them. Its name and kind are the module's; the columns of before keep what they had.
pub fn ensure_anchor(conn: &Connection) -> Result<(String, String), StorageError> {
    if let Some((institution, profile, _)) = anchor(conn)? {
        return Ok((institution, profile));
    }
    let institution: String = match conn.query_row("SELECT id FROM institution LIMIT 1", [], |r| r.get(0)).optional()? {
        Some(id) => id,
        None => {
            let iid = id("inst");
            conn.execute(
                "INSERT INTO institution (id, name, kind, created_at, updated_at)
                 VALUES (?1, '', 'other', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
                [&iid],
            )?;
            iid
        }
    };
    let profile = id("prof");
    conn.execute(
        "INSERT INTO institution_profile (id, institution_id, version, created_at) VALUES (?1, ?2, 1, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        params![profile, institution],
    )?;
    Ok((institution, profile))
}

/// The profile now, if it was ever saved.
pub fn load_current(conn: &Connection) -> Result<Option<StoredProfile>, StorageError> {
    let now = mi_institucion::service::profile(conn)?;
    if now.version == 0 {
        return Ok(None);
    }
    let Some((institution_id, profile_id, onboarded_at)) = anchor(conn)? else { return Ok(None) };
    let mut input = input_of(&now.values);
    input.population = conn
        .prepare("SELECT label, age_min, age_max, count, dependency_level, notes, paying_count, monthly_fee_mxn FROM population_group WHERE profile_id = ?1 ORDER BY rowid")?
        .query_map([&profile_id], |r| {
            Ok(PopulationGroupInput {
                label: r.get(0)?,
                age_min: r.get(1)?,
                age_max: r.get(2)?,
                count: r.get(3)?,
                dependency_level: r.get::<_, Option<String>>(4)?.and_then(|s| DependencyLevel::from_db(&s)),
                notes: r.get(5)?,
                paying_count: r.get(6)?,
                monthly_fee_mxn: r.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    input.staff = conn
        .prepare("SELECT role, count, shift, paid, monthly_salary_mxn, contract, start_year, notes, relation FROM staff_group WHERE profile_id = ?1 ORDER BY rowid")?
        .query_map([&profile_id], |r| {
            Ok(StaffGroupInput {
                role: r.get(0)?,
                count: r.get(1)?,
                shift: r.get(2)?,
                paid: r.get::<_, i64>(3)? != 0,
                monthly_salary_mxn: r.get(4)?,
                contract: r.get::<_, Option<String>>(5)?.and_then(|s| ContractKind::from_db(&s)),
                start_year: r.get(6)?,
                notes: r.get(7)?,
                relation: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(StoredProfile { institution_id, profile_id, version: now.version, confirmed_at: onboarded_at, input, as_of_year: current_year(conn)? }))
}

/// Saves one window of the profile in the module, with its origin, and makes the institution the first time. What
/// the window already says is not a change.
pub fn save_window(conn: &mut Connection, window: &str, values: Map<String, Value>, origin: &kernel::Origin) -> Result<(), StorageError> {
    match mi_institucion::service::apply(conn, &WindowInput { window: window.into(), values }, None, origin) {
        Ok(_) | Err(kernel::ApplyError::NothingToChange) => {}
        Err(kernel::ApplyError::Db(e)) => return Err(e.into()),
        Err(kernel::ApplyError::Invalid(found)) => return Err(StorageError::Invalid(found)),
        Err(e) => return Err(StorageError::Change(e.code())),
    }
    ensure_anchor(conn)?;
    Ok(())
}

/// Saves a whole profile, window by window, and the anonymous lines it brings: the examples of `fixtures/` and the
/// tests write it this way. A kind with nobody marked as served fills whom it serves from the kind.
pub fn save(conn: &mut Connection, input: &ProfileInput, origin: &kernel::Origin) -> Result<StoredProfile, StorageError> {
    let mut input = input.clone();
    if input.institution.attention.as_ref().is_none_or(|a| a.populations.is_empty()) {
        let (_, attention) = reconcile_attention(None, input.institution.kind, None);
        input.institution.attention = Some(Attention { sex_served: input.institution.attention.and_then(|a| a.sex_served), ..attention });
    }
    let values = values_of(&input);
    for w in WINDOWS {
        let shown: Map<String, Value> = values.iter().filter(|(k, _)| w.has(k)).map(|(k, v)| (k.clone(), v.clone())).collect();
        save_window(conn, w.id, shown, origin)?;
    }
    refresh_lines(conn, &input.staff, &input.population)?;
    load_current(conn)?.ok_or(StorageError::NoProfile)
}

// ------------------------------------------------------------------ the lines and the institution

/// The anonymous lines of the profile.
fn insert_lines(tx: &rusqlite::Transaction<'_>, profile_id: &str, staff: &[StaffGroupInput], population: &[PopulationGroupInput]) -> Result<(), StorageError> {
    for g in population {
        tx.execute(
            "INSERT INTO population_group (id,profile_id,label,age_min,age_max,count,dependency_level,notes,paying_count,monthly_fee_mxn,origin)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'computed')",
            params![id("pop"), profile_id, g.label.trim(), g.age_min, g.age_max, g.count,
                    g.dependency_level.map(|d| d.as_db()), text(&g.notes), g.paying_count, g.monthly_fee_mxn],
        )?;
    }
    for s in staff {
        tx.execute(
            "INSERT INTO staff_group (id,profile_id,role,count,shift,paid,monthly_salary_mxn,contract,start_year,notes,relation,origin)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'computed')",
            params![id("staff"), profile_id, s.role.trim(), s.count, text(&s.shift), s.paid as i64,
                    s.monthly_salary_mxn, s.contract.map(|c| c.as_db()), s.start_year, text(&s.notes), text(&s.relation)],
        )?;
    }
    Ok(())
}

/// Replaces the anonymous lines in place (audit D1). The lines only mirror what the modules hold now; the progress
/// over time is kept by the snapshots (ADR-033 §4).
pub fn refresh_lines(conn: &mut Connection, staff: &[StaffGroupInput], population: &[PopulationGroupInput]) -> Result<Option<StoredProfile>, StorageError> {
    let tx = conn.transaction()?;
    let latest: Option<String> =
        tx.query_row("SELECT id FROM institution_profile ORDER BY version DESC LIMIT 1", [], |r| r.get(0)).optional()?;
    let Some(profile_id) = latest else { return Ok(None) };
    for t in PROFILE_LISTS {
        tx.execute(&format!("DELETE FROM {t} WHERE profile_id=?1"), [&profile_id])?;
    }
    insert_lines(&tx, &profile_id, staff, population)?;
    tx.commit()?;
    load_current(conn)
}

/// Days since the first start was finished (`None` before). The projects ask it through `core::api` (the stage
/// «Perfil»): what is saved is the profile, with no confirmation to renew (D3 of ERP-1).
pub fn confirmed_days_ago(conn: &Connection) -> Result<Option<i64>, StorageError> {
    Ok(conn
        .query_row("SELECT CAST(julianday('now') - julianday(onboarded_at) AS INTEGER) FROM institution WHERE onboarded_at IS NOT NULL LIMIT 1", [], |r| {
            r.get(0)
        })
        .optional()?)
}

/// The institution and the row of its profile, which a new project is tied to, once the first start is finished.
pub fn latest_confirmed(conn: &Connection) -> Result<Option<(String, String)>, StorageError> {
    Ok(anchor(conn)?.filter(|(_, _, onboarded)| onboarded.is_some()).map(|(i, p, _)| (i, p)))
}

/// What the scanner must not flag: the institution's own name, contact and representative.
pub fn scanner_config(conn: &Connection) -> Result<ScannerConfig, StorageError> {
    let v = mi_institucion::service::profile(conn)?.values;
    // the accounts and the board are the institution's own too (ADR-033 §3)
    let (clabes, board) = crate::core::protected::storage::own_clabes_and_names(conn)?;
    Ok(ScannerConfig {
        institutional_phones: string(&v, "contact_phone").into_iter().collect(),
        institutional_emails: string(&v, "contact_email").into_iter().collect(),
        safe_phrases: string(&v, "name").into_iter().chain(string(&v, "legal_rep_name")).chain(board).collect(),
        institutional_clabes: clabes,
        cleared: Vec::new(),
    })
}

/// The origin of the data the tests write, with no person (as the examples of `fixtures/`).
#[cfg(test)]
pub fn fixtures() -> kernel::Origin {
    kernel::Origin::Calculated { rule: "fixtures".into() }
}

/// Saves a profile as the tests need it, and finishes the first start: what a project is tied to.
#[cfg(test)]
pub fn save_finished(conn: &mut Connection, input: &ProfileInput) -> StoredProfile {
    save(conn, input, &fixtures()).unwrap();
    finish_first_start(conn);
    load_current(conn).unwrap().unwrap()
}

/// The first start finished, for the tests.
#[cfg(test)]
pub fn finish_first_start(conn: &Connection) {
    conn.execute("UPDATE institution SET onboarded_at = coalesce(onboarded_at, strftime('%Y-%m-%dT%H:%M:%SZ','now'))", []).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::open_encrypted;

    const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn conn() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let c = open_encrypted(&dir.path().join("t.db"), KEY).unwrap();
        (dir, c)
    }

    fn sample() -> ProfileInput {
        ProfileInput {
            institution: InstitutionInput {
                name: "Casa Hogar Ficticia".into(),
                kind: InstitutionKind::ElderlyHome,
                contact_phone: Some("55 1234 5678".into()),
                ..Default::default()
            },
            capacity_total: Some(25),
            population: vec![PopulationGroupInput {
                label: "Adultos mayores".into(), count: 18, dependency_level: Some(DependencyLevel::High), ..Default::default()
            }],
            staff: vec![StaffGroupInput { role: "Enfermería".into(), count: 3, paid: true, ..Default::default() }],
            ..Default::default()
        }
    }

    fn fixtures() -> kernel::Origin {
        kernel::Origin::Calculated { rule: "fixtures".into() }
    }

    #[test]
    fn saves_and_loads_a_profile() {
        let (_d, mut c) = conn();
        assert!(load_current(&c).unwrap().is_none());
        let saved = save(&mut c, &sample(), &fixtures()).unwrap();
        assert!(saved.confirmed_at.is_none(), "the first start is not finished");
        assert_eq!(saved.input.population[0].count, 18);
        assert_eq!(saved.input.institution.kind, InstitutionKind::ElderlyHome, "the kind fills whom it serves");
        assert_eq!(saved.input.institution.attention.unwrap().modalities, ["residential"]);
        assert_eq!(saved.input.capacity_total, Some(25));
        assert!((2026..2200).contains(&saved.as_of_year));
        // the data are the module's, and the history of each window is the kernel's
        let module = mi_institucion::service::profile(&c).unwrap();
        assert_eq!(module.values["contact_phone"], json!("55 1234 5678"));
        assert_eq!(saved.version, module.version);
        assert_eq!(saved.input.institution.details.unwrap()["institution.contact_phone"], json!("55 1234 5678"));
    }

    #[test]
    fn the_lines_are_replaced_in_place() {
        let (_d, mut c) = conn();
        save(&mut c, &sample(), &fixtures()).unwrap();
        let mut p = sample();
        p.population[0].count = 19;
        p.population.push(PopulationGroupInput { label: "Otro".into(), count: 2, ..Default::default() });
        let s = save(&mut c, &p, &fixtures()).unwrap();
        assert_eq!(s.input.population.len(), 2);
        let n: i64 = c.query_row("SELECT count(*) FROM population_group", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
        let rows: i64 = c.query_row("SELECT count(*) FROM institution_profile", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 1, "no version of the profile is kept here");
    }

    /// A project is tied to the institution once the first start is finished, and the stage «Perfil» counts from then.
    #[test]
    fn the_profile_counts_as_confirmed_once_the_first_start_is_finished() {
        let (_d, mut c) = conn();
        save(&mut c, &sample(), &fixtures()).unwrap();
        assert_eq!((latest_confirmed(&c).unwrap(), confirmed_days_ago(&c).unwrap()), (None, None));
        c.execute("UPDATE institution SET onboarded_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')", []).unwrap();
        assert!(latest_confirmed(&c).unwrap().is_some());
        assert_eq!(confirmed_days_ago(&c).unwrap(), Some(0));
        assert!(load_current(&c).unwrap().unwrap().confirmed_at.is_some());
    }

    #[test]
    fn scanner_config_comes_from_the_institution() {
        let (_d, mut c) = conn();
        assert!(scanner_config(&c).unwrap().safe_phrases.is_empty());
        save(&mut c, &sample(), &fixtures()).unwrap();
        let cfg = scanner_config(&c).unwrap();
        assert_eq!(cfg.institutional_phones, vec!["55 1234 5678"]);
        assert_eq!(cfg.safe_phrases, vec!["Casa Hogar Ficticia"]);
    }
}
