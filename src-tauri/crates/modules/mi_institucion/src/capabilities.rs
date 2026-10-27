//! What Mi institución publishes (ADR-001): what is missing of the profile, a window of it, saving a window, and
//! changing only the fields it is given (ERP-11). Each capability lives in this crate with the contract and with a
//! permission the manifest defines (ADR-003 A2). The platform finds them in the registry and invokes them through its
//! executor; the module never knows the platform.
//!
//! What a capability gives names its fields of the dictionary (`x-field`), so the class of each datum is the one the
//! catalog declares, never written here again (ADR-003 A5). The fields a change takes are named the same way.

use capability::{Call, CapabilityError, Limits};
use kernel::{Applied, Origin, Proposal};

use crate::catalog::PROFILE;
use crate::contract::{ChangeInput, WindowInput};
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

/// The fields of the profile, each with its type in JSON: the one list `Profile` and `ProfileChange` come from, so the
/// two never drift apart. A test checks that it is the catalog.
macro_rules! profile_types {
    ($($field:ident: $ty:ty),* $(,)?) => {
        /// The profile, each field by its id in the dictionary. Only the fields of the window asked come filled.
        #[derive(Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
        pub struct Profile {
            $(
                #[schemars(extend("x-field" = concat!("mi_institucion.profile.", stringify!($field))))]
                pub $field: Option<$ty>,
            )*
        }

        /// What changing data of the profile asks: only the fields it changes, each by its id in the dictionary, from
        /// any window. A field left out is left as it is; a `null` empties it. No other field fits.
        #[derive(Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct ProfileChange {
            $(
                #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
                #[schemars(extend("x-field" = concat!("mi_institucion.profile.", stringify!($field))))]
                pub $field: Option<Option<$ty>>,
            )*
        }

        /// The ids of the fields, in the order of the list.
        #[cfg(test)]
        const FIELDS: &[&str] = &[$(stringify!($field)),*];
    };
}

profile_types! {
    name: String, mission: String, populations: Vec<String>, sex_served: String, modalities: Vec<String>,
    care_areas: Vec<String>, services: String, age_min: i64, age_max: i64, admission_criteria: String,
    contact_phone: String, contact_email: String, street: String, ext_number: String, int_number: String,
    neighborhood: String, postal_code: String, municipality: String, state: String, legal_name: String,
    purpose: String, legal_form: String, founded_year: i64, junta_folio: String, legal_rfc: String, tax_regime: String,
    fiscal_postal_code: String, authorized_donee: String, cluni: String, donee_category: String,
    donee_letter_number: String, donee_letter_date: String, cluni_key: String, legal_rep_name: String,
    legal_rep_valid_until: String, capacity_total: i64, served_estimate: i64, served_last_year: i64,
    staff_paid_estimate: i64, staff_volunteer_estimate: i64, notes: String
}

/// A field that comes, even as `null`, is `Some`: `Some(None)` empties it. One that does not come stays `None`.
fn present<'de, D: serde::Deserializer<'de>, T: serde::Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
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
    ask_what_is_required(proposal, |f| window.is_some_and(|w| w.has(f)))
}

/// The proposal, unless a field the profile cannot be saved without is missing among `asked`: then that field is
/// asked of the person, by its field of the dictionary (IA-8).
fn ask_what_is_required(proposal: Proposal, asked: impl Fn(&str) -> bool) -> Result<Proposal, CapabilityError> {
    let needed: Vec<&'static str> = proposal
        .invalid
        .iter()
        .filter(|i| i.reason == "required" && asked(&i.field))
        .filter_map(|i| PROFILE.field(&i.field))
        .map(|f| f.id)
        .collect();
    if !needed.is_empty() {
        return Err(CapabilityError::needs("mi_institucion.profile", needed));
    }
    Ok(proposal)
}

/// A task of the platform proposed it, and this account accepted it.
fn by_the_task(call: &Call) -> Origin {
    Origin::Task { task: call.task.into(), account: call.by.into() }
}

fn save(call: &mut Call, input: WindowInput) -> Result<Applied, CapabilityError> {
    let origin = by_the_task(call);
    service::apply(call.conn, &input, call.fingerprint, &origin).map_err(|e| CapabilityError::from_apply(e.code()))
}

/// The fields the change names, as the service takes them. Naming none is not a change.
fn change_input(fields: ProfileChange) -> Result<ChangeInput, CapabilityError> {
    match serde_json::to_value(fields) {
        Ok(serde_json::Value::Object(values)) if !values.is_empty() => Ok(ChangeInput { values }),
        Ok(_) => Err(CapabilityError::Invalid { field: "input", reason: "no_fields" }),
        Err(_) => Err(CapabilityError::Failed { reason: "the change could not be read" }),
    }
}

