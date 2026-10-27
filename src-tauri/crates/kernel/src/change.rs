//! A change of data, prepared and then applied (the ERP blueprint §7), for any entity of any module. A person who saves
//! a form and a task of the platform that proposes values go the same way:
//!
//! 1. **prepare**: the module reads the entity as it is and the kernel turns that into a `Proposal` with the entity's
//!    dictionary (K3): the differences field by field, which of them are protected, what does not pass the
//!    validation, the base version and the fingerprint. Nothing is written.
//! 2. **approve**, when the contract asks for it: the approval is tied to the fingerprint.
//! 3. **apply**: in one transaction (K4) the kernel prepares again, and applies only if the fingerprint is still the
//!    approved one; then the module writes its datum and raises its version, and the kernel writes the record of the
//!    change, the history with its origin (K5) and the fact (K8). Or nothing at all.
//!
//! The fingerprint comes from the entity, its base version and the input: if the datum or the input changed after the
//! approval, it no longer matches, and the change is prepared and approved again.

use std::fmt;

use rusqlite::{params, Connection, OptionalExtension};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::audit::{is_code, is_id};
use crate::dictionary::{Entity, Field};

/// Any entity of any module: `muestra` · `note` · its id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Ref {
    pub module: String,
    pub entity: String,
    pub id: String,
}

impl Ref {
    pub fn new(module: &str, entity: &str, id: &str) -> Ref {
        Ref { module: module.into(), entity: entity.into(), id: id.into() }
    }
}

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}#{}", self.module, self.entity, self.id)
    }
}

/// A new id: the prefix, `_` and a ULID. Whoever asks for a new entity makes its id first and puts it in the input,
/// so preparing and applying see the same entity, and the same creation repeated does not make two.
pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", ulid::Ulid::generate())
}

/// The fingerprint of a change: SHA-256, in hexadecimal, of the entity, its base version and the input written as
/// canonical JSON (keys in order). Version 0 is an entity that does not exist yet.
pub fn fingerprint(entity: &Ref, version: i64, input: &Value) -> String {
    let mut text = format!("kernel.v1\n{entity}@{version}\n");
    canonical(input, &mut text);
    hex::encode(Sha256::digest(text.as_bytes()))
}

fn canonical(v: &Value, out: &mut String) {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(k.clone()).to_string());
                out.push(':');
                canonical(&map[k], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// One field that changes. A protected one carries its values here, for the person who approves; the history keeps
/// only that it changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FieldChange {
    pub field: String,
    pub before: Value,
    pub after: Value,
    pub protected: bool,
}

/// A field that does not pass the validation, and the code of why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Invalid {
    pub field: String,
    pub reason: String,
}

impl Invalid {
    /// The reason is a code written in the program (`required`, `too_long`), never text of a person.
    pub fn new(field: &str, reason: &'static str) -> Invalid {
        Invalid { field: field.into(), reason: reason.into() }
    }
}

/// What a change would do, before doing it. It is the output of `prepare`, and what a person approves: the platform
/// checks it against its schema and ties the approval to its fingerprint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Proposal {
    pub entity: Ref,
    /// The version it was prepared against; 0 for a new entity.
    pub base_version: i64,
    pub changes: Vec<FieldChange>,
    pub invalid: Vec<Invalid>,
    pub fingerprint: String,
}

/// What a module says to prepare a change: the entity, its version and its fields as they are (none if it is new),
/// its fields as they would be, what breaks its own rules beyond the dictionary, and the input it came from.
pub struct Draft<'a> {
    pub entity: Ref,
    pub version: i64,
    pub before: Option<Map<String, Value>>,
    pub after: Map<String, Value>,
    /// What only the module can check: a name that is repeated, a sum that has to match another. The dictionary
    /// checks the rest.
    pub invalid: Vec<Invalid>,
    pub input: &'a Value,
}

