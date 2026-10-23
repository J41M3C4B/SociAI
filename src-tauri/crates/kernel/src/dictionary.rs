//! The data dictionary (K3, the ERP blueprint §5): the one description of every entity and field of every module. A
//! module declares its entities in its manifest, each field with its kind, its options, whether it is needed, when it
//! applies, its limits, its rule and **its class**, once. From here come the validation of an input, what is missing,
//! which fields the history protects and the class the policy of the platform reads (IA-3, ADR-003 A5): no module
//! writes any of that again.
//!
//! The words are not here: the label of a field lives in `src/i18n/es-MX.ts` under its id.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::access::KEPT_IN_HISTORY;
use crate::change::Invalid;

/// An entity of a module: a note, the profile of the institution, a donor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Entity {
    /// `note`: lowercase letters, digits and `_`. With the module's id it names the entity (`Ref`).
    pub id: &'static str,
    pub fields: &'static [Field],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Text,
    LongText,
    /// A whole number (people, pieces).
    Number,
    /// Centavos, whole (D3).
    Money,
    Year,
    /// `YYYY-MM-DD`.
    Date,
    /// One code of `options`.
    Select,
    /// Several codes of `options`, none twice.
    MultiSelect,
    YesNo,
    Email,
    Phone,
}

/// Whether a field has to be filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    Optional,
    /// Needed for the data to be complete: it is listed in what is missing, but it never stops a save.
    Expected,
    /// Without it the entity is not saved.
    Mandatory,
}

