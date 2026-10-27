//! What is proposed to change in «Mi institución» (ADR-034 §2), today by the rules that read a document. Nothing is
//! written by the proposer: a proposal names a field of a form of the
//! catalog, carries a value that fits that field, says where it comes from and waits until a person accepts or
//! rejects it. Accepting is the usual save of the form (IA4): the person puts the suggestion in its field, may
//! correct it, and «Guardar» marks it accepted in the same transaction as the datum.

use crate::common::forms::{is_filled, problem, AiUse};
use crate::core::error::ServiceError;
use crate::scanner::guard::Decision;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The AI worked it out from what the person told it or from another part of the ERP.
    AiAssumption,
    /// Read from a document of the institution (`source_ref` says which and where).
    Document,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::AiAssumption => "ai_assumption",
            Origin::Document => "document",
        }
    }
    fn from_db(s: &str) -> Origin {
        if s == "document" { Origin::Document } else { Origin::AiAssumption }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewProposal {
    pub form_id: String,
    pub record_id: Option<String>,
    pub field_id: String,
    pub value: Value,
    pub origin: Origin,
    pub source_ref: Option<String>,
    pub agent: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Proposal {
    pub id: String,
    pub form_id: String,
    pub record_id: Option<String>,
    pub field_id: String,
    pub value: Value,
    pub origin: Origin,
    pub source_ref: Option<String>,
    pub agent: String,
    pub status: String,
    pub created_at: String,
    /// The words of the person it was taken from, when it came from the chat (`source_ref`).
    pub said: Option<String>,
    /// The document it was read from, by its name, and the page, when it came from a document (IA6).
    pub document: Option<String>,
    pub page: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposeOutcome {
    Kept { id: String },
    /// Why it was not kept: `form_unknown`, `field_unknown`, `not_for_the_ai` (a field the AI never sees), `empty`,
    /// or the problem of the value (`code_unknown`, `number_too_large`, …).
    Refused { code: &'static str },
}

/// Keeps a proposal if it fits its field. A text goes through the scanner first and whatever it finds is covered:
/// there is no person in front to decide, and the save that follows the acceptance scans it again. A newer proposal
/// for the same field replaces the one still waiting.
pub fn propose(conn: &Connection, p: NewProposal) -> Result<ProposeOutcome, ServiceError> {
    let refused = |code| Ok(ProposeOutcome::Refused { code });
    let Some(form) = crate::core::institution::forms::form(&p.form_id) else { return refused("form_unknown") };
    let Some(field) = form.field(&p.field_id) else { return refused("field_unknown") };
    // what the AI never sees it cannot propose; a document can (read by rules, ADR-034 §3)
    if p.origin == Origin::AiAssumption && field.ai == AiUse::Never {
        return refused("not_for_the_ai");
    }
    if !is_filled(Some(&p.value)) {
        return refused("empty");
    }
    if let Some(code) = problem(field, &p.value) {
        return refused(code);
    }
    if p.origin == Origin::Document {
        if let Some(code) = from_a_document(conn, &p)? {
            return refused(code);
        }
    }
    // a protected datum of the institution read by a rule (its RFC, a folio) is its own, not a person's: covering it
    // would lose it; it never reaches the AI and its value is forgotten once the person decides (`forget_protected`)
    let own = p.origin == Origin::Document && field.protected();
    // the name of a place is not scanned, as when the person saves it (`profile::domain`): many are named after a person
    let place = field.rule == Some(crate::common::forms::Rule::PlaceName);
    let value = match &p.value {
        Value::String(s) if !own && !place => {
            let fields = [(p.field_id.clone(), s.clone())];
            match crate::core::screen::guard_texts(conn, "ai_proposal", &fields, Some(Decision::Redact))? {
                Ok(mut texts) => Value::String(texts.remove(0)),
                // with «cover it» decided there is no quarantine; should there be one, nothing is kept
                Err(_) => return refused("not_for_the_ai"),
            }
        }
        other => other.clone(),
    };
    let value = as_written(conn, &p.field_id, value)?;
    conn.execute(
        "DELETE FROM ai_proposal WHERE form_id = ?1 AND record_id IS ?2 AND field_id = ?3 AND status = 'pending'",
        params![p.form_id, p.record_id, p.field_id],
    )?;
    let id = format!("prop_{}", ulid::Ulid::generate());
    conn.execute(
        "INSERT INTO ai_proposal (id, form_id, record_id, field_id, value_json, origin, source_ref, agent, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        params![id, p.form_id, p.record_id, p.field_id, value.to_string(), p.origin.as_str(), p.source_ref, p.agent],
    )?;
    Ok(ProposeOutcome::Kept { id })
}

/// A suggestion written as the program writes that field (ADR-042, phase 5), whoever made it: a municipality with its
/// official name, in its state when it is saved or waits («BENITO JUAREZ» is «Benito Juárez»); a street without
/// «Calle» before it, which its label already says («Calle de los Ejemplos» is «de los Ejemplos»). The policy above
/// still reads the value as it came (its fingerprint), so what was rejected stays rejected.
fn as_written(conn: &Connection, field: &str, value: Value) -> Result<Value, ServiceError> {
    let Value::String(s) = &value else { return Ok(value) };
    Ok(match field {
        "institution.municipality" => {
            let saved = saved_value(conn, "institution.contact", "institution.state")?;
            let waiting = || -> Result<Option<Value>, ServiceError> {
                let v: Option<String> = conn
                    .query_row(
                        "SELECT value_json FROM ai_proposal WHERE form_id = 'institution.contact' AND field_id = 'institution.state' AND status = 'pending' ORDER BY created_at DESC LIMIT 1",
                        [],
                        |r| r.get(0),
                    )
                    .optional()?;
                Ok(v.and_then(|v| serde_json::from_str(&v).ok()))
            };
            let state = match saved.filter(|v| is_filled(Some(v))) {
                Some(v) => Some(v),
                None => waiting()?,
            };
            let state = state.as_ref().and_then(Value::as_str);
            crate::core::institution::catalog::municipality_name(state, s).map_or(value.clone(), |name| Value::String(name.to_string()))
        }
        "institution.street" => {
            let t = s.trim();
            let rest = ["calle ", "c. "].iter().find_map(|w| t.get(..w.len()).filter(|h| h.eq_ignore_ascii_case(w)).map(|_| t[w.len()..].trim()));
            match rest {
                Some(r) if !r.is_empty() => Value::String(r.to_string()),
                _ => value,
            }
        }
        _ => value,
    })
}

/// The one policy for what a document suggests (ADR-038), the same for every drawer:
///
/// - `filled`: a field that already has a value gets nothing, whatever the document says (decision of the
///   institution, 2026-10-13);
/// - `rejected_before`: what the person once rejected for a field is not suggested again, by the fingerprint of the
///   value (`sha` in `source_ref`), which outlives the value and the document;
/// - `lower_authority`: a suggestion still waiting that came from a document with more say on that field (its
///   `rank` in `source_ref`, `reader::authority`) is not replaced by one with less; with the same, the newer wins.
///
/// What the AI proposes is not held to it: the person asked for that change in the chat.
fn from_a_document(conn: &Connection, p: &NewProposal) -> Result<Option<&'static str>, ServiceError> {
    if is_filled(saved_value(conn, &p.form_id, &p.field_id)?.as_ref()) {
        return Ok(Some("filled"));
    }
    // by the fingerprint of the value: it stays when the value is forgotten or its document is deleted
    let rejected: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM ai_proposal WHERE form_id = ?1 AND record_id IS ?2 AND field_id = ?3 AND status = 'rejected'
                        AND origin = 'document' AND json_valid(source_ref) AND json_extract(source_ref, '$.sha') = ?4)",
        params![p.form_id, p.record_id, p.field_id, fingerprint(&p.value)],
        |r| r.get(0),
    )?;
    if rejected {
        return Ok(Some("rejected_before"));
    }
    let waiting: Option<i64> = conn.query_row(
        "SELECT max(coalesce(json_extract(source_ref, '$.rank'), 0)) FROM ai_proposal
         WHERE form_id = ?1 AND record_id IS ?2 AND field_id = ?3 AND status = 'pending' AND origin = 'document' AND json_valid(source_ref)",
        params![p.form_id, p.record_id, p.field_id],
        |r| r.get(0),
    )?;
    if waiting.is_some_and(|w| w > rank_of(p.source_ref.as_deref())) {
        return Ok(Some("lower_authority"));
    }
    Ok(None)
}

/// The fingerprint of a value a document suggested (SHA-256 of its JSON): enough to know the person already said
/// no to it, without keeping the value.
pub fn fingerprint(value: &Value) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(value.to_string().as_bytes()))
}

