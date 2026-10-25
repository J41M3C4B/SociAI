//! Encrypted database, migrations and repositories.

pub mod backup;
pub mod db;
mod migrations;
pub mod fts;
mod secrets;

#[allow(unused_imports)]
pub use db::open_encrypted;

/// The connection the app shares between its commands and the work that runs in the background.
pub type SharedDb = std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>;
#[allow(unused_imports)]
pub use secrets::get_or_create_db_key;
#[allow(unused_imports)]
pub use secrets::{delete_api_key, get_api_key, set_api_key};

use thiserror::Error;

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum StorageError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("keychain error: {0}")]
    Keychain(#[from] keyring_core::Error),
    #[error("random generator error: {0}")]
    Random(String),
    #[error("the database could not be opened with the given key")]
    WrongKey,
    #[error("the password does not open the backup")]
    WrongPassword,
    #[error("the password is too short")]
    WeakPassword,
    #[error("that file already exists")]
    BackupExists,
    #[error("the institution profile does not exist yet")]
    NoProfile,
    /// The kernel did not apply a change of a module, by its code (`kernel::ApplyError::code`).
    #[error("the change was not applied: {0}")]
    Change(&'static str),
    /// A change of a module does not pass the validation of its dictionary: each field, with the code of why.
    #[error("the data do not pass the validation")]
    Invalid(Vec<kernel::Invalid>),
    #[error("a module does not keep the shape of a module: {0}")]
    Modules(String),
    #[error("audit error: {0}")]
    Audit(#[from] crate::audit::AuditError),
}

/// One repository per aggregate will implement CRUD behind this trait.
#[allow(dead_code)]
pub trait Repository {}
