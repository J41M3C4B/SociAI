//! The entry to the runtime of the AI platform (ADR-003, D6): the screens and the chat send every request here. A
//! session is enough to come in; the kernel decides inside, per capability, whether the account may, and again before
//! the outcome is handed over (K2). A text of the chat answers «thinking» at once, and the rest of its turn goes on
//! apart (IA-4): its outcome reaches the window as `task-update`.

use rusqlite::Connection;
use tauri::{AppHandle, Emitter, Manager, State};

use super::{guard, lock};
use crate::ai::platform_models::ProviderModels;
use crate::core::access::service::Session;
use crate::error::UiError;
use crate::Db;
use capability::Registry;
use kernel::Modules;
use platform::request::Channel;
use platform::task::TaskState;
use platform::{Accounts, Answer, Deliver, Models, Outcome, Request, Runtime, SystemClock, Who};

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

/// How the chat answers a question, as the settings of the AI say (E1 of IA-4).
fn answer(conn: &Connection) -> Answer {
    match crate::ai::settings::load(conn) {
        Ok(s) if !s.chat_answer_in_words => Answer::Data,
        _ => Answer::Words,
    }
}

/// The rest of a turn of the chat, apart from the window: a model may take seconds, and the app goes on meanwhile.
/// What it ends with reaches the window as `task-update`.
fn go_on(app: AppHandle, account: String, organization: String, task: String) {
    tauri::async_runtime::spawn_blocking(move || {
        let session = app.state::<Session>();
        let db = app.state::<Db>();
        let registry = app.state::<Registry>();
        let modules = app.state::<Modules>();
        let accounts = SessionAccounts { session: &session };
        let models = ProviderModels::from_settings(&db.0);
        let answer = db.0.lock().map(|conn| answer(&conn)).unwrap_or_default();
        let deliver = Window(app.clone());
        let runtime = Runtime {
            registry: &registry,
            modules: &modules,
            accounts: &accounts,
            clock: &SystemClock,
            deliver: &deliver,
            models: models.as_ref().map(|m| m as &dyn Models),
            answer,
        };
        // a turn that fails here stays running until its time runs out: the next request closes it by its limit
        let _ = runtime.think(&db.0, &Who { account: &account, organization: &organization }, &task);
    });
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
    // the models first: they read the settings and the keychain on their own
    let models = ProviderModels::from_settings(&db.0);
    let mut conn = lock(&db)?;
    // one database per institution (ADR-002, D4): the organization is the institution of the base
    let organization = crate::core::institution::id(&conn).unwrap_or_default();
    let accounts = SessionAccounts { session: &session };
    let deliver = Window(app.clone());
    let runtime = Runtime {
        registry: &registry,
        modules: &modules,
        accounts: &accounts,
        clock: &SystemClock,
        deliver: &deliver,
        models: models.as_ref().map(|m| m as &dyn Models),
        answer: answer(&conn),
    };
    let outcome = runtime.submit(&mut conn, &Who { account: &user.id, organization: &organization }, request).map_err(|_| UiError::internal())?;
    drop(conn);
    if outcome.state == TaskState::Running && outcome.channel == Channel::Chat {
        go_on(app, user.id.clone(), organization, outcome.task.clone());
    }
    Ok(outcome)
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
    // asking for a task sends nothing to a model
    let runtime = Runtime { registry: &registry, modules: &modules, accounts: &accounts, clock: &SystemClock, deliver: &Window(app), models: None, answer: Answer::default() };
    runtime.task(&conn, &Who { account: &user.id, organization: &organization }, &task).map_err(|_| UiError::internal())
}
