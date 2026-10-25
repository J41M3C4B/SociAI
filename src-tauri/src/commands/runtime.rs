//! The entry to the runtime of the AI platform (ADR-003, D6): the screens, and later the chat, send every request
//! here. A session is enough to come in; the kernel decides inside, per capability, whether the account may, and
//! again before the outcome is handed over (K2).

use rusqlite::Connection;
use tauri::{AppHandle, Emitter, State};

use super::{guard, lock};
use crate::core::access::service::Session;
use crate::error::UiError;
use crate::Db;
use capability::Registry;
use kernel::Modules;
use platform::{Accounts, Deliver, Outcome, Request, Runtime, SystemClock, Who};

/// The account as it is now, for the kernel: only while the session is open, not locked and of this same account.
struct SessionAccounts<'a> {
    session: &'a Session,
}

impl Accounts for SessionAccounts<'_> {
    fn now(&self, conn: &Connection, who: &Who) -> Option<kernel::Account> {
        let open = self.session.state.lock().ok().is_some_and(|s| !s.locked && s.user.as_ref().is_some_and(|u| u.id == who.account));
        if !open {
            return None;
        }
        crate::core::access::service::account(conn, who.account).ok().flatten()
    }
}

/// Every outcome is also heard by the window, so the person does not have to stay on the screen that asked.
struct Window(AppHandle);

impl Deliver for Window {
    fn deliver(&self, outcome: &Outcome) {
        let _ = self.0.emit("task-update", outcome);
    }
}

#[tauri::command]
pub fn runtime_submit(
    app: AppHandle,
    session: State<'_, Session>,
    db: State<'_, Db>,
    registry: State<'_, Registry>,
    modules: State<'_, Modules>,
    request: Request,
) -> Result<Outcome, UiError> {
    let gate = guard(&session, "runtime_submit")?;
    let user = gate.user()?;
    let mut conn = lock(&db)?;
    // one database per institution (ADR-002, D4): the organization is the institution of the base
    let organization = crate::core::institution::id(&conn).unwrap_or_default();
    let accounts = SessionAccounts { session: &session };
    let runtime = Runtime { registry: &registry, modules: &modules, accounts: &accounts, clock: &SystemClock, deliver: &Window(app) };
    runtime.submit(&mut conn, &Who { account: &user.id, organization: &organization }, request).map_err(|_| UiError::internal())
}

/// The last outcome of a task of the person, to ask for it later.
#[tauri::command]
pub fn runtime_task(
    app: AppHandle,
    session: State<'_, Session>,
    db: State<'_, Db>,
    registry: State<'_, Registry>,
    modules: State<'_, Modules>,
    task: String,
) -> Result<Option<Outcome>, UiError> {
    let gate = guard(&session, "runtime_task")?;
    let user = gate.user()?;
    let conn = lock(&db)?;
    let organization = crate::core::institution::id(&conn).unwrap_or_default();
    let accounts = SessionAccounts { session: &session };
    let runtime = Runtime { registry: &registry, modules: &modules, accounts: &accounts, clock: &SystemClock, deliver: &Window(app) };
    runtime.task(&conn, &Who { account: &user.id, organization: &organization }, &task).map_err(|_| UiError::internal())
}
