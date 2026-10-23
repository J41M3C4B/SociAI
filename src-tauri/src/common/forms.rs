//! Forms described as data (ADR-033 §1). A form says which fields it has, of what kind, with which options, when each
//! one applies, how sensitive it is, whether it may reach the AI and who uses it. The screen draws it, this validates
//! it, and the assistant reads the same description to explain a field (ADR-034). The words live elsewhere: the label
//! in `src/i18n/es-MX.ts` under the field id, the long explanation in the manual of the ERP.
//!
//! A value is plain JSON: text, a whole number, a list of codes or yes/no. Nothing here knows the app.

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

// the forms of the core use some kinds today; the rest arrive as each form moves to the catalog (ADR-033)
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Text,
    LongText,
    /// A whole number (people, pieces).
    Number,
    /// Whole pesos.
    Money,
    Year,
    /// `YYYY-MM-DD`.
    Date,
    /// One code of `options`.
    Select,
    /// Several codes of `options`.
    MultiSelect,
    YesNo,
    Email,
    Phone,
}

/// How far a value may travel.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    /// What the institution says of itself in public (its name, what it does).
    Public,
    /// The institution's own, not personal (its figures, its money).
    Internal,
    /// The institution's own and protected: contact, board members, bank accounts. Never to the AI nor to a tool of
    /// an agent; the code puts it in an export.
    InstitutionalPrivate,
    /// A datum of a person (the records of staff and people served).
    Personal,
}

/// Whether, and how, a value reaches the AI.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AiUse {
    AsIs,
    /// Only counted with others (groups of three or more).
    AggregateOnly,
    Never,
}

/// A check of a text with a name, beyond its kind: the code says what is wrong (ADR-033 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// Five digits.
    PostalCode,
    /// The RFC of an organization (persona moral): three letters, the date it was set up and three characters.
    RfcMoral,
    /// A CLABE: eighteen digits and its check digit (`common::ids`).
    Clabe,
    /// The name of a place (a municipality): letters, spaces and the signs of a name, up to 80. Nothing else fits, so
    /// no phone, mail or key can hide in it, and it needs no scanner: «Benito Juárez» is a place there.
    PlaceName,
}

/// When a field applies. A field that does not apply is not shown, not asked and not kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "when", rename_all = "snake_case")]
pub enum Condition {
    Always,
    /// Another field is filled (a text that is not blank, a list that is not empty). No field of the core waits for
    /// one today; the forms of the modules will.
    #[allow(dead_code)]
    Filled { field: &'static str },
    /// Another field holds one of these codes (a list: any of them).
    AnyOf { field: &'static str, values: &'static [&'static str] },
}

impl Condition {
    /// Whether it holds with these values. The field it looks at may belong to another form: whoever asks passes the
    /// values of that form too (`profile::forms`).
    pub fn holds(&self, values: &Values) -> bool {
        match *self {
            Condition::Always => true,
            Condition::Filled { field } => is_filled(values.get(field)),
            Condition::AnyOf { field, values: wanted } => match values.get(field) {
                Some(Value::String(s)) => wanted.contains(&s.as_str()),
                Some(Value::Array(a)) => a.iter().any(|x| x.as_str().is_some_and(|s| wanted.contains(&s))),
                _ => false,
            },
        }
    }

    /// The field it looks at, if any.
    pub const fn field(&self) -> Option<&'static str> {
        match *self {
            Condition::Always => None,
            Condition::Filled { field } | Condition::AnyOf { field, .. } => Some(field),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FieldSpec {
    /// Stable id, also the key of its value and of its words (`institution.populations`).
    pub id: &'static str,
    pub kind: FieldKind,
    /// The codes of a select or a multi-select.
    pub options: &'static [&'static str],
    /// Needed for the data to count as complete. It never stops a save: what is missing is listed, not refused.
    pub required: bool,
    /// Indispensable (ADR-042): the program cannot work well without it, so the first start asks it. It is required
    /// too.
    pub essential: bool,
    pub applies_when: Condition,
    pub sensitivity: Sensitivity,
    pub ai: AiUse,
    /// Who uses the value (`care`, `hr`, `facilities`, `finance`, `projects`, `ai`, `documents`): the «what is it
    /// for» of the field.
    pub used_by: &'static [&'static str],
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub rule: Option<Rule>,
}

impl FieldSpec {
    /// A field that applies always, is public, may reach the AI as it is and is not required.
    pub const fn new(id: &'static str, kind: FieldKind) -> Self {
        FieldSpec { id, kind, options: &[], required: false, essential: false, applies_when: Condition::Always, sensitivity: Sensitivity::Public, ai: AiUse::AsIs, used_by: &[], min: None, max: None, rule: None }
    }
    pub const fn options(mut self, options: &'static [&'static str]) -> Self {
        self.options = options;
        self
    }
    pub const fn required(mut self) -> Self {
        self.required = true;
        self
    }
    /// Indispensable, and so required (ADR-042).
    pub const fn essential(mut self) -> Self {
        self.essential = true;
        self.required = true;
        self
    }
    pub const fn when(mut self, condition: Condition) -> Self {
        self.applies_when = condition;
        self
    }
    pub const fn sensitivity(mut self, sensitivity: Sensitivity, ai: AiUse) -> Self {
        self.sensitivity = sensitivity;
        self.ai = ai;
        self
    }
    pub const fn used_by(mut self, used_by: &'static [&'static str]) -> Self {
        self.used_by = used_by;
        self
    }
    pub const fn range(mut self, min: i64, max: i64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }
    pub const fn rule(mut self, rule: Rule) -> Self {
        self.rule = Some(rule);
        self
    }
    /// The field its condition looks at, if any.
    pub fn looks_at(&self) -> Option<&'static str> {
        self.applies_when.field()
    }
}