/// When a field applies. A field that does not apply is not shown, not asked, not checked and not kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "when", rename_all = "snake_case")]
pub enum Condition {
    Always,
    /// Another field of the entity is filled.
    Filled { field: &'static str },
    /// Another field of the entity holds one of these codes (in a list: any of them).
    AnyOf { field: &'static str, values: &'static [&'static str] },
}

impl Condition {
    pub fn holds(&self, values: &Map<String, Value>) -> bool {
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

/// A check of a text beyond its kind, by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// Five digits.
    PostalCode,
    /// The RFC of an organization (persona moral).
    RfcMoral,
    /// Eighteen digits and their check digit.
    Clabe,
    /// The name of a place: letters and the signs of a name, so no phone, mail or key can hide in it.
    PlaceName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Field {
    /// Its id in the entity, also the key of its value and of its words: lowercase letters, digits and `_`.
    pub id: &'static str,
    pub kind: Kind,
    /// The codes of a select or a multi-select.
    pub options: &'static [&'static str],
    pub presence: Presence,
    pub applies_when: Condition,
    /// The class of the datum, by id (`kernel::CLASSES`): what the policy of the platform reads, and whether the
    /// history keeps its value.
    pub class: &'static str,
    /// The range of a number, a sum or a year.
    pub min: Option<i64>,
    pub max: Option<i64>,
    /// The most characters of a text.
    pub max_len: Option<u32>,
    pub rule: Option<Rule>,
}

impl Field {
    /// An optional field that always applies. Its class goes here, so there is no field without one.
    pub const fn new(id: &'static str, kind: Kind, class: &'static str) -> Field {
        Field { id, kind, options: &[], presence: Presence::Optional, applies_when: Condition::Always, class, min: None, max: None, max_len: None, rule: None }
    }
    pub const fn options(mut self, options: &'static [&'static str]) -> Field {
        self.options = options;
        self
    }
    pub const fn expected(mut self) -> Field {
        self.presence = Presence::Expected;
        self
    }
    pub const fn mandatory(mut self) -> Field {
        self.presence = Presence::Mandatory;
        self
    }
    pub const fn when(mut self, condition: Condition) -> Field {
        self.applies_when = condition;
        self
    }
    pub const fn range(mut self, min: i64, max: i64) -> Field {
        self.min = Some(min);
        self.max = Some(max);
        self
    }
    pub const fn max_len(mut self, max_len: u32) -> Field {
        self.max_len = Some(max_len);
        self
    }
    pub const fn rule(mut self, rule: Rule) -> Field {
        self.rule = Some(rule);
        self
    }

    /// The history says this field changed, never its value: its class is not one the history keeps.
    pub fn protected(&self) -> bool {
        !KEPT_IN_HISTORY.contains(&self.class)
    }
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

impl Entity {
    pub fn field(&self, id: &str) -> Option<&'static Field> {
        self.fields.iter().find(|f| f.id == id)
    }

    /// The fields whose values the history does not keep.
    pub fn protected(&self) -> impl Iterator<Item = &'static str> {
        self.fields.iter().filter(|f| f.protected()).map(|f| f.id)
    }

    /// What does not pass, in the order of the fields: a mandatory field that is empty, a value of the wrong kind,
    /// out of its list, its range or its rule, and a value of a field the entity does not have. A field that does
    /// not apply is not checked.
    pub fn validate(&self, values: &Map<String, Value>) -> Vec<Invalid> {
        let mut out = Vec::new();
        for f in self.fields.iter().filter(|f| f.applies_when.holds(values)) {
            match values.get(f.id) {
                v if !is_filled(v) => {
                    if f.presence == Presence::Mandatory {
                        out.push(Invalid::new(f.id, "required"));
                    }
                }
                Some(v) => out.extend(problem(f, v).map(|reason| Invalid::new(f.id, reason))),
                None => {}
            }
        }
        out.extend(values.keys().filter(|k| self.field(k).is_none()).map(|k| Invalid::new(k, "unknown_field")));
        out
    }

    /// The fields that are expected or mandatory, apply and are empty, in the order of the fields. What is missing
    /// is listed, never refused.
    pub fn missing(&self, values: &Map<String, Value>) -> Vec<&'static str> {
        self.fields
            .iter()
            .filter(|f| f.presence != Presence::Optional && f.applies_when.holds(values) && !is_filled(values.get(f.id)))
            .map(|f| f.id)
            .collect()
    }

    /// Empties the fields that do not apply, so nothing hidden is kept.
    pub fn clear_hidden(&self, values: &mut Map<String, Value>) {
        let hidden: Vec<&str> = self.fields.iter().filter(|f| !f.applies_when.holds(values)).map(|f| f.id).collect();
        for id in hidden {
            if let Some(v) = values.get_mut(id) {
                *v = Value::Null;
            }
        }
    }
}

/// What is wrong with one filled value, if anything, by its code.
fn problem(f: &Field, v: &Value) -> Option<&'static str> {
    let known = |s: &str| f.options.contains(&s);
    match f.kind {
        Kind::Select => match v.as_str() {
            Some(s) if known(s) => None,
            Some(_) => Some("code_unknown"),
            None => Some("wrong_type"),
        },
        Kind::MultiSelect => match v.as_array() {
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
        Kind::Number | Kind::Money | Kind::Year => match v.as_i64() {
            Some(n) if f.kind == Kind::Year && (f.min.is_some_and(|m| n < m) || f.max.is_some_and(|m| n > m)) => Some("year_invalid"),
            // a negative number only where its range allows it
            Some(n) if n < 0 && f.min.is_none_or(|m| m >= 0) => Some("negative_number"),
            Some(n) if f.min.is_some_and(|m| n < m) => Some("number_too_small"),
            Some(n) if f.max.is_some_and(|m| n > m) => Some("number_too_large"),
            Some(_) => None,
            None => Some("wrong_type"),
        },
        Kind::YesNo => v.as_bool().map_or(Some("wrong_type"), |_| None),
        Kind::Date => match v.as_str() {
            Some(s) if is_date(s.trim()) => None,
            Some(_) => Some("date_invalid"),
            None => Some("wrong_type"),
        },
        Kind::Text | Kind::LongText | Kind::Email | Kind::Phone => {
            let Some(s) = v.as_str() else { return Some("wrong_type") };
            if f.max_len.is_some_and(|m| s.trim().chars().count() > m as usize) {
                return Some("too_long");
            }
            match (f.kind, f.rule) {
                (Kind::Email, _) if !is_email(s.trim()) => Some("not_an_email"),
                (Kind::Phone, _) if !is_phone(s.trim()) => Some("not_a_phone"),
                (_, Some(Rule::PostalCode)) if !is_postal_code(s.trim()) => Some("postal_code_invalid"),
                (_, Some(Rule::RfcMoral)) if !is_rfc_moral(s.trim()) => Some("rfc_moral_invalid"),
                (_, Some(Rule::Clabe)) if !clabe_is_valid(s) => Some("clabe_invalid"),
                (_, Some(Rule::PlaceName)) if !is_place_name(s) => Some("place_name_invalid"),
                _ => None,
            }
        }
    }
}

/// `YYYY-MM-DD`, a day that exists, from 1800.
pub fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| s.get(r).filter(|x| x.bytes().all(|c| c.is_ascii_digit())).and_then(|x| x.parse::<u32>().ok());
    let (Some(y), Some(m), Some(d)) = (num(0..4), num(5..7), num(8..10)) else { return false };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    y >= 1800 && (1..=days).contains(&d)
}

/// The name of a place: letters (accents and Ñ too), spaces, periods, hyphens and apostrophes, up to 80.
pub fn is_place_name(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty() && s.chars().count() <= 80 && s.chars().all(|c| c.is_alphabetic() || matches!(c, ' ' | '.' | '-' | '\'' | '’'))
}

pub fn is_postal_code(s: &str) -> bool {
    s.len() == 5 && s.bytes().all(|c| c.is_ascii_digit())
}

