//! What the sample module publishes (the ERP blueprint §4, `capabilities`): its capabilities, declared from its own
//! crate with the contract (ADR-001) and with a permission it defines in its manifest (ADR-003 A2). The platform
//! finds them in the registry and invokes them through its executor; the module never knows the platform.
//!
//! Saving a note goes through the kernel like any change (ERP-9): `prepare` gives the kernel's proposal, and `apply`
//! applies only the fingerprint the person approved, with the task as its origin.

use capability::{Call, CapabilityError, Limits};
use kernel::{Applied, Origin, Proposal};

use crate::contract::{NoteInput, NoteRef};
use crate::service;

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct WhichNote {
    /// The id of the note.
    pub id: String,
}

/// A note as the person reads it. Its phone is protected and never leaves this way.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: Option<String>,
    pub version: i64,
}

fn read(call: &Call, which: WhichNote) -> Result<Note, CapabilityError> {
    let note = crate::storage::find(call.conn, &NoteRef(which.id.clone()))
        .map_err(|_| CapabilityError::Failed { reason: "the note could not be read" })?
        .ok_or(CapabilityError::Invalid { field: "id", reason: "not_found" })?;
    Ok(Note { id: which.id, title: note.title, body: note.body, version: note.version })
}

fn prepare(call: &Call, note: NoteInput) -> Result<Proposal, CapabilityError> {
    service::prepare(call.conn, &note).map_err(|_| CapabilityError::Failed { reason: "the note could not be prepared" })
}

fn save(call: &mut Call, note: NoteInput) -> Result<Applied, CapabilityError> {
    // a task of the platform proposed it, and this account accepted it
    let origin = Origin::Task { task: call.task.into(), account: call.by.into() };
    service::apply(call.conn, &note, call.fingerprint, &origin).map_err(|e| CapabilityError::from_apply(e.code()))
}

capability::capability! {
    static READ_NOTE = "muestra.leer_nota" {
        description: "Lee una nota de la muestra.",
        effect: Query,
        permission: "muestra.notas.leer",
        data: [Internal],
        approval: Never,
        limits: Limits::READ,
        run: read(read),
    }
}

capability::capability! {
    static SAVE_NOTE = "muestra.guardar_nota" {
        description: "Guarda una nota de la muestra, nueva o cambiada, con la aprobación de la persona.",
        effect: Modify,
        permission: "muestra.notas.escribir",
        data: [Internal, Personal],
        approval: Required,
        limits: Limits::CHANGE,
        run: change(prepare, save),
    }
}
