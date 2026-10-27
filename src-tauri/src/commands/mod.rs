//! Thin Tauri command layer: validate input, call the service, map errors.

pub mod access;
pub mod ai;
pub mod care;
pub mod facilities;
pub mod finance;
pub mod hr;
pub mod institution;
pub mod records;
pub mod onboarding;
pub mod runtime;
pub mod security;

use crate::core::access::service::{CurrentUser, Session};
use crate::core::access::domain::{need_of, Permission};
use crate::error::access_message;
use crate::error::UiError;
use crate::scanner::guard::Decision;
use crate::core::profile::service;
use crate::core::profile::service::{ProfileView, SaveProfileOutcome};
use crate::core::archive::storage::{self as docs, DeleteSummary, DocumentSummary};
use crate::Db;
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
}

#[tauri::command]
pub fn app_info(session: State<'_, Session>) -> AppInfo {
    let _ = guard(&session, "app_info"); // open: the name and version of the app
    AppInfo { name: "SociAI", version: env!("CARGO_PKG_VERSION") }
}

/// What a command may do once it passed its check: who is acting, and whether they may delete for good.
pub struct Gate {
    pub user: Option<CurrentUser>,
}

impl Gate {
    /// Whether a deletion runs now (the person may delete) or becomes a request for the administrator.
    pub fn may_delete(&self) -> bool {
        self.user.as_ref().is_some_and(|u| u.role.can(Permission::Delete))
    }
    pub fn user(&self) -> Result<&CurrentUser, UiError> {
        self.user.as_ref().ok_or_else(|| UiError::new("not_signed_in", access_message("not_signed_in")))
    }
}

/// The shared handle of the database, for the commands that call services which take it whole.
pub(crate) fn shared(db: &State<'_, Db>) -> crate::storage::SharedDb {
    db.0.clone()
}

/// The check every command makes first (ADR-028): a session that is open and not locked, and the permission the
/// command needs (`domain::access::COMMANDS`). A refusal is written in the audit log.
pub fn guard(session: &State<'_, Session>, command: &str) -> Result<Gate, UiError> {
    let need = need_of(command).ok_or_else(UiError::internal)?;
    let (user, locked) = {
        let state = session.state.lock().map_err(|_| UiError::internal())?;
        (state.user.clone(), state.locked)
    };
    match crate::core::access::service::check(user, locked, need) {
        Ok(user) => Ok(Gate { user }),
        Err(code) => {
            if code == "access_denied" {
                if let Some(db) = &session.db {
                    if let Ok(conn) = db.lock() {
                        let _ = crate::audit::record(&conn, crate::audit::AuditKind::AccessDenied, None, None, serde_json::json!({ "command": command }));
                    }
                }
            }
            Err(UiError::new(code, access_message(code)))
        }
    }
}

fn lock<'a>(db: &'a State<'a, Db>) -> Result<std::sync::MutexGuard<'a, rusqlite::Connection>, UiError> {
    db.0.lock().map_err(|_| UiError::internal())
}

#[tauri::command]
pub fn profile_get(session: State<'_, Session>, db: State<Db>) -> Result<Option<ProfileView>, UiError> {
    guard(&session, "profile_get")?;
    let conn = lock(&db)?;
    Ok(service::get_profile(&conn)?)
}

/// The institution at a glance (ADR-033): the figure of each module, how full it is, the balance and what is missing.
#[tauri::command]
pub fn institution_overview(session: State<'_, Session>, db: State<Db>) -> Result<crate::core::overview::InstitutionOverview, UiError> {
    guard(&session, "institution_overview")?;
    let conn = lock(&db)?;
    Ok(crate::core::overview::institution_overview(&conn)?)
}

/// A form of «Mi institución» described as data, with its values and what is missing (ADR-033).
#[tauri::command]
pub fn form_get(session: State<'_, Session>, db: State<Db>, id: String) -> Result<crate::core::profile::forms::FormView, UiError> {
    guard(&session, "form_get")?;
    let conn = lock(&db)?;
    Ok(crate::core::profile::forms::get(&conn, &id)?)
}

