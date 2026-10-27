//! The AI platform of SociAI (ADR-003, the AI blueprint §3), as its own crate (A1): the one door every request goes
//! through, the task that carries it, the one executor that invokes capabilities, the approval and the common
//! delivery. It does not think: it keeps the rules. What needs judgment (the intent, which capability, its values,
//! the words) comes from the model inside the steps, with IA-4; what is allowed, the limits and the approval are kept
//! here, by code.
//!
//! It uses the capability contract and the kernel, and no module: it finds the capabilities in the registry, asks the
//! kernel whether an account may (K2), and leaves the life of each task in the kernel's audit store (K9). What may go
//! where is one decision, its policy (ADR-004), with the local detector and the vault of `privacy`. Nothing
//! here knows the screens or Tauri: the app gives the runtime who the account is now, the clock and where to deliver.
//! Neither the kernel nor a module can use this crate: it does not compile.

pub mod dictionary;
mod executor;
pub mod outcome;
pub mod policy;
pub mod request;
pub mod store;
pub mod task;
#[cfg(test)]
mod tests;

use capability::{ContractError, Effect, Fault, Registry, Run};
use kernel::Modules;

pub use dictionary::{output_fields, outputs_outside_dictionary, OutputField};
pub use outcome::Outcome;
pub use request::Request;
pub use store::migrate;

/// Who asks: the account and the organization of the session, never something a screen sends.
pub struct Who<'a> {
    pub account: &'a str,
    pub organization: &'a str,
}

/// The account as it is now, for the kernel to decide on it (K2), read on the connection the runtime works on. The app
/// gives it only while the session is open, not locked and of this same account; otherwise nothing, and nothing is
/// allowed.
pub trait Accounts {
    fn now(&self, conn: &rusqlite::Connection, who: &Who) -> Option<kernel::Account>;
}

pub trait Clock {
    /// Milliseconds since 1970 (UTC).
    fn now_ms(&self) -> i64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
    }
}

/// Where outcomes go besides the answer to the request: the window hears them, so whoever waits for a task does
/// not have to stay on the screen that asked.
pub trait Deliver {
    fn deliver(&self, outcome: &Outcome);
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("audit error: {0}")]
    Audit(#[from] kernel::AuditError),
}

pub struct Runtime<'a> {
    pub registry: &'a Registry,
    pub modules: &'a Modules,
    pub accounts: &'a dyn Accounts,
    pub clock: &'a dyn Clock,
    pub deliver: &'a dyn Deliver,
    /// The provider of models the institution uses, with its conditions (ADR-004 §2); none while there is none.
    pub provider: Option<&'a policy::Provider>,
}

impl Runtime<'_> {
    /// The last outcome of a task of this account, to ask for it later.
    pub fn task(&self, conn: &rusqlite::Connection, who: &Who, id: &str) -> Result<Option<Outcome>, RuntimeError> {
        match store::load_task(conn, id)? {
            Some(t) if t.account == who.account && t.organization == who.organization => Ok(store::last_outcome(conn, id)?),
            _ => Ok(None),
        }
    }
}

/// The capabilities whose permission neither the kernel nor any module defines (ADR-003 A2). The registry does not
/// know the catalog of permissions, so the platform checks it against the kernel: with one, the app does not start.
pub fn unknown_permissions(registry: &Registry, modules: &Modules) -> Vec<ContractError> {
    registry
        .all()
        .filter(|c| modules.permission(c.permission).is_none())
        .map(|c| ContractError { id: c.id, fault: Fault::UnknownPermission })
        .collect()
}

/// The modifications whose `prepare` does not give the kernel's proposal (ERP-9): the platform ties the approval to its
/// fingerprint, so with one the app does not start. The registry sits under the kernel and cannot check it.
pub fn changes_without_proposal(registry: &Registry) -> Vec<ContractError> {
    let proposal = schemars::schema_for!(kernel::Proposal).to_value();
    registry
        .with_effect(Effect::Modify)
        .filter(|c| !matches!(c.run, Run::Change { prepare, .. } if (prepare.output)() == proposal))
        .map(|c| ContractError { id: c.id, fault: Fault::PrepareWithoutProposal })
        .collect()
}