/// Three letters (Ñ and & count), the date `YYMMDD` and a check of three letters or digits. Upper or lower case.
pub fn is_rfc_moral(s: &str) -> bool {
    let c: Vec<char> = s.to_uppercase().chars().collect();
    c.len() == 12
        && c[..3].iter().all(|x| x.is_ascii_uppercase() || *x == 'Ñ' || *x == '&')
        && is_date(&format!("20{}-{}-{}", c[3..5].iter().collect::<String>(), c[5..7].iter().collect::<String>(), c[7..9].iter().collect::<String>()))
        && c[9..].iter().all(|x| x.is_ascii_uppercase() || x.is_ascii_digit())
}

/// The CLABE of a bank account: eighteen digits (spaces and hyphens aside), the last one weighted 3-7-1 over the
/// first seventeen.
pub fn clabe_is_valid(s: &str) -> bool {
    let clean: String = s.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    let d: Vec<u32> = clean.chars().filter_map(|c| c.to_digit(10)).collect();
    if d.len() != 18 || clean.len() != 18 {
        return false;
    }
    let sum: u32 = d[..17].iter().enumerate().map(|(i, &x)| (x * [3, 7, 1][i % 3]) % 10).sum();
    (10 - sum % 10) % 10 == d[17]
}

/// Digits, spaces, `+`, `-` and parentheses, with eight digits at least.
pub fn is_phone(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_digit() || matches!(c, ' ' | '+' | '-' | '(' | ')')) && s.chars().filter(char::is_ascii_digit).count() >= 8
}

