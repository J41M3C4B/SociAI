//! The photo of each month (ADR-033 §4): the figures the modules already compute, kept once a month so the progress
//! of the institution can be drawn without keeping old records. Only figures: never a person, and the balance only
//! in words (as the AI gets it), never its amount.
//!
//! The photo of the current month is taken again each time the app opens, so it ends up saying how the month ended;
//! once the month is over it is fixed (a trigger refuses to change a past month, and nothing is ever deleted).

use crate::core::error::ServiceError;
use crate::core::overview::Figure;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// The figures of one month. New figures are added with `#[serde(default)]`, so older photos still read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Figures {
    /// People served: the records, or the quick figure of the first start while there are none (`approx`).
    pub people: i64,
    pub people_approx: bool,
    pub capacity: Option<i64>,
    pub waiting: i64,
    pub staff: i64,
    pub staff_approx: bool,
    pub spaces: i64,
    pub spaces_good: i64,
    pub spaces_fair: i64,
    pub spaces_poor: i64,
    pub spaces_unusable: i64,
    pub spaces_unchecked: i64,
    /// `surplus`, `even` or `deficit`; `None` while the income or the expenses are not known.
    pub balance: Option<String>,
    /// How far «Mi institución» is filled in, 0 to 100.
    pub percent: u32,
    /// People in the board today.
    pub board: i64,
    /// The documents of the record that apply, but the ones marked as not applying (N4).
    #[serde(default)]
    pub records_expected: i64,
    /// Of those, the ones the institution has and are not expired: «expediente vigente».
    #[serde(default)]
    pub records_current: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    /// `YYYY-MM`.
    pub month: String,
    pub taken_at: String,
    pub figures: Figures,
}

/// The balance in words: what is left of the year is said, never its amount.
pub fn balance_word(balance: Option<i64>) -> Option<String> {
    balance.map(|b| match b {
        b if b < 0 => "deficit",
        0 => "even",
        _ => "surplus",
    }.to_string())
}

/// The figures of today. `None` while the institution has not been written yet: there is nothing to photograph.
pub fn figures(conn: &Connection) -> Result<Option<Figures>, ServiceError> {
    if crate::core::profile::storage::load_current(conn)?.is_none() {
        return Ok(None);
    }
    let o = crate::core::overview::institution_overview(conn)?;
    let fac = crate::modules::facilities::api::indicators(conn)?;
    let s = fac.spaces_states;
    let (Figure { value: people, approx: people_approx }, Figure { value: staff, approx: staff_approx }) = (o.people, o.staff);
    let records = crate::core::records::expediente(conn)?.summary;
    Ok(Some(Figures {
        people,
        people_approx,
        capacity: o.capacity,
        waiting: crate::modules::care::api::waiting(conn)?,
        staff,
        staff_approx,
        spaces: o.spaces,
        spaces_good: s.good,
        spaces_fair: s.fair,
        spaces_poor: s.poor,
        spaces_unusable: s.unusable,
        spaces_unchecked: fac.spaces_unchecked,
        balance: balance_word(o.balance_annual_mxn),
        percent: o.completion.percent,
        board: crate::core::protected::ai_board(conn)?.total as i64,
        records_expected: records.expected as i64,
        records_current: records.current() as i64,
    }))
}

/// The month of today on this computer (`YYYY-MM`).
pub fn this_month(conn: &Connection) -> Result<String, ServiceError> {
    Ok(conn.query_row("SELECT strftime('%Y-%m', 'now', 'localtime')", [], |r| r.get(0))?)
}

/// Takes the photo of this month, or takes it again. Returns whether there was something to photograph.
pub fn take(conn: &Connection) -> Result<bool, ServiceError> {
    let month = this_month(conn)?;
    take_for(conn, &month)
}

fn take_for(conn: &Connection, month: &str) -> Result<bool, ServiceError> {
    let Some(f) = figures(conn)? else { return Ok(false) };
    let json = serde_json::to_string(&f).map_err(|e| ServiceError::Internal(e.to_string()))?;
    conn.execute(
        "INSERT INTO core_snapshot (month, taken_at, figures_json) VALUES (?1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?2)
         ON CONFLICT(month) DO UPDATE SET taken_at = excluded.taken_at, figures_json = excluded.figures_json",
        params![month, json],
    )?;
    Ok(true)
}

/// Every photo, oldest first: what a progress chart reads.
pub fn snapshots(conn: &Connection) -> Result<Vec<Snapshot>, ServiceError> {
    let mut stmt = conn.prepare("SELECT month, taken_at, figures_json FROM core_snapshot ORDER BY month")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (month, taken_at, json) = row?;
        // a photo that does not read is left out, never guessed
        if let Ok(figures) = serde_json::from_str(&json) {
            out.push(Snapshot { month, taken_at, figures });
        }
    }
    Ok(out)
}

