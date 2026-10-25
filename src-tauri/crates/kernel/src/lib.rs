//! The kernel of the ERP (ADR-002, the ERP blueprint §5): what every module needs and none owns. It sits under the
//! modules and knows none of them, not even by name; what it needs of a module, the module declares in its manifest.
//!
//! It holds the manifest of a module and the registry of modules (K12); the migrations of each owner of tables: the
//! kernel, every module and the AI platform (K4); the catalog of permissions, the roles and the decision to authorize
//! (K2); the data dictionary, where each module declares its entities and the class of every field (K3); the audit
//! store, where the platform leaves the life of each task (K9); and every change of data, prepared and then applied
//! in one transaction with its history, its origin and its fact (K4, K5 and K8).
//!
//! A module declares itself with `module!`, and the registry finds it when the program is linked, with no list
//! (compiled, not run: a documentation example does not link SQLCipher on Windows):
//!
//! ```no_run
//! kernel::module! {
//!     static NOTES = kernel::Manifest {
//!         id: "notas",
//!         name: "Notas",
//!         version: "0.1.0",
//!         depends_on: &[],
//!         permissions: &[kernel::Permission { id: "notas.escribir", description: "Escribir notas." }],
//!         migrations: &[kernel::Migration { version: 1, sql: "CREATE TABLE notas_note (id TEXT PRIMARY KEY, title TEXT);" }],
//!         entities: &[kernel::Entity {
//!             id: "note",
//!             fields: &[kernel::Field::new("title", kernel::Kind::Text, "internal").mandatory()],
//!         }],
//!         on: true,
//!     };
//! }
//!
//! let modules = kernel::Modules::discover().unwrap();
//! assert!(modules.is_on("notas"));
//! assert_eq!(modules.field("notas.note.title").map(|f| f.class), Some("internal"));
//! ```
//!
//! The kernel cannot use a module: it does not depend on any, so this does not compile.
//!
//! ```compile_fail
//! use muestra as _;
//! ```
//!
//! Nor does the audit take a reason that is not a code of the program: text read while the program runs could carry
//! data of a person.
//!
//! ```compile_fail
//! let typed = String::from("lo que escribió la persona");
//! let event = kernel::TaskEvent {
//!     moment: kernel::TaskMoment::Denied,
//!     task: "task_01JB3M7Q8ZK4X2N6V9R5T0W1YC",
//!     account: None,
//!     capability: None,
//!     outcome: None,
//!     reason: Some(typed.as_str()),
//! };
//! # drop(event);
//! ```

mod access;
mod audit;
mod change;
mod conformance;
mod dictionary;
mod migrate;
mod module;
mod registry;

pub use access::{authorize, classes_seen, role, Account, Denial, Permission, Role, Scope, CLASSES, GENERAL, KEPT_IN_HISTORY, ROLES};
pub use audit::{record_task, AuditError, TaskEvent, TaskMoment};
pub use change::{
    adopt, applied, apply, fingerprint, history, new_id, propose, Applied, ApplyError, Changeable, Draft, Earlier, FieldChange, Invalid,
    Origin, Proposal, Record, Ref,
};
pub use conformance::{conform, Breach};
pub use dictionary::{
    clabe_is_valid, is_date, is_email, is_filled, is_phone, is_place_name, is_postal_code, is_rfc_moral, Condition, Entity, Field, Kind, Presence, Rule,
};
pub use migrate::{migrate, migrate_owner, MIGRATIONS_TABLE};
pub use module::{Manifest, Migration};
pub use registry::{Fault, FieldFault, ManifestError, Modules, MODULES};

// what `module!` writes needs this from here, wherever it is used
#[doc(hidden)]
pub use linkme;

/// Declares the manifest of a module, for the registry to find it when the program is linked.
#[macro_export]
macro_rules! module {
    (static $name:ident = $manifest:expr;) => {
        #[$crate::linkme::distributed_slice($crate::MODULES)]
        #[linkme(crate = $crate::linkme)]
        static $name: $crate::Manifest = $manifest;
    };
}
