//! The fields of the data dictionary (K3) in the platform (ADR-003 A5; the AI blueprint §6 and §7, points 5 and 9).
//! What a capability gives names its fields, and what it asks of the person is fields too: the class of a datum comes
//! from where it is declared, once, and the person answers with the kind and the options of the field. The platform
//! never writes a class, and never checks a value by itself: the kernel does.

use serde_json::{Map, Value};

use capability::{Capability, ContractError, Fault, Registry, Run};
use kernel::Modules;

use super::outcome::Asked;

/// The key with which a property of an output type names its field of the dictionary:
/// `#[schemars(extend("x-field" = "muestra.note.title"))]`.
pub const FIELD_KEY: &str = "x-field";

/// A property of the output of a capability that names a field of the dictionary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputField {
    /// Where it is in the output: `title`, or `notes[].phone` inside a list.
    pub path: String,
    /// The full id of its field: `muestra.note.title`.
    pub field: String,
    /// The class of the datum, from the dictionary: what the policy reads, field by field (IA-3).
    pub class: &'static str,
}

/// The properties of the outputs of a capability that name a field, each with its class from the dictionary. A
/// property that names no field (an id, a version) has no class of its own: the `data` of the contract is its cap.
pub fn output_fields(cap: &Capability, modules: &Modules) -> Result<Vec<OutputField>, Fault> {
    let mut named = Vec::new();
    for schema in outputs(cap) {
        walk(&schema, &schema, String::new(), &mut Vec::new(), &mut named);
    }
    named
        .into_iter()
        .map(|(path, field)| {
            let class = class_within(cap, modules, &field)?;
            Ok(OutputField { path, field, class })
        })
        .collect()
}

/// The capabilities whose output names a field the dictionary does not have, or one of a class outside their `data`
/// (ADR-003 A5). The registry does not know the dictionary, so the platform checks it against the kernel: with one,
/// the app does not start.
pub fn outputs_outside_dictionary(registry: &Registry, modules: &Modules) -> Vec<ContractError> {
    registry.all().filter_map(|c| output_fields(c, modules).err().map(|fault| ContractError { id: c.id, fault })).collect()
}

/// The class of a field of the dictionary, if the capability may touch it.
fn class_within(cap: &Capability, modules: &Modules, field: &str) -> Result<&'static str, Fault> {
    let class = modules.field(field).ok_or(Fault::UnknownField)?.class;
    if cap.data.iter().any(|c| c.id() == class) {
        Ok(class)
    } else {
        Err(Fault::ClassOutsideData)
    }
}

fn outputs(cap: &Capability) -> Vec<Value> {
    match cap.run {
        Run::Read(s) => vec![(s.output)()],
        Run::Change { prepare, apply } => vec![(prepare.output)(), (apply.output)()],
    }
}

/// Every property under `node` that names a field, with its path. A `$ref` is followed into the `$defs` of the root,
/// once along each path, so a type that holds itself does not loop.
fn walk(root: &Value, node: &Value, path: String, refs: &mut Vec<String>, out: &mut Vec<(String, String)>) {
    if let Some(field) = node.get(FIELD_KEY).and_then(Value::as_str) {
        out.push((path.clone(), field.to_string()));
    }
    if let Some(r) = node.get("$ref").and_then(Value::as_str) {
        if let Some(target) = r.strip_prefix('#').and_then(|p| root.pointer(p)).filter(|_| !refs.iter().any(|x| x == r)) {
            refs.push(r.to_string());
            walk(root, target, path.clone(), refs, out);
            refs.pop();
        }
    }
    for (key, sub) in node.get("properties").and_then(Value::as_object).into_iter().flatten() {
        let inner = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
        walk(root, sub, inner, refs, out);
    }
    if let Some(items) = node.get("items") {
        walk(root, items, format!("{path}[]"), refs, out);
    }
    for key in ["anyOf", "oneOf", "allOf"] {
        for sub in node.get(key).and_then(Value::as_array).into_iter().flatten() {
            walk(root, sub, path.clone(), refs, out);
        }
    }
}

/// What a capability asks of the person: each field with its description from the dictionary, and the code of the
/// kernel where the last answer did not pass. Nothing when it asks for no field, for one the dictionary does not
/// have, or for one of a class outside its `data`: an input may not carry what the capability does not declare.
pub fn asked(cap: &Capability, modules: &Modules, ids: &[String], problems: &[(String, String)]) -> Option<Vec<Asked>> {
    if ids.is_empty() {
        return None;
    }
    ids.iter()
        .map(|id| {
            class_within(cap, modules, id).ok()?;
            let field = serde_json::to_value(modules.field(id)?).ok()?;
            let reason = problems.iter().find(|(f, _)| f == id).map(|(_, r)| r.clone());
            Some(Asked { id: id.clone(), field, reason })
        })
        .collect()
}

/// The values of an answer to the fields asked, under the ids of the input, once the kernel lets them pass
/// (`kernel::Entity::validate`). What was not asked is left out, and the rest of the input is only the context of
/// the conditions. If something does not pass: the code of each field, by its full id.
pub fn answered(modules: &Modules, ids: &[String], input: &Value, values: &Value) -> Result<Map<String, Value>, Vec<(String, String)>> {
    // every field asked is of one entity: the one the capability named
    let entity_id = ids.first().and_then(|id| id.rsplit_once('.')).map(|(e, _)| e).unwrap_or_default();
    let Some(entity) = entity_id.split_once('.').and_then(|(m, e)| modules.entity(m, e)) else {
        return Err(ids.iter().map(|id| (id.clone(), "unknown_field".to_string())).collect());
    };
    let mut answer = Map::new();
    for id in ids {
        if let (Some(v), Some((_, field))) = (values.get(id), id.rsplit_once('.')) {
            answer.insert(field.to_string(), v.clone());
        }
    }
    let fields_of_input = input.as_object().into_iter().flatten().filter(|(k, _)| entity.field(k).is_some());
    let mut context: Map<String, Value> = fields_of_input.map(|(k, v)| (k.clone(), v.clone())).collect();
    context.extend(answer.clone());
    let problems: Vec<(String, String)> = entity
        .validate(&context)
        .into_iter()
        .filter(|i| answer.contains_key(&i.field))
        .map(|i| (format!("{entity_id}.{}", i.field), i.reason))
        .collect();
    if problems.is_empty() {
        Ok(answer)
    } else {
        Err(problems)
    }
}
