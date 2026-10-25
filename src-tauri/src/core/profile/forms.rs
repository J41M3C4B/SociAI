//! The windows of «Mi institución» as the screen gets and saves them (ADR-033 §1): the description, the values, what is
//! missing and where each datum comes from. They are the windows of the module Mi institución (ERP-1): its kernel
//! validates a save, empties what no longer applies and keeps the history; the scanner reads the free texts first
//! (D2). The screen never maps a field to the profile itself.

use std::collections::BTreeMap;

use crate::common::forms::{FormSpec, Values};
use crate::core::error::ServiceError;
use crate::core::history::FieldOrigin;
use crate::core::institution::forms::{form, module_id, screen_id};
use crate::core::profile::domain::ProfileInput;
use crate::core::profile::service::{self, SaveProfileOutcome};
use crate::core::profile::storage as store;
use crate::scanner::guard::Decision;
use mi_institucion::windows::{self, Window};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{Map, Value};

/// A form ready to draw: its description, what is saved, where each saved datum comes from and what is still missing.
#[derive(Debug, Serialize)]
pub struct FormView {
    pub spec: &'static FormSpec,
    pub values: Values,
    /// The saved values of other windows its conditions and words look at (the ways it serves, for the capacity):
    /// the screen reads them, it does not save them.
    pub context: Values,
    pub origins: BTreeMap<String, FieldOrigin>,
    pub missing: Vec<&'static str>,
    /// The suggestions waiting in this form (IA4), but the ones that say what is already saved.
    pub proposals: Vec<crate::core::proposals::Proposal>,
}

/// Every datum of the profile, by the id the screen gives it.
pub fn catalog_values(p: &ProfileInput) -> Values {
    store::values_of(p).into_iter().map(|(k, v)| (screen_id(&k), v)).collect()
}

/// The window of the module behind a form of the screen.
fn window(id: &str) -> Result<&'static Window, ServiceError> {
    id.strip_prefix(crate::core::institution::forms::PREFIX).and_then(windows::window).ok_or(ServiceError::NotFound)
}

/// The saved data of the profile, by the module's ids.
fn saved(conn: &Connection) -> Result<Map<String, Value>, ServiceError> {
    Ok(mi_institucion::service::profile(conn)?.values)
}

/// The saved values of the fields of other windows a form looks at (`FormSpec::looks_outside`).
fn context_of(spec: &FormSpec, values: &Map<String, Value>) -> Values {
    spec.looks_outside()
        .into_iter()
        .filter_map(|id| Some((id.to_string(), values.get(module_id(id)?)?.clone())))
        .collect()
}

/// A form's values with the context its conditions look at: what decides whether each field applies. The context
/// holds only fields of other windows, and what is saved there wins over whatever came with the form for them.
pub fn with_context(values: &Values, context: &Values) -> Values {
    let mut all = values.clone();
    all.extend(context.iter().map(|(k, v)| (k.clone(), v.clone())));
    all
}

/// Where a datum comes from, as the screen tells it: a person, a document and its page, a task of the platform the
/// person accepted, or a calculation.
fn origin_of(source: &mi_institucion::service::Source) -> FieldOrigin {
    let (origin, source_ref) = match &source.origin {
        kernel::Origin::Person { .. } => ("user", None),
        kernel::Origin::Document { document, page, .. } => ("document", Some(serde_json::json!({ "document_id": document, "page": page }).to_string())),
        kernel::Origin::Task { .. } => ("ai_assumption", None),
        kernel::Origin::Calculated { .. } => ("computed", None),
    };
    FieldOrigin { origin: origin.into(), source_ref, confirmed_at: Some(source.at.clone()) }
}

pub fn get(conn: &Connection, id: &str) -> Result<FormView, ServiceError> {
    let spec = form(id).ok_or(ServiceError::NotFound)?;
    let w = window(id)?;
    let all = saved(conn)?;
    let values: Values = all.iter().filter(|(k, _)| w.has(k)).map(|(k, v)| (screen_id(k), v.clone())).collect();
    let origins = mi_institucion::service::origins(conn)?
        .iter()
        .filter(|(k, _)| w.has(k) && all.contains_key(*k))
        .map(|(k, s)| (screen_id(k), origin_of(s)))
        .collect();
    // what a document suggests shows only while its field is empty (ADR-038); any other suggestion, while it differs
    let proposals = crate::core::proposals::pending(conn, id, None)?
        .into_iter()
        .filter(|p| match p.origin {
            crate::core::proposals::Origin::Document => !crate::common::forms::is_filled(values.get(&p.field_id)),
            crate::core::proposals::Origin::AiAssumption => values.get(&p.field_id) != Some(&p.value),
        })
        .collect();
    // a quick figure the records of a module already answer is not missing (ADR-031 §3: the records win)
    let answered = crate::core::onboarding::service::answered(conn)?;
    let missing = mi_institucion::catalog::PROFILE
        .missing(&all)
        .into_iter()
        .filter(|f| w.has(f))
        .filter_map(|f| spec.field(&screen_id(f)).map(|s| s.id))
        .filter(|f| !answered.contains(f))
        .collect();
    Ok(FormView { spec, context: context_of(spec, &all), values, origins, missing, proposals })
}