/// How much say the document a proposal came from has on its field (`rank` in its `source_ref`); 0 without one.
fn rank_of(source_ref: Option<&str>) -> i64 {
    source_ref.and_then(|s| serde_json::from_str::<Value>(s).ok()).and_then(|v| v["rank"].as_i64()).unwrap_or(0)
}

/// What a field of a form of «Mi institución» holds today; `None` for the forms of lists, which hold their own.
fn saved_value(conn: &Connection, form_id: &str, field_id: &str) -> Result<Option<Value>, ServiceError> {
    if !crate::core::institution::forms::forms().iter().any(|f| f.id == form_id) {
        return Ok(None);
    }
    Ok(crate::core::profile::forms::saved_values(conn, form_id)?.remove(field_id))
}

/// Once a field is saved with a value, what documents suggested for it is set aside, as if the person had said no
/// (ADR-038): it will not show again. Called in the transaction of the save. Says how many.
pub fn discard_filled(conn: &Connection, form_id: &str, record_id: Option<&str>, values: &crate::common::forms::Values) -> rusqlite::Result<usize> {
    let mut stmt = conn.prepare("SELECT id, field_id FROM ai_proposal WHERE form_id = ?1 AND record_id IS ?2 AND status = 'pending' AND origin = 'document'")?;
    let ids: Vec<String> = stmt
        .query_map(params![form_id, record_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|(_, field)| is_filled(values.get(field)))
        .map(|(id, _)| id)
        .collect();
    for id in &ids {
        conn.execute("UPDATE ai_proposal SET status = 'rejected', resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?1", [id])?;
        forget_protected(conn, id)?;
    }
    Ok(ids.len())
}

fn read(r: &rusqlite::Row) -> rusqlite::Result<Proposal> {
    let value: String = r.get(4)?;
    let origin: String = r.get(5)?;
    let source_ref: Option<String> = r.get(6)?;
    let source = source_ref.as_deref().and_then(|s| serde_json::from_str::<Value>(s).ok()).unwrap_or(Value::Null);
    let from_document = source["document_id"].is_string();
    let said = source["quote"].as_str().filter(|_| !from_document).map(str::to_string);
    Ok(Proposal {
        id: r.get(0)?,
        form_id: r.get(1)?,
        record_id: r.get(2)?,
        field_id: r.get(3)?,
        value: serde_json::from_str(&value).unwrap_or(Value::Null),
        origin: Origin::from_db(&origin),
        source_ref,
        agent: r.get(7)?,
        status: r.get(8)?,
        created_at: r.get(9)?,
        said,
        document: if from_document { r.get(10)? } else { None },
        page: source["page"].as_i64().filter(|_| from_document),
    })
}

const COLUMNS: &str = "id, form_id, record_id, field_id, value_json, origin, source_ref, agent, status, created_at,
    (SELECT d.display_name FROM document d WHERE json_valid(source_ref) AND d.id = json_extract(source_ref, '$.document_id'))";

/// Once a person decided, the value of a protected datum is not kept in the proposal: it is in its field, or nowhere.
fn forget_protected(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    let field: Option<String> = conn.query_row("SELECT field_id FROM ai_proposal WHERE id = ?1", [id], |r| r.get(0)).optional()?;
    let protected = field.and_then(|f| crate::core::institution::forms::field(&f)).is_some_and(|f| f.protected());
    if protected {
        conn.execute("UPDATE ai_proposal SET value_json = 'null' WHERE id = ?1 AND status <> 'pending'", [id])?;
    }
    Ok(())
}

/// The proposals still waiting for a form (and its record), in the order they came.
pub fn pending(conn: &Connection, form_id: &str, record_id: Option<&str>) -> Result<Vec<Proposal>, ServiceError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM ai_proposal WHERE form_id = ?1 AND record_id IS ?2 AND status = 'pending' ORDER BY created_at, id"
    ))?;
    let rows = stmt.query_map(params![form_id, record_id], read)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The proposals of these ids still waiting for this form: the ones a person took in a save.
