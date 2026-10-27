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

/// The value at a path of an output: `missing[0]`, `profile.mission`.
pub fn at<'v>(value: &'v Value, path: &str) -> Option<&'v Value> {
    let mut here = value;
    for part in path.split('.') {
        let (key, indexes) = match part.find('[') {
            Some(i) => (&part[..i], &part[i..]),
            None => (part, ""),
        };
        if !key.is_empty() {
            here = here.get(key)?;
        }
        for index in indexes.split('[').skip(1) {
            here = here.get(index.strip_suffix(']')?.parse::<usize>().ok()?)?;
        }
    }
    Some(here)
}

/// The path as the schema writes it, from the path of a value: `missing[0].field` is `missing[].field`.
pub fn pattern_of(path: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for c in path.chars() {
        match c {
            '[' => {
                skipping = true;
                out.push('[');
            }
            ']' => {
                skipping = false;
                out.push(']');
            }
            _ if skipping => {}
            c => out.push(c),
        }
    }
    out
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
            // how many there are: the words may say it without counting by themselves (the figure check)
            data.push(Datum::Count { what: if at.is_empty() { "output".into() } else { at.into() }, class: unnamed, n: items.len() as u64 });
            for (i, v) in items.iter().enumerate() {
                walk(v, &format!("{pattern}[]"), &format!("{at}[{i}]"), fields, unnamed, data);
            }
        }
        Value::Null => {}
        leaf => data.push(Datum::Field { id: if at.is_empty() { "output".into() } else { at.into() }, class: unnamed, value: leaf.clone() }),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_path_finds_its_value() {
        let value = json!({ "missing": [{ "field": "name" }, { "field": "mission", "n": [7, 8] }], "profile": { "mission": "Cuidar" } });
        assert_eq!(at(&value, "missing[1]"), Some(&json!({ "field": "mission", "n": [7, 8] })));
        assert_eq!(at(&value, "missing[1].n[1]"), Some(&json!(8)));
        assert_eq!(at(&value, "profile.mission"), Some(&json!("Cuidar")));
        assert_eq!(at(&value, "missing[5]"), None);
        assert_eq!(at(&value, "profile.vision"), None);
    }

    #[test]
    fn a_path_of_a_value_gives_the_path_of_its_schema() {
        assert_eq!(pattern_of("missing[12].field"), "missing[].field");
        assert_eq!(pattern_of("profile.mission"), "profile.mission");
        assert_eq!(pattern_of("a[0].b[3]"), "a[].b[]");
    }

    /// Each leaf by its path, with the class of its field or, naming none, the one given; each list with its count.
    #[test]
    fn an_output_becomes_data_by_their_paths() {
        let fields = [OutputField { path: "profile.phone".into(), field: "m.profile.phone".into(), class: "personal" }];
        let value = json!({ "missing": [{ "field": "name" }, { "field": "mission" }], "profile": { "phone": "55", "note": null } });
        let mut data = Vec::new();
        walk(&value, "", "", &fields, Class::Internal, &mut data);
        let shown: Vec<String> = data
            .iter()
            .map(|d| match d {
                Datum::Field { id, class, .. } => format!("{id}:{}", class.id()),
                Datum::Count { what, n, .. } => format!("count {what}:{n}"),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(shown, ["count missing:2", "missing[0].field:internal", "missing[1].field:internal", "profile.phone:personal"]);
    }
}