/// A group of fields drawn together, in one or two columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SectionSpec {
    pub id: &'static str,
    pub columns: u8,
    pub fields: &'static [FieldSpec],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FormSpec {
    pub id: &'static str,
    pub sections: &'static [SectionSpec],
}

/// The values of a form, by field id.
pub type Values = BTreeMap<String, Value>;

/// Something wrong with a value. `blocking` stops the save; a missing value never does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldIssue {
    pub field: &'static str,
    pub code: &'static str,
    pub blocking: bool,
}

/// Whether a value says something: a text that is not blank, a list that is not empty, a number or a yes/no.
pub fn is_filled(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) => !s.trim().is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(_) => true,
    }
}

impl FormSpec {
    pub fn fields(&self) -> impl Iterator<Item = &FieldSpec> {
        self.sections.iter().flat_map(|s| s.fields.iter())
    }

    pub fn field(&self, id: &str) -> Option<&FieldSpec> {
        self.fields().find(|f| f.id == id)
    }

    /// Whether a field applies with these values.
    pub fn applies(&self, f: &FieldSpec, values: &Values) -> bool {
        f.applies_when.holds(values)
    }

    /// The fields of other forms its conditions look at, each once: their values travel with the form.
    pub fn looks_outside(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for id in self.fields().flat_map(FieldSpec::looks_at) {
            if self.field(id).is_none() && !out.contains(&id) {
                out.push(id);
            }
        }
        out
    }

    /// The required fields that apply and are empty, in the order of the form.
    pub fn missing(&self, values: &Values) -> Vec<&'static str> {
        self.fields().filter(|f| f.required && self.applies(f, values) && !is_filled(values.get(f.id))).map(|f| f.id).collect()
    }

    /// Drops the values of fields the form does not have or that do not apply, so nothing hidden is kept.
    pub fn clear_hidden(&self, values: &mut Values) {
        let snapshot = values.clone();
        values.retain(|id, _| self.field(id).is_some_and(|f| self.applies(f, &snapshot)));
    }

    /// What is wrong with the values: blocking problems (a code out of the list, a number out of range, a value of
    /// the wrong kind) and, without blocking, what is missing.
    pub fn validate(&self, values: &Values) -> Vec<FieldIssue> {
        let mut out = Vec::new();
        for f in self.fields().filter(|f| self.applies(f, values)) {
            let v = values.get(f.id);
            if !is_filled(v) {
                if f.required {
                    out.push(FieldIssue { field: f.id, code: "missing", blocking: false });
                }
                continue;
            }
            if let Some(code) = problem(f, v.unwrap_or(&Value::Null)) {
                out.push(FieldIssue { field: f.id, code, blocking: true });
            }
        }
        out
    }
}