#[tauri::command]
pub fn form_save(session: State<'_, Session>, db: State<Db>,
    id: String,
    values: crate::common::forms::Values,
    decision: Option<Decision>,
    used: Option<Vec<String>>,
) -> Result<SaveProfileOutcome, UiError> {
    guard(&session, "form_save")?;
    let mut conn = lock(&db)?;
    // `used`: the suggestions of the AI the person put in their fields (IA4)
    Ok(crate::core::profile::forms::save_using(&mut conn, &id, values, decision, &used.unwrap_or_default())?)
}

/// The person discards suggestions of a window: nothing of the institution changes.
#[tauri::command]
pub fn proposals_reject(session: State<'_, Session>, db: State<Db>, ids: Vec<String>) -> Result<usize, UiError> {
    let gate = guard(&session, "proposals_reject")?;
    let by = gate.user.as_ref().map(|u| u.id.clone());
    let conn = lock(&db)?;
    Ok(crate::core::proposals::reject(&conn, &ids, by.as_deref())?)
}

/// What the manual says of a screen, a form or a field (the «?»): in the program, with no AI and no internet.
#[tauri::command]
pub fn manual_entry(session: State<'_, Session>, id: String) -> Result<Option<crate::core::manual::EntryView>, UiError> {
    guard(&session, "manual_entry")?;
    Ok(crate::core::manual::entry(&id))
}

/// The places of the program and their names (`core::places`): what the buttons «Ir a…» say.
#[tauri::command]
pub fn app_places(session: State<'_, Session>) -> Result<Vec<crate::core::places::Place>, UiError> {
    guard(&session, "app_places")?;
    Ok(crate::core::places::PLACES.clone())
}

/// Searches the manual (ADR-034 §4): it works offline.
#[tauri::command]
pub fn manual_search(session: State<'_, Session>, db: State<Db>, query: String) -> Result<Vec<crate::core::manual::Hit>, UiError> {
    guard(&session, "manual_search")?;
    let conn = lock(&db)?;
    Ok(crate::core::manual::search(&conn, &query, 8)?)
}

/// Looks for a question in the documents of the institution (IA5): by words, with no internet.
#[tauri::command]
pub fn documents_search(session: State<'_, Session>, db: State<'_, Db>, query: String) -> Result<crate::core::archive::search::Found, UiError> {
    use crate::core::archive::search::{search_documents, SNIPPET_CHARS};
    guard(&session, "documents_search")?;
    Ok(search_documents(&db.0, crate::core::archive::storage::Drawer::Record, &query, 8, SNIPPET_CHARS)?)
}

/// The documents of the drawers of the core (ADR-036): today, the Expediente. Each one says which document of the
/// Expediente it is the file of; one of no drawer (from before ADR-036) is not listed.
#[tauri::command]
pub fn documents_list(session: State<'_, Session>, db: State<Db>) -> Result<Vec<DocumentSummary>, UiError> {
    guard(&session, "documents_list")?;
    let conn = lock(&db)?;
    docs::list_drawer(&conn, docs::Drawer::Record).map_err(|e| UiError::from(crate::core::error::ServiceError::from(e)))
}

#[tauri::command]
pub fn document_emergency_delete(session: State<'_, Session>, db: State<Db>, id: String) -> Result<DeleteSummary, UiError> {
    let gate = guard(&session, "document_emergency_delete")?;
    let mut conn = lock(&db)?;
    if !gate.may_delete() {
        // it disappears now; the administrator deletes it for good or brings it back (ADR-028)
        crate::core::access::service::request_deletion(&mut conn, gate.user()?, crate::core::access::domain::DeletionKind::Document, &id)?;
        return Ok(DeleteSummary { chunks: 0, derived_rows: 0 });
    }
    docs::emergency_delete_document(&mut conn, &id).map_err(|e| UiError::from(crate::core::error::ServiceError::from(e)))
}

/// Development only: loads a fictitious profile from `fixtures/`.
#[tauri::command]
pub fn dev_load_fixture(session: State<'_, Session>, db: State<Db>, name: String) -> Result<ProfileView, UiError> {
    guard(&session, "dev_load_fixture")?;
    #[cfg(debug_assertions)]
    {
        let mut conn = lock(&db)?;
        return crate::core::examples::load(&mut conn, &name)?.ok_or_else(|| UiError::new("unknown_fixture", "No existe ese perfil de ejemplo."));
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = (db, name);
        Err(UiError::new("dev_only", "Esta opción solo existe en la versión de desarrollo."))
    }
}