/// The proposal of a draft, with the entity's dictionary (the one the module declares in its manifest): only the
/// fields that change, in the order of their names. A field that does not apply goes empty, and the module writes
/// what the proposal says. What does not pass is the dictionary's validation and then the module's own; a field is
/// protected by its class, and one the dictionary does not have is protected too.
pub fn propose(dictionary: &Entity, draft: Draft) -> Proposal {
    let before = draft.before.unwrap_or_default();
    let mut after = draft.after;
    dictionary.clear_hidden(&mut after);
    let mut invalid = dictionary.validate(&after);
    invalid.extend(draft.invalid);
    let mut fields: Vec<&String> = before.keys().chain(after.keys()).collect();
    fields.sort();
    fields.dedup();
    let changes = fields
        .into_iter()
        .filter_map(|f| {
            let (b, a) = (before.get(f).cloned().unwrap_or(Value::Null), after.get(f).cloned().unwrap_or(Value::Null));
            let protected = dictionary.field(f).is_none_or(Field::protected);
            (b != a).then(|| FieldChange { field: f.clone(), before: b, after: a, protected })
        })
        .collect();
    let fingerprint = fingerprint(&draft.entity, draft.version, draft.input);
    Proposal { entity: draft.entity, base_version: draft.version, changes, invalid, fingerprint }
}

/// Where a change comes from: the evidence the person sees in the history. Only ids and codes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Origin {
    /// The person wrote it.
    Person { account: String },
    /// It was read from a document, on this page.
    Document { document: String, page: Option<i64>, account: String },
    /// A rule calculated it.
    Calculated { rule: String },
    /// A task of the platform proposed it, and this account accepted it.
    Task { task: String, account: String },
}

impl Origin {
    fn kind(&self) -> &'static str {
        match self {
            Origin::Person { .. } => "person",
            Origin::Document { .. } => "document",
            Origin::Calculated { .. } => "calculated",
            Origin::Task { .. } => "task",
        }
    }

    /// Its ids have the shape of an id and its rule is a code: no data of a person enters the history this way.
    fn well_formed(&self) -> bool {
        match self {
            Origin::Person { account } => is_id(account),
            Origin::Document { document, page, account } => is_id(document) && is_id(account) && page.is_none_or(|p| p > 0),
            Origin::Calculated { rule } => is_code(rule),
            Origin::Task { task, account } => is_id(task) && is_id(account),
        }
    }
}

/// What a module gives the kernel to prepare and apply one change of one entity. The kernel calls it inside its
/// transaction.
pub trait Changeable {
    /// The proposal against the entity as it is now. Writes nothing.
    fn prepare(&self, conn: &Connection) -> rusqlite::Result<Proposal>;
    /// The version of the entity now; 0 if it does not exist.
    fn version(&self, conn: &Connection) -> rusqlite::Result<i64>;
    /// Writes the change and raises the version by one.
    fn write(&self, conn: &Connection, proposal: &Proposal) -> rusqlite::Result<()>;
}

/// A change applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Applied {
    /// The id of the record of the change.
    pub change: String,
    pub entity: Ref,
    /// The version the entity has after it.
    pub version: i64,
    /// It had been applied before, and nothing was done this time: an `apply` repeated after an answer that did not
    /// arrive. The change is made all the same, so it is a success.
    pub already_applied: bool,
}

#[derive(Debug)]
pub enum ApplyError {
    /// The datum or the input changed since it was prepared: prepare and approve again.
    Stale,
    /// Something does not pass the validation.
    Invalid(Vec<Invalid>),
    /// The entity is already like that.
    NothingToChange,
    /// The origin carries something that is not an id or a code.
    BadOrigin,
    /// The fact is not a code.
    BadFact,
    /// The module did not leave the entity at the next version.
    VersionNotRaised,
    /// The entity has a history already: an earlier one cannot be taken in under it (`adopt`).
    HasHistory,
    Db(rusqlite::Error),
}

impl ApplyError {
    /// The code, for the platform and the screens.
    pub fn code(&self) -> &'static str {
        match self {
            ApplyError::Stale => "prepare_again",
            ApplyError::Invalid(_) => "invalid",
            ApplyError::NothingToChange => "nothing_to_change",
            ApplyError::BadOrigin => "bad_origin",
            ApplyError::BadFact => "bad_fact",
            ApplyError::VersionNotRaised => "version_not_raised",
            ApplyError::HasHistory => "has_history",
            ApplyError::Db(_) => "database",
        }
    }
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApplyError::Db(e) => write!(f, "database error: {e}"),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for ApplyError {}

impl From<rusqlite::Error> for ApplyError {
    fn from(e: rusqlite::Error) -> Self {
        ApplyError::Db(e)
    }
}