/// What changing these fields would do. Any field missing that the profile cannot be saved without (the name of a
/// profile still empty) is asked of the person.
fn prepare_change(call: &Call, fields: ProfileChange) -> Result<Proposal, CapabilityError> {
    let input = change_input(fields)?;
    let proposal = service::prepare_change(call.conn, &input).map_err(|_| CapabilityError::Failed { reason: "the change could not be prepared" })?;
    ask_what_is_required(proposal, |_| true)
}

fn apply_change(call: &mut Call, fields: ProfileChange) -> Result<Applied, CapabilityError> {
    let input = change_input(fields)?;
    let origin = by_the_task(call);
    service::apply_change(call.conn, &input, call.fingerprint, &origin).map_err(|e| CapabilityError::from_apply(e.code()))
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

capability::capability! {
    static CHANGE_DATA = "mi_institucion.cambiar_datos" {
        description: "Cambia solo los datos del perfil de la institución que se le dan, de cualquier ventana, con la aprobación de la persona. Un dato que no se da se queda como está.",
        effect: Modify,
        permission: "mi_institucion.perfil.escribir",
        data: [Public, Internal, Confidential, Personal],
        approval: Required,
        limits: Limits::CHANGE,
        run: change(prepare_change, apply_change),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use capability::{Registry, Run};
    use rusqlite::Connection;
    use serde_json::{json, Value};

    const ROSA: &str = "usr_01JB3M7Q8ZK4X2N6V9R5T0W1YD";
    const TASK: &str = "task_01JB3M7Q8ZK4X2N6V9R5T0W1YC";

    #[test]
    fn the_fields_of_the_profile_types_are_the_catalog() {
        let catalog: Vec<&str> = PROFILE.fields.iter().map(|f| f.id).collect();
        assert_eq!(FIELDS, catalog);
    }

    /// The input of a change names each field of the dictionary, as an output does, asks for none and takes no other.
    #[test]
    fn a_change_names_each_field_and_takes_no_other() {
        let schema = serde_json::to_value(schemars::schema_for!(ProfileChange)).unwrap();
        let properties = schema["properties"].as_object().unwrap();
        assert_eq!(properties.len(), FIELDS.len());
        for (id, property) in properties {
            assert_eq!(property["x-field"], json!(format!("mi_institucion.profile.{id}")), "{id}");
        }
        assert_eq!(schema["additionalProperties"], json!(false));
        assert!(schema.get("required").is_none_or(|r| r.as_array().is_some_and(Vec::is_empty)), "{schema}");
    }

    /// `mi_institucion.cambiar_datos` through the registry, as the platform runs it: the proposal changes only the
    /// field it was given, and applying it with the approved fingerprint leaves the task as the origin.
    #[test]
    fn changing_data_runs_from_the_registry() {
        let registry = Registry::discover().unwrap();
        let cap = registry.get("mi_institucion.cambiar_datos").unwrap();
        let Run::Change { prepare, apply } = cap.run else { panic!("a modification prepares and applies") };
        let mut conn = Connection::open_in_memory().unwrap();
        kernel::migrate(&mut conn, &kernel::Modules::discover().unwrap()).unwrap();
        let identity = WindowInput { window: "identity".into(), values: json!({ "name": "Casa Ficticia" }).as_object().unwrap().clone() };
        service::apply(&mut conn, &identity, None, &Origin::Person { account: ROSA.into() }).unwrap();

        let reason = |r: Result<Value, CapabilityError>| match r {
            Err(CapabilityError::Invalid { field, reason }) => (field, reason),
            other => panic!("{other:?}"),
        };
        let call = Call::new(&mut conn, ROSA, TASK);
        assert_eq!(reason((prepare.call)(&call, json!({}))), ("input", "no_fields"));
        assert_eq!(reason((prepare.call)(&call, json!({ "patronato": "Rosa" }))), ("input", "does not fit the schema of the capability"));
        let proposal = (prepare.call)(&call, json!({ "contact_phone": "55 1234 5678", "notes": null })).unwrap();
        assert_eq!(proposal["changes"].as_array().unwrap().len(), 1, "a null of what is empty changes nothing: {proposal}");
        assert_eq!(proposal["changes"][0]["field"], json!("contact_phone"));

        let fingerprint = proposal["fingerprint"].as_str().unwrap().to_string();
        let mut applying = Call::applying(&mut conn, ROSA, TASK, &fingerprint);
        let applied = (apply.call)(&mut applying, json!({ "contact_phone": "55 1234 5678", "notes": null })).unwrap();
        assert_eq!(applied["version"], json!(2));
        let now = service::profile(&conn).unwrap();
        assert_eq!((&now.values["name"], &now.values["contact_phone"]), (&json!("Casa Ficticia"), &json!("55 1234 5678")));
        let history = kernel::history(&conn, &service::profile_ref()).unwrap();
        assert_eq!(history[1].origin, Origin::Task { task: TASK.into(), account: ROSA.into() });
    }
}