/// What is wrong with one filled value, if anything (a proposal of the AI is checked with it, ADR-034).
pub fn problem(f: &FieldSpec, v: &Value) -> Option<&'static str> {
    let known = |s: &str| f.options.contains(&s);
    match f.kind {
        FieldKind::Select => match v.as_str() {
            Some(s) if known(s) => None,
            Some(_) => Some("code_unknown"),
            None => Some("wrong_type"),
        },
        FieldKind::MultiSelect => match v.as_array() {
            Some(a) => {
                let codes: Vec<&str> = a.iter().filter_map(Value::as_str).collect();
                if codes.len() != a.len() {
                    Some("wrong_type")
                } else if !codes.iter().all(|c| known(c)) {
                    Some("code_unknown")
                } else if (1..codes.len()).any(|i| codes[..i].contains(&codes[i])) {
                    Some("repeated")
                } else {
                    None
                }
            }
            None => Some("wrong_type"),
        },
        FieldKind::Number | FieldKind::Money | FieldKind::Year => match v.as_i64() {
            Some(n) if f.kind == FieldKind::Year && (f.min.is_some_and(|m| n < m) || f.max.is_some_and(|m| n > m)) => Some("year_invalid"),
            Some(n) if n < 0 => Some("negative_number"),
            Some(n) if f.min.is_some_and(|m| n < m) => Some("number_too_small"),
            Some(n) if f.max.is_some_and(|m| n > m) => Some("number_too_large"),
            Some(_) => None,
            None => Some("wrong_type"),
        },
        FieldKind::YesNo => v.as_bool().map_or(Some("wrong_type"), |_| None),
        FieldKind::Date => match v.as_str() {
            Some(s) if is_date(s.trim()) => None,
            Some(_) => Some("date_invalid"),
            None => Some("wrong_type"),
        },
        FieldKind::Text | FieldKind::LongText | FieldKind::Email | FieldKind::Phone => match (v.as_str(), f.rule) {
            (None, _) => Some("wrong_type"),
            (Some(s), Some(Rule::PostalCode)) if !is_postal_code(s.trim()) => Some("postal_code_invalid"),
            (Some(s), Some(Rule::RfcMoral)) if !is_rfc_moral(s.trim()) => Some("rfc_moral_invalid"),
            (Some(s), Some(Rule::Clabe)) if !crate::common::ids::clabe_is_valid(s) => Some("clabe_invalid"),
            (Some(s), Some(Rule::PlaceName)) if !is_place_name(s) => Some("place_name_invalid"),
            _ => None,
        },
    }
}