/// Applies a change in one transaction (K4). `approved` is the fingerprint a person approved, when the change asked
/// for an approval; without it the change is applied as it is prepared now. If that fingerprint was applied already,
/// nothing is done again and the earlier change is the answer. `fact` is the code of what happened, for whoever
/// reacts to it (`muestra.nota_guardada`).
pub fn apply(conn: &mut Connection, what: &impl Changeable, approved: Option<&str>, origin: &Origin, fact: &'static str) -> Result<Applied, ApplyError> {
    if !origin.well_formed() {
        return Err(ApplyError::BadOrigin);
    }
    if !is_code(fact) {
        return Err(ApplyError::BadFact);
    }
    let tx = conn.transaction()?;
    if let Some(done) = approved.map(|fp| applied(&tx, fp)).transpose()?.flatten() {
        return Ok(done);
    }
    let proposal = what.prepare(&tx)?;
    if approved.is_some_and(|fp| fp != proposal.fingerprint) {
        return Err(ApplyError::Stale);
    }
    if !proposal.invalid.is_empty() {
        return Err(ApplyError::Invalid(proposal.invalid));
    }
    if proposal.changes.is_empty() {
        return Err(ApplyError::NothingToChange);
    }
    what.write(&tx, &proposal)?;
    let version = proposal.base_version + 1;
    if what.version(&tx)? != version {
        return Err(ApplyError::VersionNotRaised);
    }
    let change = new_id("chg");
    let e = &proposal.entity;
    let (account, document, page, rule, task) = match origin {
        Origin::Person { account } => (Some(account), None, None, None, None),
        Origin::Document { document, page, account } => (Some(account), Some(document), *page, None, None),
        Origin::Calculated { rule } => (None, None, None, Some(rule), None),
        Origin::Task { task, account } => (Some(account), None, None, None, Some(task)),
    };
    tx.execute(
        "INSERT INTO kernel_change (id, fingerprint, module, entity, entity_id, base_version, version, origin, account_id,
                                    document_id, page, rule, task_id, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        params![change, proposal.fingerprint, e.module, e.entity, e.id, proposal.base_version, version, origin.kind(), account, document, page, rule, task],
    )?;
    for c in &proposal.changes {
        let (before, after) = if c.protected { (None, None) } else { (Some(c.before.to_string()), Some(c.after.to_string())) };
        tx.execute(
            "INSERT INTO kernel_history (change_id, field, before, after, protected) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![change, c.field, before, after, c.protected],
        )?;
    }
    tx.execute(
        "INSERT INTO kernel_outbox (change_id, fact, module, entity, entity_id, at)
         VALUES (?1, ?2, ?3, ?4, ?5, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        params![change, fact, e.module, e.entity, e.id],
    )?;
    tx.commit()?;
    Ok(Applied { change, entity: proposal.entity, version, already_applied: false })
}

/// Whether this fingerprint was applied, and what it did (with `already_applied`): so an `apply` that did not answer
/// is not repeated blindly.
pub fn applied(conn: &Connection, fingerprint: &str) -> rusqlite::Result<Option<Applied>> {
    conn.query_row(
        "SELECT id, module, entity, entity_id, version FROM kernel_change WHERE fingerprint = ?1",
        [fingerprint],
        |r| {
            Ok(Applied {
                change: r.get(0)?,
                entity: Ref { module: r.get(1)?, entity: r.get(2)?, id: r.get(3)? },
                version: r.get(4)?,
                already_applied: true,
            })
        },
    )
    .optional()
}

/// One change in the history of an entity.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Record {
    pub change: String,
    pub version: i64,
    pub at: String,
    pub origin: Origin,
    /// A protected field comes with `null` before and after.
    pub fields: Vec<FieldChange>,
}