/// The values of a form as saved now, by field id: what a suggestion must not propose again.
pub fn saved_values(conn: &Connection, id: &str) -> Result<Values, ServiceError> {
    Ok(get(conn, id)?.values)
}

/// The saved values of other windows a form looks at, by field id (`FormView::context`).
pub fn context(conn: &Connection, id: &str) -> Result<Values, ServiceError> {
    let spec = form(id).ok_or(ServiceError::NotFound)?;
    Ok(context_of(spec, &saved(conn)?))
}

/// Saves a form. A value out of its list or range is refused with the field it belongs to; what is missing never
/// stops the save.
pub fn save(conn: &mut Connection, id: &str, values: Values, decision: Option<Decision>) -> Result<SaveProfileOutcome, ServiceError> {
    save_using(conn, id, values, decision, &[])
}

/// The document and page a suggestion was read from (`source_ref` of the reader).
fn read_from(p: &crate::core::proposals::Proposal) -> Option<(String, Option<i64>)> {
    let source: Value = serde_json::from_str(p.source_ref.as_deref()?).ok()?;
    Some((source.get("document_id")?.as_str()?.to_string(), source.get("page").and_then(Value::as_i64)))
}

/// As `save`, with the suggestions the person put in their fields (`used`, by id; IA4). The kernel keeps one origin per
/// change: the data taken as a document said them are saved first, document by document and page by page, with the
/// document as their origin; then the rest of the window, as the person's. A suggestion taken is marked accepted.
pub fn save_using(conn: &mut Connection, id: &str, values: Values, decision: Option<Decision>, used: &[String]) -> Result<SaveProfileOutcome, ServiceError> {
    form(id).ok_or(ServiceError::NotFound)?;
    let w = window(id)?;
    // only the fields of this window: what it sends of another one does not decide
    let mut sent: Map<String, Value> = values.iter().filter_map(|(k, v)| Some((module_id(k)?, v)).filter(|(f, _)| w.has(f))).map(|(f, v)| (f.to_string(), v.clone())).collect();
    if let Some(report) = service::scan(conn, &mut sent, decision)? {
        return Ok(SaveProfileOutcome::Quarantine { report });
    }
    let person = service::origin(conn)?;
    // what the whole save would do, before anything is written
    let input = mi_institucion::contract::WindowInput { window: w.id.into(), values: sent.clone() };
    let proposal = mi_institucion::service::prepare(conn, &input)?;
    if !proposal.invalid.is_empty() {
        return Ok(SaveProfileOutcome::Invalid { issues: proposal.invalid.iter().map(|i| service::issue(&i.field, &i.reason)).collect() });
    }
    let taken = crate::core::proposals::waiting(conn, id, used)?;
    let mut by_document: BTreeMap<(String, Option<i64>), Vec<String>> = BTreeMap::new();
    for p in taken.iter().filter(|p| p.origin == crate::core::proposals::Origin::Document) {
        let Some(field) = module_id(&p.field_id) else { continue };
        if let (Some(read), true) = (read_from(p), sent.get(field) == Some(&p.value)) {
            by_document.entry(read).or_default().push(field.to_string());
        }
    }
    if let kernel::Origin::Person { account } = &person {
        for ((document, page), fields) in by_document {
            let mut now: Map<String, Value> = saved(conn)?.into_iter().filter(|(k, _)| w.has(k)).collect();
            for f in &fields {
                now.insert(f.clone(), sent[f].clone());
            }
            let origin = kernel::Origin::Document { document, page, account: account.clone() };
            if let Err(e) = store::save_window(conn, w.id, now, &origin) {
                return service::invalid(e);
            }
        }
    }
    if let Err(e) = store::save_window(conn, w.id, sent, &person) {
        return service::invalid(e);
    }
    // with the save, what documents suggested for the fields now written is set aside (ADR-038)
    let written = saved_values(conn, id)?;
    let ids: Vec<String> = taken.into_iter().map(|p| p.id).collect();
    crate::core::proposals::accept_in(conn, &ids)?;
    crate::core::proposals::discard_filled(conn, id, None, &written)?;
    crate::core::onboarding::service::after_save(conn)?;
    let profile = service::get_profile(conn)?.ok_or(crate::storage::StorageError::NoProfile)?;
    Ok(SaveProfileOutcome::Saved { profile })
}

#[cfg(test)]
#[path = "forms_tests.rs"]
mod tests;
