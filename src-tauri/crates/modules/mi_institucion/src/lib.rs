//! Mi institución: the profile of the institution, the first module with the shape of the ERP blueprint (§4). Its
//! manifest is what the registry reads; its contract (`mi_institucion-contract`) is the only part of it another module
//! may use; its tables start with `mi_institucion_`, and only it writes SQL on them.
//!
//! The profile is one entity of the dictionary (`catalog`), shown in four windows (`windows`). Saving a window is
//! prepared and applied on the kernel, the same for a person and for a task of the platform; the kernel validates it,
//! calculates what is missing and keeps its history. It publishes what is missing, a window and saving a window as
//! capabilities. It does not know the AI exists: it does not depend on the app nor on the platform.

pub mod capabilities;
pub mod catalog;
pub mod service;
pub mod storage;
pub mod windows;

pub use mi_institucion_contract as contract;

use kernel::{Manifest, Migration, Permission};

kernel::module! {
    static MANIFEST = Manifest {
        id: "mi_institucion",
        name: "Mi institución",
        version: env!("CARGO_PKG_VERSION"),
        depends_on: &[],
        permissions: &[
            Permission { id: "mi_institucion.perfil.leer", description: "Ver el perfil de la institución." },
            Permission { id: "mi_institucion.perfil.escribir", description: "Cambiar el perfil de la institución." },
        ],
        migrations: &[Migration { version: 1, sql: include_str!("../migrations/0001_profile.sql") }],
        entities: &[catalog::PROFILE],
        on: true,
    };
}