/// The history of any entity, oldest first.
pub fn history(conn: &Connection, entity: &Ref) -> rusqlite::Result<Vec<Record>> {
    let mut records: Vec<Record> = conn
        .prepare(
            "SELECT id, version, at, origin, account_id, document_id, page, rule, task_id FROM kernel_change
             WHERE module = ?1 AND entity = ?2 AND entity_id = ?3 ORDER BY version",
        )?
        .query_map([&entity.module, &entity.entity, &entity.id], |r| {
            let account = || r.get::<_, Option<String>>(4).map(Option::unwrap_or_default);
            let origin = match r.get::<_, String>(3)?.as_str() {
                "person" => Origin::Person { account: account()? },
                "document" => Origin::Document { document: r.get::<_, Option<String>>(5)?.unwrap_or_default(), page: r.get(6)?, account: account()? },
                "calculated" => Origin::Calculated { rule: r.get::<_, Option<String>>(7)?.unwrap_or_default() },
                _ => Origin::Task { task: r.get::<_, Option<String>>(8)?.unwrap_or_default(), account: account()? },
            };
            Ok(Record { change: r.get(0)?, version: r.get(1)?, at: r.get(2)?, origin, fields: Vec::new() })
        })?
        .collect::<Result<_, _>>()?;
    let mut fields = conn.prepare("SELECT field, before, after, protected FROM kernel_history WHERE change_id = ?1 ORDER BY id")?;
    let json = |s: Option<String>| s.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
    for record in &mut records {
        record.fields = fields
            .query_map([&record.change], |r| {
                Ok(FieldChange { field: r.get(0)?, before: json(r.get(1)?), after: json(r.get(2)?), protected: r.get(3)? })
            })?
            .collect::<Result<_, _>>()?;
    }
    Ok(records)
}

/// A change from before the kernel kept the history, as the app kept it: when it happened, where it came from and
/// what it did to each field.
#[derive(Debug, Clone, PartialEq)]
pub struct Earlier {
    /// `YYYY-MM-DDTHH:MM:SSZ`.
    pub at: String,
    pub origin: Origin,
    pub fields: Vec<FieldChange>,
}

