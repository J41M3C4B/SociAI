//! What Mi institución publishes (ADR-001): what is missing of the profile, a window of it, and saving a window. Each
//! capability lives in this crate with the contract and with a permission the manifest defines (ADR-003 A2). The
//! platform finds them in the registry and invokes them through its executor; the module never knows the platform.
//!
//! What a capability gives names its fields of the dictionary (`x-field`), so the class of each datum is the one the
//! catalog declares, never written here again (ADR-003 A5).

use capability::{Call, CapabilityError, Limits};
use kernel::{Applied, Origin, Proposal};

use crate::catalog::PROFILE;
use crate::contract::WindowInput;
use crate::service;
use crate::windows;

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct Nothing {}

/// A field that is missing, and the window it is in.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct Missing {
    pub window: &'static str,
    pub field: &'static str,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct WhatIsMissing {
    /// In the order of the windows.
    pub missing: Vec<Missing>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct WhichWindow {
    /// `identity`, `contact`, `legal` or `capacity`.
    pub window: String,
}

/// The profile, each field by its id in the dictionary. Only the fields of the window asked come filled.
#[derive(Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct Profile {
    #[schemars(extend("x-field" = "mi_institucion.profile.name"))]
    pub name: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.mission"))]
    pub mission: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.populations"))]
    pub populations: Option<Vec<String>>,
    #[schemars(extend("x-field" = "mi_institucion.profile.sex_served"))]
    pub sex_served: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.modalities"))]
    pub modalities: Option<Vec<String>>,
    #[schemars(extend("x-field" = "mi_institucion.profile.care_areas"))]
    pub care_areas: Option<Vec<String>>,
    #[schemars(extend("x-field" = "mi_institucion.profile.services"))]
    pub services: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.age_min"))]
    pub age_min: Option<i64>,
    #[schemars(extend("x-field" = "mi_institucion.profile.age_max"))]
    pub age_max: Option<i64>,
    #[schemars(extend("x-field" = "mi_institucion.profile.admission_criteria"))]
    pub admission_criteria: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.contact_phone"))]
    pub contact_phone: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.contact_email"))]
    pub contact_email: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.street"))]
    pub street: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.ext_number"))]
    pub ext_number: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.int_number"))]
    pub int_number: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.neighborhood"))]
    pub neighborhood: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.postal_code"))]
    pub postal_code: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.municipality"))]
    pub municipality: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.state"))]
    pub state: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.legal_name"))]
    pub legal_name: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.purpose"))]
    pub purpose: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.legal_form"))]
    pub legal_form: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.founded_year"))]
    pub founded_year: Option<i64>,
    #[schemars(extend("x-field" = "mi_institucion.profile.junta_folio"))]
    pub junta_folio: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.legal_rfc"))]
    pub legal_rfc: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.tax_regime"))]
    pub tax_regime: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.fiscal_postal_code"))]
    pub fiscal_postal_code: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.authorized_donee"))]
    pub authorized_donee: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.cluni"))]
    pub cluni: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.donee_category"))]
    pub donee_category: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.donee_letter_number"))]
    pub donee_letter_number: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.donee_letter_date"))]
    pub donee_letter_date: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.cluni_key"))]
    pub cluni_key: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.legal_rep_name"))]
    pub legal_rep_name: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.legal_rep_valid_until"))]
    pub legal_rep_valid_until: Option<String>,
    #[schemars(extend("x-field" = "mi_institucion.profile.capacity_total"))]
    pub capacity_total: Option<i64>,
    #[schemars(extend("x-field" = "mi_institucion.profile.served_estimate"))]
    pub served_estimate: Option<i64>,
    #[schemars(extend("x-field" = "mi_institucion.profile.served_last_year"))]
    pub served_last_year: Option<i64>,
    #[schemars(extend("x-field" = "mi_institucion.profile.staff_paid_estimate"))]
    pub staff_paid_estimate: Option<i64>,
    #[schemars(extend("x-field" = "mi_institucion.profile.staff_volunteer_estimate"))]
    pub staff_volunteer_estimate: Option<i64>,
    #[schemars(extend("x-field" = "mi_institucion.profile.notes"))]
    pub notes: Option<String>,
}