pub fn waiting(conn: &Connection, form_id: &str, ids: &[String]) -> Result<Vec<Proposal>, ServiceError> {
    Ok(pending(conn, form_id, None)?.into_iter().filter(|p| ids.contains(&p.id)).collect())
}

/// Marks these waiting proposals as accepted by whoever acts in this connection. Called inside the transaction of
/// the save that writes their data.
pub fn accept_in(conn: &Connection, ids: &[String]) -> rusqlite::Result<usize> {
    conn.execute_batch("CREATE TEMP TABLE IF NOT EXISTS session_actor (user_id TEXT);")?;
    let mut n = 0;
    for id in ids {
        n += conn.execute(
            "UPDATE ai_proposal SET status = 'accepted', resolved_by = (SELECT user_id FROM temp.session_actor LIMIT 1),
                    resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ?1 AND status = 'pending'",
            [id],
        )?;
        forget_protected(conn, id)?;
    }
    Ok(n)
}

/// The person discards these suggestions: nothing of the institution changes. Returns how many were waiting.
pub fn reject(conn: &Connection, ids: &[String], by: Option<&str>) -> Result<usize, ServiceError> {
    let mut n = 0;
    for id in ids {
        n += usize::from(resolve(conn, id, false, by)?.is_some());
    }
    Ok(n)
}

