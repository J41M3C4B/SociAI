//! The context manager (P6), as small as the chat needs it now: the output of a capability as data on their way to a
//! model, each with its class. A property that names a field of the dictionary takes the class of that field (IA-8);
//! one that names none takes the strictest class its capability touches (ADR-004 §5.3). The policy decides then what
//! goes (`policy::to_model`): nothing here decides it.

use serde_json::Value;

use capability::Capability;

use super::dictionary::OutputField;
use super::policy::{self, Class, Datum, Destination, Purpose};

/// The data of an output, by their path in it (`missing[0].field`).
pub fn data_of(cap: &Capability, fields: &[OutputField], value: &Value, to: Destination, purpose: Purpose) -> Vec<Datum> {
    // a capability that declares no class has nothing to give: what names no field never goes
    let unnamed = policy::strictest(cap.data, to, purpose).unwrap_or(Class::SensitivePersonal);
    let mut data = Vec::new();
    walk(value, "", "", fields, unnamed, &mut data);
    data
}

/// `pattern` is the path as the schema writes it (`missing[].field`), `at` the path of this value (`missing[0].field`).
fn walk(value: &Value, pattern: &str, at: &str, fields: &[OutputField], unnamed: Class, data: &mut Vec<Datum>) {
    if let Some(field) = fields.iter().find(|f| f.path == pattern) {
        let class = Class::from_id(field.class).unwrap_or(Class::SensitivePersonal);
        data.push(Datum::Field { id: at.into(), class, value: value.clone() });
        return;
    }
    let join = |key: &str, base: &str| if base.is_empty() { key.to_string() } else { format!("{base}.{key}") };
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                walk(v, &join(k, pattern), &join(k, at), fields, unnamed, data);
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                walk(v, &format!("{pattern}[]"), &format!("{at}[{i}]"), fields, unnamed, data);
            }
        }
        Value::Null => {}
        leaf => data.push(Datum::Field { id: if at.is_empty() { "output".into() } else { at.into() }, class: unnamed, value: leaf.clone() }),
    }
}