/// Something, `@` and a domain with a period, without spaces.
pub fn is_email(s: &str) -> bool {
    match s.split_once('@') {
        Some((user, domain)) => {
            !user.is_empty()
                && !domain.contains('@')
                && domain.split('.').count() >= 2
                && domain.split('.').all(|p| !p.is_empty())
                && !s.chars().any(char::is_whitespace)
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const KINDS: &[&str] = &["a", "b", "c"];
    const THING: Entity = Entity {
        id: "thing",
        fields: &[
            Field::new("name", Kind::Text, "public").mandatory().max_len(20),
            Field::new("kinds", Kind::MultiSelect, "internal").options(KINDS).expected(),
            Field::new("main", Kind::Select, "internal").options(KINDS).when(Condition::Filled { field: "kinds" }),
            Field::new("only_b", Kind::YesNo, "internal").when(Condition::AnyOf { field: "kinds", values: &["b"] }),
            Field::new("people", Kind::Number, "internal").range(0, 100),
            Field::new("balance", Kind::Money, "confidential").range(-1_000_00, 1_000_00),
            Field::new("year", Kind::Year, "public").range(1800, 2026),
            Field::new("zip", Kind::Text, "public").rule(Rule::PostalCode),
            Field::new("rfc", Kind::Text, "public").rule(Rule::RfcMoral),
            Field::new("clabe", Kind::Text, "confidential").rule(Rule::Clabe),
            Field::new("since", Kind::Date, "public"),
            Field::new("mail", Kind::Email, "personal"),
            Field::new("phone", Kind::Phone, "personal"),
        ],
    };

    fn values(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn codes(v: Value) -> Vec<(String, String)> {
        THING.validate(&values(v)).into_iter().map(|i| (i.field, i.reason)).collect()
    }

    fn pairs(p: &[(&str, &str)]) -> Vec<(String, String)> {
        p.iter().map(|(f, r)| (f.to_string(), r.to_string())).collect()
    }

    #[test]
    fn a_field_applies_by_its_condition() {
        let main = THING.field("main").unwrap();
        let only_b = THING.field("only_b").unwrap();
        let with_b = values(json!({ "kinds": ["a", "b"] }));
        assert!(!main.applies_when.holds(&values(json!({}))) && main.applies_when.holds(&with_b));
        assert!(!main.applies_when.holds(&values(json!({ "kinds": [] }))), "an empty list is not filled");
        assert!(!only_b.applies_when.holds(&values(json!({ "kinds": ["a"] }))) && only_b.applies_when.holds(&with_b));
        assert!(only_b.applies_when.holds(&values(json!({ "kinds": "b" }))), "one code too");
    }

    #[test]
    fn what_is_missing_is_listed_and_only_a_mandatory_field_stops_a_save() {
        assert_eq!(THING.missing(&values(json!({ "name": "  " }))), ["name", "kinds"]);
        assert_eq!(THING.missing(&values(json!({ "name": "Casa" }))), ["kinds"]);
        assert!(THING.missing(&values(json!({ "name": "Casa", "kinds": ["a"] }))).is_empty());
        assert_eq!(codes(json!({ "name": "  " })), pairs(&[("name", "required")]), "kinds is expected: it is missing, not invalid");
    }

    #[test]
    fn values_out_of_their_kind_list_or_range_do_not_pass() {
        assert_eq!(
            codes(json!({ "name": "Casa", "kinds": ["a", "z"], "people": 101, "year": 1700, "balance": -1_000_01 })),
            pairs(&[("kinds", "code_unknown"), ("people", "number_too_large"), ("balance", "number_too_small"), ("year", "year_invalid")])
        );
        assert_eq!(
            codes(json!({ "name": 3, "kinds": ["a", "a"], "main": "z", "people": -1, "only_b": "sí" })),
            pairs(&[("name", "wrong_type"), ("kinds", "repeated"), ("main", "code_unknown"), ("people", "negative_number")]),
            "only_b does not apply"
        );
        assert!(codes(json!({ "name": "Casa", "balance": -500_00 })).is_empty(), "a sum may be negative where its range says so");
        assert_eq!(codes(json!({ "name": "Una casa con un nombre largo" })), pairs(&[("name", "too_long")]));
    }

    #[test]
    fn a_rule_a_date_a_mail_and_a_phone_check_the_text() {
        let ok = json!({ "name": "Casa", "zip": "06700", "rfc": "abc010203xy9", "clabe": "002010077777777771", "since": "2024-02-29",
                         "mail": "contacto@casa.org.mx", "phone": "+52 (55) 1234-5678" });
        assert!(codes(ok).is_empty());
        let bad = json!({ "name": "Casa", "zip": "0670", "rfc": "ABC0102031", "clabe": "002010077777777772", "since": "2023-02-29",
                          "mail": "contacto@casa", "phone": "llámale a Rosa" });
        assert_eq!(
            codes(bad),
            pairs(&[
                ("zip", "postal_code_invalid"),
                ("rfc", "rfc_moral_invalid"),
                ("clabe", "clabe_invalid"),
                ("since", "date_invalid"),
                ("mail", "not_an_email"),
                ("phone", "not_a_phone"),
            ])
        );
    }

    #[test]
    fn a_value_of_a_field_the_entity_does_not_have_does_not_pass() {
        assert_eq!(codes(json!({ "name": "Casa", "color": "rojo" })), pairs(&[("color", "unknown_field")]));
    }

    #[test]
    fn a_field_that_does_not_apply_is_not_kept() {
        let mut v = values(json!({ "name": "Casa", "kinds": ["a"], "only_b": true }));
        THING.clear_hidden(&mut v);
        assert_eq!(v, values(json!({ "name": "Casa", "kinds": ["a"], "only_b": null })));
    }

    #[test]
    fn the_history_protects_every_class_but_the_public_and_the_internal() {
        assert_eq!(THING.protected().collect::<Vec<_>>(), ["balance", "clabe", "mail", "phone"]);
    }

    /// The screen draws a form from the same description.
    #[test]
    fn the_description_travels_as_json() {
        let j = serde_json::to_value(THING.field("main").unwrap()).unwrap();
        assert_eq!((j["kind"].clone(), j["presence"].clone(), j["class"].clone()), (json!("select"), json!("optional"), json!("internal")));
        assert_eq!(j["applies_when"], json!({ "when": "filled", "field": "kinds" }));
    }

    #[test]
    fn dates_postal_codes_rfc_clabe_and_places() {
        for good in ["2026-10-09", "2000-02-29", "1800-01-01"] {
            assert!(is_date(good), "{good}");
        }
        for bad in ["2026-13-01", "2026-04-31", "1900-02-29", "26-10-09", "2026/10/09", "2026-1-09", "1799-12-31", ""] {
            assert!(!is_date(bad), "{bad}");
        }
        assert!(is_postal_code("01000") && !is_postal_code("1000") && !is_postal_code("0100A") && !is_postal_code("010000"));
        for place in ["Benito Juárez", "San Pedro Tlaquepaque", "Dolores Hidalgo Cuna de la Independencia Nacional", "Peñón Blanco", "Gral. Escobedo", "Bahía de Banderas"] {
            assert!(is_place_name(place), "{place}");
        }
        for not_a_place in ["", "   ", "Zapopan 45010", "contacto@casa.org", "Llamar al 55 1234 5678", &"a".repeat(81)] {
            assert!(!is_place_name(not_a_place), "{not_a_place}");
        }
        for good in ["ABC010203XY9", "AÑ&991231AB1", "abc010203xy9"] {
            assert!(is_rfc_moral(good), "{good}");
        }
        // a person's RFC has 13 characters; a wrong date or a sign does not pass
        for bad in ["ABCD010203XY9", "ABC011303XY9", "AB1010203XY9", "ABC010203XY-", ""] {
            assert!(!is_rfc_moral(bad), "{bad}");
        }
        assert!(clabe_is_valid("002010077777777771") && clabe_is_valid("032 180 00011835971 9"));
        assert!(!clabe_is_valid("002010077777777772") && !clabe_is_valid("00201007777777777A"));
    }
}