/// How many suggestions wait in each form, for «Mi institución» to say it.
pub fn pending_counts(conn: &Connection) -> Result<Vec<(String, i64)>, ServiceError> {
    let mut stmt = conn.prepare("SELECT form_id, count(*) FROM ai_proposal WHERE status = 'pending' GROUP BY form_id ORDER BY form_id")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Marks a waiting proposal as accepted or rejected, with who did it. Only a waiting one can be resolved, once;
/// `None` when there is none with that id waiting.
pub fn resolve(conn: &Connection, id: &str, accepted: bool, by: Option<&str>) -> Result<Option<Proposal>, ServiceError> {
    let changed = conn.execute(
        "UPDATE ai_proposal SET status = ?2, resolved_by = ?3, resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ?1 AND status = 'pending'",
        params![id, if accepted { "accepted" } else { "rejected" }, by],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    let resolved = conn.query_row(&format!("SELECT {COLUMNS} FROM ai_proposal WHERE id = ?1"), [id], read).optional()?;
    forget_protected(conn, id)?;
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn conn() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let c = crate::storage::open_encrypted(&dir.path().join("t.db"), KEY).unwrap();
        (dir, c)
    }

    fn new(field: &str, value: Value) -> NewProposal {
        NewProposal {
            form_id: "institution.identity".into(),
            record_id: None,
            field_id: field.into(),
            value,
            origin: Origin::AiAssumption,
            source_ref: Some("chat".into()),
            agent: "reader",
        }
    }

    #[test]
    fn a_municipality_takes_its_official_name_and_its_state_decides() {
        use crate::core::institution::catalog::municipality_name;
        assert_eq!(municipality_name(Some("cdmx"), "BENITO JUAREZ"), Some("Benito Juárez"));
        assert_eq!(municipality_name(Some("cdmx"), "Alcaldía benito juárez"), Some("Benito Juárez"));
        assert_eq!(municipality_name(Some("jal"), "zapopan"), Some("Zapopan"));
        assert_eq!(municipality_name(Some("cdmx"), "GUSTAVO A MADERO"), Some("Gustavo A. Madero"));
        assert_eq!(municipality_name(None, "BENITO JUAREZ"), Some("Benito Juárez"), "the same name in three states");
        assert_eq!(municipality_name(Some("jal"), "Benito Juárez"), None, "not a municipality of that state");
        assert_eq!(municipality_name(None, "Ciudad Inventada"), None);
    }

    /// ADR-042, phase 5: whoever suggests them, the municipality is kept with its official name and the street without
    /// the «Calle» its label already says; what was rejected is still read by the value as it came.
    #[test]
    fn a_suggestion_is_kept_as_the_program_writes_its_field() {
        let (_d, c) = conn();
        let contact = |field: &str, value: Value| NewProposal { form_id: "institution.contact".into(), origin: Origin::Document, ..new(field, value) };
        assert!(matches!(propose(&c, contact("institution.state", json!("cdmx"))).unwrap(), ProposeOutcome::Kept { .. }));
        for (field, said, kept) in [
            ("institution.municipality", "BENITO JUAREZ", "Benito Juárez"),
            ("institution.street", "Calle de los Ejemplos", "de los Ejemplos"),
            ("institution.street", "CALLE Hidalgo", "Hidalgo"),
            ("institution.street", "Avenida Insurgentes", "Avenida Insurgentes"),
            ("institution.municipality", "Ciudad Inventada", "Ciudad Inventada"),
        ] {
            propose(&c, contact(field, json!(said))).unwrap();
            let v: String = c.query_row("SELECT value_json FROM ai_proposal WHERE field_id = ?1 AND status = 'pending'", [field], |r| r.get(0)).unwrap();
            assert_eq!(v, json!(kept).to_string(), "{said}");
        }
    }

    fn kept(o: ProposeOutcome) -> String {
        match o {
            ProposeOutcome::Kept { id } => id,
            other => panic!("not kept: {other:?}"),
        }
    }

    #[test]
    fn a_proposal_waits_until_a_person_resolves_it_once() {
        let (_d, c) = conn();
        let id = kept(propose(&c, new("institution.populations", json!(["older_adults"]))).unwrap());
        let waiting = pending(&c, "institution.identity", None).unwrap();
        assert_eq!(waiting.len(), 1);
        assert_eq!((waiting[0].value.clone(), waiting[0].origin, waiting[0].status.as_str()), (json!(["older_adults"]), Origin::AiAssumption, "pending"));
        // nothing of the profile changed: a proposal is not a save
        assert!(crate::core::profile::storage::load_current(&c).unwrap().is_none());

        let done = resolve(&c, &id, false, Some("usr_1")).unwrap().unwrap();
        assert_eq!(done.status, "rejected");
        assert!(pending(&c, "institution.identity", None).unwrap().is_empty());
        assert_eq!(resolve(&c, &id, true, Some("usr_1")).unwrap(), None, "resolved once");
        let by: String = c.query_row("SELECT resolved_by FROM ai_proposal WHERE id = ?1", [&id], |r| r.get(0)).unwrap();
        assert_eq!(by, "usr_1");
    }

    #[test]
    fn a_value_that_does_not_fit_its_field_is_refused() {
        let (_d, c) = conn();
        let refused = |p| match propose(&c, p).unwrap() {
            ProposeOutcome::Refused { code } => code,
            other => panic!("{other:?}"),
        };
        assert_eq!(refused(new("institution.populations", json!(["aliens"]))), "code_unknown");
        assert_eq!(refused(new("institution.name", json!(3))), "wrong_type");
        assert_eq!(refused(new("institution.name", json!("  "))), "empty");
        assert_eq!(refused(new("institution.nope", json!("x"))), "field_unknown");
        assert_eq!(refused(NewProposal { form_id: "nope".into(), ..new("institution.name", json!("x")) }), "form_unknown");
        assert!(pending(&c, "institution.identity", None).unwrap().is_empty());
    }

    #[test]
    fn a_text_is_scanned_before_it_is_kept_and_a_newer_proposal_replaces_the_waiting_one() {
        let (_d, c) = conn();
        kept(propose(&c, new("institution.mission", json!("Cuidar personas mayores."))).unwrap());
        kept(propose(&c, new("institution.mission", json!("Cuidar personas mayores. Llamar a maria.lopez@example.com"))).unwrap());
        let waiting = pending(&c, "institution.identity", None).unwrap();
        assert_eq!(waiting.len(), 1, "one proposal per field");
        let text = waiting[0].value.as_str().unwrap();
        assert!(!text.contains("maria.lopez") && text.starts_with("Cuidar personas mayores."), "{text}");
    }

    /// What the AI never sees it cannot propose, whoever signs: there is no exception left for any agent.
    #[test]
    fn nobody_signing_as_the_ai_proposes_what_the_ai_never_sees() {
        let (_d, c) = conn();
        for agent in ["reader", "capturist"] {
            let rfc = NewProposal { form_id: "institution.legal".into(), agent, ..new("institution.legal_rfc", json!("AAA010101AAA")) };
            assert_eq!(propose(&c, rfc).unwrap(), ProposeOutcome::Refused { code: "not_for_the_ai" }, "{agent}");
        }
        assert!(pending(&c, "institution.legal", None).unwrap().is_empty());
    }
}