/// The photo of one month, if there is one.
pub fn of_month(conn: &Connection, month: &str) -> Result<Option<Snapshot>, ServiceError> {
    let row: Option<(String, String)> =
        conn.query_row("SELECT taken_at, figures_json FROM core_snapshot WHERE month = ?1", [month], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    Ok(row.and_then(|(taken_at, json)| serde_json::from_str(&json).ok().map(|figures| Snapshot { month: month.to_string(), taken_at, figures })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::profile::domain::{InstitutionInput, InstitutionKind, ProfileInput};

    fn conn() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let c = crate::storage::open_encrypted(&dir.path().join("t.db"), "k").unwrap();
        (dir, c)
    }

    fn institution(c: &mut Connection, served: i64) {
        let input = ProfileInput {
            institution: InstitutionInput { name: "Asilo Ficticio".into(), kind: InstitutionKind::ElderlyHome, ..Default::default() },
            capacity_total: Some(20),
            served_estimate: Some(served),
            ..Default::default()
        };
        crate::core::profile::storage::save(c, &input, &crate::core::profile::storage::fixtures()).unwrap();
    }

    #[test]
    fn without_an_institution_there_is_nothing_to_photograph() {
        let (_d, c) = conn();
        assert!(!take(&c).unwrap());
        assert!(snapshots(&c).unwrap().is_empty());
    }

    /// The month on course is taken again with each opening and ends up saying how the month ended.
    #[test]
    fn the_photo_of_this_month_follows_the_data_until_the_month_ends() {
        let (_d, mut c) = conn();
        institution(&mut c, 12);
        assert!(take(&c).unwrap());
        let month = this_month(&c).unwrap();
        let first = of_month(&c, &month).unwrap().unwrap();
        assert_eq!((first.figures.people, first.figures.people_approx, first.figures.capacity), (12, crate::core::modules::shown("people"), Some(20)));
        assert_eq!((first.figures.balance.clone(), first.figures.board), (None, 0));
        assert!(first.figures.percent < 100);

        institution(&mut c, 15);
        take(&c).unwrap();
        let all = snapshots(&c).unwrap();
        assert_eq!(all.len(), 1, "one photo per month");
        assert_eq!(all[0].figures.people, 15);
    }

    /// ADR-033 §4: a month that is over is fixed, and no photo is ever deleted.
    #[test]
    fn a_past_month_never_changes_and_nothing_is_deleted() {
        let (_d, mut c) = conn();
        institution(&mut c, 12);
        take_for(&c, "2000-01").unwrap();
        institution(&mut c, 30);
        assert!(take_for(&c, "2000-01").is_err(), "a past month is fixed");
        assert_eq!(of_month(&c, "2000-01").unwrap().unwrap().figures.people, 12);
        take(&c).unwrap();
        assert_eq!(snapshots(&c).unwrap().iter().map(|s| s.month.as_str()).next(), Some("2000-01"), "oldest first");
        assert!(c.execute("DELETE FROM core_snapshot", []).is_err());
    }

    /// Only figures: the balance goes as a word, never its amount, and no name of anybody.
    #[test]
    fn the_photo_holds_only_figures() {
        assert_eq!(balance_word(Some(-5)).as_deref(), Some("deficit"));
        assert_eq!(balance_word(Some(0)).as_deref(), Some("even"));
        assert_eq!(balance_word(Some(1)).as_deref(), Some("surplus"));
        assert_eq!(balance_word(None), None);
        let (_d, mut c) = conn();
        institution(&mut c, 12);
        let v = serde_json::json!({ "institution.board.name": "Presidenta Ficticia", "institution.board.role": "president" });
        crate::core::protected::save(&mut c, crate::core::protected::ListKind::BoardMember, None, serde_json::from_value(v).unwrap()).unwrap();
        take(&c).unwrap();
        let raw: String = c.query_row("SELECT figures_json FROM core_snapshot", [], |r| r.get(0)).unwrap();
        assert!(!raw.contains("Ficticia") && raw.contains("\"board\":1"), "{raw}");
    }

    /// «Expediente vigente» (N4): how many documents apply and how many it has, not expired. A photo taken before
    /// the record existed still reads.
    #[test]
    fn the_photo_counts_the_record_and_older_photos_still_read() {
        let (_d, mut c) = conn();
        institution(&mut c, 12);
        let have = serde_json::json!({ "institution.record.status": "have" });
        crate::core::records::save(&mut c, "articles_of_incorporation", None, serde_json::from_value(have).unwrap(), None).unwrap();
        take(&c).unwrap();
        let f = of_month(&c, &this_month(&c).unwrap()).unwrap().unwrap().figures;
        let summary = crate::core::records::expediente(&c).unwrap().summary;
        assert!(summary.expected > 12, "an elderly home also needs its sanitary notice");
        assert_eq!((f.records_expected, f.records_current), (summary.expected as i64, 1));

        let mut old = serde_json::to_value(&f).unwrap();
        old.as_object_mut().unwrap().retain(|k, _| !k.starts_with("records_"));
        let read: Figures = serde_json::from_value(old).unwrap();
        assert_eq!((read.records_expected, read.records_current), (0, 0));
    }
}