/// A window of the profile, at the version it was read.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct Window {
    pub window: String,
    pub version: i64,
    pub profile: Profile,
}

fn failed(_: rusqlite::Error) -> CapabilityError {
    CapabilityError::Failed { reason: "the profile could not be read" }
}

fn what_is_missing(call: &Call, _: Nothing) -> Result<WhatIsMissing, CapabilityError> {
    let missing = service::missing(call.conn).map_err(failed)?;
    let missing = windows::WINDOWS
        .iter()
        .flat_map(|w| w.fields().filter(|f| missing.contains(f)).map(|field| Missing { window: w.id, field }))
        .collect();
    Ok(WhatIsMissing { missing })
}

fn read_window(call: &Call, which: WhichWindow) -> Result<Window, CapabilityError> {
    let window = windows::window(&which.window).ok_or(CapabilityError::Invalid { field: "window", reason: "unknown_window" })?;
    let now = service::profile(call.conn).map_err(failed)?;
    let shown: serde_json::Map<_, _> = now.values.into_iter().filter(|(k, _)| window.has(k)).collect();
    let profile = serde_json::from_value(shown.into()).map_err(|_| CapabilityError::Failed { reason: "the profile could not be read" })?;
    Ok(Window { window: window.id.into(), version: now.version, profile })
}

/// What saving the window would do. A field of the window the profile cannot be saved without is asked of the
/// person, by its field of the dictionary (IA-8).
fn prepare(call: &Call, input: WindowInput) -> Result<Proposal, CapabilityError> {
    let proposal = service::prepare(call.conn, &input).map_err(|_| CapabilityError::Failed { reason: "the window could not be prepared" })?;
    let window = windows::window(&input.window);
    let needed: Vec<&'static str> = proposal
        .invalid
        .iter()
        .filter(|i| i.reason == "required" && window.is_some_and(|w| w.has(&i.field)))
        .filter_map(|i| PROFILE.field(&i.field))
        .map(|f| f.id)
        .collect();
    if !needed.is_empty() {
        return Err(CapabilityError::needs("mi_institucion.profile", needed));
    }
    Ok(proposal)
}

fn save(call: &mut Call, input: WindowInput) -> Result<Applied, CapabilityError> {
    // a task of the platform proposed it, and this account accepted it
    let origin = Origin::Task { task: call.task.into(), account: call.by.into() };
    service::apply(call.conn, &input, call.fingerprint, &origin).map_err(|e| CapabilityError::from_apply(e.code()))
}

capability::capability! {
    static WHAT_IS_MISSING = "mi_institucion.que_falta" {
        description: "Dice qué datos le faltan al perfil de la institución y en qué ventana está cada uno.",
        effect: Query,
        permission: "mi_institucion.perfil.leer",
        data: [Internal],
        approval: Never,
        limits: Limits::READ,
        run: read(what_is_missing),
    }
}

capability::capability! {
    static READ_WINDOW = "mi_institucion.ver_ventana" {
        description: "Da los datos de una ventana del perfil de la institución: su identidad, su contacto, sus datos legales o su capacidad.",
        effect: Query,
        permission: "mi_institucion.perfil.leer",
        data: [Public, Internal, Confidential, Personal],
        approval: Never,
        limits: Limits::READ,
        run: read(read_window),
    }
}

capability::capability! {
    static SAVE_WINDOW = "mi_institucion.guardar_ventana" {
        description: "Guarda una ventana del perfil de la institución, con la aprobación de la persona.",
        effect: Modify,
        permission: "mi_institucion.perfil.escribir",
        data: [Public, Internal, Confidential, Personal],
        approval: Required,
        limits: Limits::CHANGE,
        run: change(prepare, save),
    }
}