/// Takes in the history an entity had before the kernel (K5), oldest first, when a module takes over data the app
/// kept. Each earlier change becomes a change of the kernel at the next version, with its fields and its origin, and
/// no fact: it already happened. A field is protected by its class, as in `propose`, whatever the old history said.
/// The old history did not always keep who acted or the document: an id it does not have, or that is not an id, goes
/// empty. Only for an entity with no history yet, inside the transaction where the module writes the entity at the
/// version this returns.
pub fn adopt(conn: &Connection, dictionary: &Entity, entity: &Ref, earlier: &[Earlier]) -> Result<i64, ApplyError> {
    let kept: i64 = conn.query_row(
        "SELECT count(*) FROM kernel_change WHERE module = ?1 AND entity = ?2 AND entity_id = ?3",
        [&entity.module, &entity.entity, &entity.id],
        |r| r.get(0),
    )?;
    if kept > 0 {
        return Err(ApplyError::HasHistory);
    }
    let id = |s: &String| Some(s.clone()).filter(|s| is_id(s));
    let mut version = 0;
    for e in earlier {
        version += 1;
        let (account, document, page, rule, task) = match &e.origin {
            Origin::Person { account } => (id(account), None, None, None, None),
            Origin::Document { document, page, account } => (id(account), id(document), *page, None, None),
            Origin::Calculated { rule } if is_code(rule) => (None, None, None, Some(rule.clone()), None),
            Origin::Calculated { .. } => return Err(ApplyError::BadOrigin),
            Origin::Task { task, account } => (id(account), None, None, None, id(task)),
        };
        let change = new_id("chg");
        conn.execute(
            "INSERT INTO kernel_change (id, fingerprint, module, entity, entity_id, base_version, version, origin, account_id,
                                        document_id, page, rule, task_id, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                change,
                fingerprint(entity, version, &serde_json::json!({ "adopted": e.at })),
                entity.module,
                entity.entity,
                entity.id,
                version - 1,
                version,
                e.origin.kind(),
                account,
                document,
                page,
                rule,
                task,
                e.at
            ],
        )?;
        for c in &e.fields {
            let protected = dictionary.field(&c.field).is_none_or(Field::protected);
            let (before, after) = if protected { (None, None) } else { (Some(c.before.to_string()), Some(c.after.to_string())) };
            conn.execute(
                "INSERT INTO kernel_history (change_id, field, before, after, protected) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![change, c.field, before, after, protected],
            )?;
        }
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::{Condition, Kind};
    use serde_json::json;

    const NOTE: Entity = Entity {
        id: "note",
        fields: &[
            Field::new("title", Kind::Text, "internal").mandatory().max_len(120),
            Field::new("body", Kind::LongText, "internal"),
            Field::new("phone", Kind::Phone, "personal"),
            Field::new("urgent", Kind::YesNo, "internal").when(Condition::Filled { field: "phone" }),
        ],
    };

    fn map(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn the_same_change_has_the_same_fingerprint_whatever_the_order_of_its_keys() {
        let note = Ref::new("muestra", "note", "nota_1");
        let a = fingerprint(&note, 3, &json!({ "title": "Hola", "body": { "x": 1, "y": [1, 2] } }));
        let b = fingerprint(&note, 3, &json!({ "body": { "y": [1, 2], "x": 1 }, "title": "Hola" }));
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert_ne!(a, fingerprint(&note, 4, &json!({ "title": "Hola", "body": { "x": 1, "y": [1, 2] } })), "another version");
        assert_ne!(a, fingerprint(&note, 3, &json!({ "title": "Hola!", "body": { "x": 1, "y": [1, 2] } })), "another input");
        assert_ne!(a, fingerprint(&Ref::new("muestra", "note", "nota_2"), 3, &json!({ "title": "Hola", "body": { "x": 1, "y": [1, 2] } })));
    }

    #[test]
    fn a_proposal_has_only_what_changes() {
        let input = json!({ "title": "Nuevo" });
        let p = propose(
            &NOTE,
            Draft {
                entity: Ref::new("muestra", "note", "nota_1"),
                version: 2,
                before: Some(map(json!({ "title": "Viejo", "body": "igual", "phone": "55 1234 5678" }))),
                after: map(json!({ "title": "Nuevo", "body": "igual", "phone": "55 0000 0000" })),
                invalid: vec![],
                input: &input,
            },
        );
        assert_eq!(
            p.changes,
            [
                FieldChange { field: "phone".into(), before: json!("55 1234 5678"), after: json!("55 0000 0000"), protected: true },
                FieldChange { field: "title".into(), before: json!("Viejo"), after: json!("Nuevo"), protected: false },
            ]
        );
        assert_eq!((p.base_version, p.fingerprint.as_str()), (2, fingerprint(&p.entity, 2, &input).as_str()));
        // a new entity: everything comes from nothing
        let new = propose(
            &NOTE,
            Draft { entity: p.entity.clone(), version: 0, before: None, after: map(json!({ "title": "Nuevo" })), invalid: vec![], input: &input },
        );
        assert_eq!(new.changes, [FieldChange { field: "title".into(), before: Value::Null, after: json!("Nuevo"), protected: false }]);
    }

    /// The module does not repeat what the dictionary says: the validation, what is protected and what is not kept.
    #[test]
    fn a_proposal_takes_its_validation_and_its_protection_from_the_dictionary() {
        let input = json!({});
        let draft = |after: Value, invalid: Vec<Invalid>| Draft {
            entity: Ref::new("muestra", "note", "nota_1"),
            version: 1,
            before: Some(map(json!({ "title": "Viejo", "phone": "55 1234 5678", "urgent": true }))),
            after: map(after),
            invalid,
            input: &input,
        };
        let p = propose(&NOTE, draft(json!({ "title": " ", "phone": "llámale a Rosa", "urgent": true, "color": "rojo" }), vec![Invalid::new("title", "repeated")]));
        let reasons: Vec<(&str, &str)> = p.invalid.iter().map(|i| (i.field.as_str(), i.reason.as_str())).collect();
        assert_eq!(reasons, [("title", "required"), ("phone", "not_a_phone"), ("color", "unknown_field"), ("title", "repeated")]);
        let protected: Vec<(&str, bool)> = p.changes.iter().map(|c| (c.field.as_str(), c.protected)).collect();
        assert_eq!(protected, [("color", true), ("phone", true), ("title", false)], "a field the dictionary does not have is protected");
        // without a phone, `urgent` no longer applies: it is not kept
        let p = propose(&NOTE, draft(json!({ "title": "Viejo", "phone": null, "urgent": true }), vec![]));
        let changed: Vec<(&str, Value)> = p.changes.iter().map(|c| (c.field.as_str(), c.after.clone())).collect();
        assert_eq!(changed, [("phone", Value::Null), ("urgent", Value::Null)]);
        assert!(p.invalid.is_empty());
    }

    /// The platform checks the output of `prepare` against the schema of the proposal and reads the fingerprint from it.
    #[test]
    fn a_proposal_has_a_schema_and_goes_and_comes_back_as_json() {
        let input = json!({ "title": "Nuevo" });
        let p = propose(
            &NOTE,
            Draft {
                entity: Ref::new("muestra", "note", "nota_1"),
                version: 1,
                before: Some(map(json!({ "title": "Viejo" }))),
                after: map(json!({ "title": "Nuevo" })),
                invalid: vec![Invalid::new("title", "too_long")],
                input: &input,
            },
        );
        let back: Proposal = serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        let schema = serde_json::to_value(schemars::schema_for!(Proposal)).unwrap();
        let required: Vec<&str> = schema["required"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
        for field in ["entity", "base_version", "changes", "invalid", "fingerprint"] {
            assert!(required.contains(&field), "{field}: {schema}");
        }
    }

    #[test]
    fn an_origin_carries_only_ids_and_codes() {
        let account = "usr_01JB3M7Q8ZK4X2N6V9R5T0W1YD".to_string();
        assert!(Origin::Person { account: account.clone() }.well_formed());
        assert!(Origin::Task { task: "task_01JB3M7Q8ZK4X2N6V9R5T0W1YC".into(), account: account.clone() }.well_formed());
        assert!(Origin::Calculated { rule: "muestra.titulo_por_defecto".into() }.well_formed());
        assert!(!Origin::Person { account: "Rosa Hernández".into() }.well_formed());
        assert!(!Origin::Calculated { rule: "lo calculó Rosa".into() }.well_formed());
        assert!(!Origin::Document { document: "doc_01JB3M7Q8ZK4X2N6V9R5T0W1YE".into(), page: Some(0), account }.well_formed());
    }

    /// The history from before the kernel is taken in once, at versions 1, 2…, with its origins and the protection
    /// of the dictionary; what the old history did not keep goes empty.
    #[test]
    fn an_earlier_history_is_taken_in_once_under_the_dictionary() {
        let conn = {
            let mut conn = Connection::open_in_memory().unwrap();
            crate::migrate(&mut conn, &crate::Modules::of([]).unwrap()).unwrap();
            conn
        };
        let note = Ref::new("muestra", "note", "nota_1");
        let change = |field: &str, before: Value, after: Value| FieldChange { field: field.into(), before, after, protected: false };
        let earlier = [
            Earlier {
                at: "2026-03-01T10:00:00Z".into(),
                origin: Origin::Person { account: String::new() },
                fields: vec![change("title", Value::Null, json!("Primera")), change("phone", Value::Null, json!("55 1234 5678"))],
            },
            Earlier {
                at: "2026-04-02T11:00:00Z".into(),
                origin: Origin::Document { document: "doc_01JB3M7Q8ZK4X2N6V9R5T0W1YE".into(), page: Some(3), account: "usr_01JB3M7Q8ZK4X2N6V9R5T0W1YD".into() },
                fields: vec![change("title", json!("Primera"), json!("Segunda"))],
            },
        ];
        assert_eq!(adopt(&conn, &NOTE, &note, &earlier).unwrap(), 2);
        let kept = history(&conn, &note).unwrap();
        assert_eq!(kept.iter().map(|r| (r.version, r.at.as_str())).collect::<Vec<_>>(), [(1, "2026-03-01T10:00:00Z"), (2, "2026-04-02T11:00:00Z")]);
        assert_eq!(kept[0].origin, Origin::Person { account: String::new() });
        assert_eq!(kept[0].fields[1], FieldChange { field: "phone".into(), before: Value::Null, after: Value::Null, protected: true });
        assert_eq!(kept[1].fields, [change("title", json!("Primera"), json!("Segunda"))]);
        let facts: i64 = conn.query_row("SELECT count(*) FROM kernel_outbox", [], |r| r.get(0)).unwrap();
        assert_eq!(facts, 0, "what already happened raises no fact");
        assert!(matches!(adopt(&conn, &NOTE, &note, &earlier), Err(ApplyError::HasHistory)));
        let bad = Earlier { origin: Origin::Calculated { rule: "lo calculó Rosa".into() }, ..earlier[0].clone() };
        assert!(matches!(adopt(&conn, &NOTE, &Ref::new("muestra", "note", "nota_2"), &[bad]), Err(ApplyError::BadOrigin)));
    }
}