// the checks of a text live in the kernel, with the dictionary that will replace this description (ERP-2, ERP-1)
pub use kernel::{is_date, is_place_name, is_postal_code, is_rfc_moral};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const KINDS: &[&str] = &["a", "b", "c"];
    const FORM: FormSpec = FormSpec {
        id: "test",
        sections: &[SectionSpec {
            id: "one",
            columns: 1,
            fields: &[
                FieldSpec::new("name", FieldKind::Text).required(),
                FieldSpec::new("kinds", FieldKind::MultiSelect).options(KINDS).required(),
                FieldSpec::new("main", FieldKind::Select).options(KINDS).when(Condition::Filled { field: "kinds" }),
                FieldSpec::new("only_b", FieldKind::YesNo).when(Condition::AnyOf { field: "kinds", values: &["b"] }),
                FieldSpec::new("people", FieldKind::Number).range(0, 100),
                FieldSpec::new("year", FieldKind::Year).range(1800, 2026),
                FieldSpec::new("secret", FieldKind::Text).sensitivity(Sensitivity::InstitutionalPrivate, AiUse::Never),
                FieldSpec::new("zip", FieldKind::Text).rule(Rule::PostalCode),
                FieldSpec::new("rfc", FieldKind::Text).rule(Rule::RfcMoral),
                FieldSpec::new("since", FieldKind::Date),
            ],
        }],
    };

    fn values(v: serde_json::Value) -> Values {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn a_field_applies_by_its_condition() {
        let empty = values(json!({}));
        let with_b = values(json!({ "kinds": ["a", "b"] }));
        let main = FORM.field("main").unwrap();
        let only_b = FORM.field("only_b").unwrap();
        assert!(!FORM.applies(main, &empty) && FORM.applies(main, &with_b));
        assert!(!FORM.applies(only_b, &values(json!({ "kinds": ["a"] }))) && FORM.applies(only_b, &with_b));
        assert!(!FORM.applies(main, &values(json!({ "kinds": [] }))), "an empty list is not filled");
    }

    /// A condition may look at a field of another form; that form's values travel with this one.
    #[test]
    fn a_form_says_which_fields_of_other_forms_it_looks_at() {
        const OUTSIDE: FormSpec = FormSpec {
            id: "outside",
            sections: &[SectionSpec {
                id: "one",
                columns: 1,
                fields: &[
                    FieldSpec::new("seats", FieldKind::Number).when(Condition::AnyOf { field: "kinds", values: &["b"] }),
                    FieldSpec::new("people", FieldKind::Number).when(Condition::AnyOf { field: "kinds", values: &["a"] }),
                    FieldSpec::new("note", FieldKind::Text).when(Condition::Filled { field: "people" }),
                ],
            }],
        };
        assert_eq!(OUTSIDE.looks_outside(), vec!["kinds"], "each once, and not its own fields");
        assert!(FORM.looks_outside().is_empty());
        let seats = OUTSIDE.field("seats").unwrap();
        assert!(OUTSIDE.applies(seats, &values(json!({ "kinds": ["a", "b"] }))) && !OUTSIDE.applies(seats, &values(json!({ "kinds": ["a"] }))));
    }

    #[test]
    fn missing_is_listed_and_never_blocks() {
        let issues = FORM.validate(&values(json!({ "name": "  " })));
        assert_eq!(issues, vec![FieldIssue { field: "name", code: "missing", blocking: false }, FieldIssue { field: "kinds", code: "missing", blocking: false }]);
        assert_eq!(FORM.missing(&values(json!({ "name": "Casa" }))), vec!["kinds"]);
        assert!(FORM.missing(&values(json!({ "name": "Casa", "kinds": ["a"] }))).is_empty());
    }

    #[test]
    fn values_out_of_their_list_range_or_kind_block() {
        let bad = values(json!({
            "name": "Casa", "kinds": ["a", "z"], "people": 101, "year": 1700
        }));
        let codes: Vec<_> = FORM.validate(&bad).into_iter().map(|i| (i.field, i.code, i.blocking)).collect();
        assert_eq!(codes, vec![("kinds", "code_unknown", true), ("people", "number_too_large", true), ("year", "year_invalid", true)]);
        let wrong = values(json!({ "name": 3, "kinds": ["a", "a"], "people": -1, "main": "z" }));
        let codes: Vec<_> = FORM.validate(&wrong).into_iter().map(|i| (i.field, i.code)).collect();
        assert_eq!(codes, vec![("name", "wrong_type"), ("kinds", "repeated"), ("main", "code_unknown"), ("people", "negative_number")]);
    }

    #[test]
    fn a_rule_and_a_date_check_the_text() {
        let ok = values(json!({ "name": "Casa", "kinds": ["a"], "zip": "06700", "rfc": "abc010203xy9", "since": "2024-02-29" }));
        assert!(FORM.validate(&ok).is_empty(), "{:?}", FORM.validate(&ok));
        let bad = values(json!({ "name": "Casa", "kinds": ["a"], "zip": "0670", "rfc": "ABC0102031", "since": "2023-02-29" }));
        let codes: Vec<_> = FORM.validate(&bad).into_iter().map(|i| (i.field, i.code)).collect();
        assert_eq!(codes, vec![("zip", "postal_code_invalid"), ("rfc", "rfc_moral_invalid"), ("since", "date_invalid")]);
    }

    #[test]
    fn a_value_of_a_field_that_does_not_apply_is_not_checked_nor_kept() {
        let mut v = values(json!({ "name": "Casa", "kinds": ["a"], "only_b": "not a bool", "unknown": 1 }));
        assert!(FORM.validate(&v).is_empty(), "only_b does not apply");
        FORM.clear_hidden(&mut v);
        assert_eq!(v, values(json!({ "name": "Casa", "kinds": ["a"] })));
    }

    #[test]
    fn the_description_travels_to_the_screen_as_json() {
        let j = serde_json::to_value(FORM.field("main").unwrap()).unwrap();
        assert_eq!(j["kind"], "select");
        assert_eq!(j["applies_when"], json!({ "when": "filled", "field": "kinds" }));
        assert_eq!(serde_json::to_value(FORM.field("secret").unwrap()).unwrap()["sensitivity"], "institutional_private");
    }
}
