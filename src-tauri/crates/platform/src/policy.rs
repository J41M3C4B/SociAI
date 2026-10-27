//! The policy of data (P10, ADR-004): one decision. Given the class of a datum, where it goes and what for, it says
//! whether it goes, goes covered, goes only as a count, or does not go. It is a table and nothing else: no prompt, no
//! tool and no expression of an agent says what the AI sees. The runtime asks it at four points: the text of the chat,
//! what a model receives, the classes a capability touches, and what an outcome hands over.

use std::sync::OnceLock;

use serde::Serialize;
use serde_json::Value;

use capability::DataClass;
use privacy::vault::Vault;
use privacy::{FindingKind, RegexScanner, ScannerConfig, SensitiveScanner};

use super::dictionary::OutputField;

/// The classes of data (ADR-004 §1), the same as `kernel::CLASSES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    Public,
    Internal,
    Confidential,
    Personal,
    SensitivePersonal,
}

impl Class {
    pub const ALL: [Class; 5] = [Class::Public, Class::Internal, Class::Confidential, Class::Personal, Class::SensitivePersonal];

    /// Its id, as the dictionary gives it.
    pub fn id(self) -> &'static str {
        match self {
            Class::Public => "public",
            Class::Internal => "internal",
            Class::Confidential => "confidential",
            Class::Personal => "personal",
            Class::SensitivePersonal => "sensitive_personal",
        }
    }

    pub fn from_id(id: &str) -> Option<Class> {
        Class::ALL.into_iter().find(|c| c.id() == id)
    }
}

/// What a provider of models does with what it receives (ADR-004 §2): what decides, not whether it is paid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conditions {
    /// It does not train on what it receives, deletes it and keeps it confidential (LFPDPPP; its Regulation, art. 52).
    Processor,
    /// Nobody has checked yet what it does: every new provider starts here.
    Unverified,
}

/// The provider a model call goes to: its id and its conditions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    pub id: String,
    pub conditions: Conditions,
}

/// Where a datum goes (ADR-004 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination<'a> {
    /// A model, by the conditions of its provider.
    Model(Conditions),
    /// The screen or the chat of the person, who sees the classes of their role (K2).
    Person { sees: &'a [&'a str] },
    /// A notice: it says what happened; the detail is seen opening the task.
    Notice,
    /// The audit store: ids and codes only.
    Audit,
}

/// What a datum is for, declared by whoever asks (an operator, A6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// The default one.
    Answer,
    /// The router: it needs the intention, never who.
    Route,
    /// A text where a person appears: it needs the character, not their data.
    Write,
}

/// What the policy says of a datum, from the strictest to the most open: the order is the one the strictest of several
/// classes takes (`min`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    Never,
    /// Only counted, three or more («cuántos, nunca quiénes»).
    OnlyCounted,
    /// Covered by a mark of the vault, and restored only toward the person.
    Covered,
    Goes,
}

/// A count of three or more is already disassociated (LFPDPPP art. 9 fr. III), and counts as internal (ADR-004 §5.2).
pub const COUNTED_FROM: u64 = 3;

/// The one decision (ADR-004 §4).
pub fn decide(class: Class, to: Destination, purpose: Purpose) -> Verdict {
    use Class::*;
    use Verdict::*;
    match to {
        Destination::Person { sees } => {
            if sees.contains(&class.id()) {
                Goes
            } else {
                Never
            }
        }
        Destination::Notice => match class {
            Public | Internal => Goes,
            Confidential | Personal | SensitivePersonal => Never,
        },
        Destination::Audit => Never,
        Destination::Model(Conditions::Processor) => match (class, purpose) {
            (Public | Internal, _) => Goes,
            // only the rules read it
            (Confidential, _) => Never,
            (Personal, Purpose::Route | Purpose::Write) => Covered,
            (Personal, Purpose::Answer) | (SensitivePersonal, _) => OnlyCounted,
        },
        Destination::Model(Conditions::Unverified) => match class {
            Public => Goes,
            Internal | Confidential | Personal | SensitivePersonal => Never,
        },
    }
}

/// Whether an account that sees these classes (K2) may run a capability that touches these (its `data`): only if it
/// sees every one of them (ADR-004 §6, the execution).
pub fn may_touch(data: &[DataClass], sees: &[&str]) -> bool {
    data.iter().all(|c| sees.contains(&c.id()))
}

/// A count of data of a class: from three on it counts as internal; below, it keeps its class, and goes only if its
/// class goes as it is.
pub fn decide_count(class: Class, n: u64, to: Destination, purpose: Purpose) -> Verdict {
    if n >= COUNTED_FROM {
        return decide(Class::Internal, to, purpose);
    }
    match decide(class, to, purpose) {
        Verdict::Goes => Verdict::Goes,
        _ => Verdict::Never,
    }
}

/// A datum that names no field of the dictionary: the strictest of the classes its capability touches (its `data`)
/// decides (ADR-004 §5.3). A capability that declares none has nothing to give.
pub fn decide_unnamed(data: &[DataClass], to: Destination, purpose: Purpose) -> Verdict {
    data.iter()
        .filter_map(|c| Class::from_id(c.id()))
        .map(|c| decide(c, to, purpose))
        .min()
        .unwrap_or(Verdict::Never)
}

/// The class of what the detector finds in a text, which brings none (ADR-004 §5.6).
pub fn class_of(kind: FindingKind) -> Class {
    match kind {
        FindingKind::PersonName
        | FindingKind::Email
        | FindingKind::Phone
        | FindingKind::Curp
        | FindingKind::RfcPerson
        | FindingKind::VoterKey
        | FindingKind::Nss
        | FindingKind::Card => Class::Personal,
        FindingKind::RfcCompany | FindingKind::Clabe => Class::Confidential,
        FindingKind::HealthContext => Class::SensitivePersonal,
    }
}

/// The detector of the platform: no «safe phrases» and nothing of the institution, because it finds and never
/// decides.
fn detector() -> &'static RegexScanner {
    static D: OnceLock<RegexScanner> = OnceLock::new();
    D.get_or_init(|| RegexScanner::new(ScannerConfig::default()))
}

/// A datum on its way to a model, as whoever gathers it has it.
#[derive(Debug, Clone)]
pub enum Datum {
    /// A value of a field of the dictionary, with the class the dictionary gives it.
    Field { id: String, class: Class, value: Value },
    /// What the person wrote: it brings no class, the detector looks at it.
    Text(String),
    /// How many of something there are, and of what class they are.
    Count { what: String, class: Class, n: u64 },
}

/// A part of what a model receives, as the policy let it go.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Part {
    Field { id: String, value: Value },
    Text { text: String },
    Count { what: String, n: u64 },
}

/// What a model receives (P6 and P7): only the policy builds it, so no model is called with anything else.
///
/// Its fields are private and it has no other constructor than [`to_model`]:
///
/// ```compile_fail
/// let _ = platform::policy::ToModel { provider: String::new(), parts: vec![] };
/// ```
pub struct ToModel {
    provider: String,
    purpose: Purpose,
    parts: Vec<Part>,
    left_out: Vec<String>,
    // the values behind the marks, in memory only and only while the step lasts (ADR-004 §5.4)
    vault: Vault,
}

impl ToModel {
    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn purpose(&self) -> Purpose {
        self.purpose
    }

    /// What the model reads.
    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    /// What did not go, by the id of its field (`text` for what the person wrote, `count:` and what for a count):
    /// never its value.
    pub fn left_out(&self) -> &[String] {
        &self.left_out
    }

    /// What the model is told of the marks: what each one is, never its value.
    pub fn legend(&self) -> Option<String> {
        self.vault.legend()
    }

    /// An answer of the model with each mark as its value: only for the person, never for a notice or the audit.
    pub fn restore(&self, text: &str) -> String {
        self.vault.reveal(text)
    }
}

/// What a model of this provider receives of these data, for this purpose: each datum goes, goes covered, or stays.
pub fn to_model(provider: &Provider, purpose: Purpose, data: Vec<Datum>) -> ToModel {
    let to = Destination::Model(provider.conditions);
    let vault = Vault::new();
    let mut parts = Vec::new();
    let mut left_out = Vec::new();
    for datum in data {
        match datum {
            Datum::Field { id, class, value } => match decide(class, to, purpose) {
                Verdict::Goes => parts.push(Part::Field { id, value }),
                Verdict::Covered => {
                    let text = match &value {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    let value = Value::String(vault.cover(&text, &detector().scan(&text)));
                    parts.push(Part::Field { id, value })
                }
                Verdict::OnlyCounted | Verdict::Never => left_out.push(id),
            },
            Datum::Text(text) => {
                let found = detector().scan(&text);
                let verdicts: Vec<Verdict> = found.findings.iter().map(|f| decide(class_of(f.kind), to, purpose)).collect();
                // something in it may not go even covered: the whole text stays, because the detector also misses
                if verdicts.iter().any(|v| *v < Verdict::Covered) {
                    left_out.push("text".into());
                } else if verdicts.contains(&Verdict::Covered) {
                    parts.push(Part::Text { text: vault.mark(&text, &found) });
                } else {
                    parts.push(Part::Text { text });
                }
            }
            Datum::Count { what, class, n } => match decide_count(class, n, to, purpose) {
                Verdict::Goes => parts.push(Part::Count { what, n }),
                _ => left_out.push(format!("count:{what}")),
            },
        }
    }
    ToModel { provider: provider.id.clone(), purpose, parts, left_out, vault }
}

/// What an outcome hands over of the output of a capability, field by field (ADR-004 §6, the output): what may not
/// go to that destination is taken out, and the paths taken out are told (`withheld`), never their values. A datum
/// that names no field is decided by the strictest class of the capability.
pub fn hand_over(data: &[DataClass], fields: &[OutputField], value: Value, to: Destination) -> (Value, Vec<String>) {
    let unnamed = decide_unnamed(data, to, Purpose::Answer);
    let mut withheld = Vec::new();
    let value = keep(value, "", fields, unnamed, to, &mut withheld);
    (value.unwrap_or(Value::Null), withheld)
}

fn keep(value: Value, path: &str, fields: &[OutputField], unnamed: Verdict, to: Destination, withheld: &mut Vec<String>) -> Option<Value> {
    if let Some(field) = fields.iter().find(|f| f.path == path) {
        let goes = Class::from_id(field.class).is_some_and(|c| decide(c, to, Purpose::Answer) == Verdict::Goes);
        if !goes && !withheld.contains(&field.path) {
            withheld.push(field.path.clone());
        }
        return goes.then_some(value);
    }
    match value {
        Value::Object(map) => {
            let kept = map.into_iter().filter_map(|(k, v)| {
                let inner = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                keep(v, &inner, fields, unnamed, to, withheld).map(|v| (k, v))
            });
            Some(Value::Object(kept.collect()))
        }
        Value::Array(items) => {
            let inner = format!("{path}[]");
            Some(Value::Array(items.into_iter().filter_map(|v| keep(v, &inner, fields, unnamed, to, withheld)).collect()))
        }
        leaf if unnamed == Verdict::Goes => Some(leaf),
        _ => {
            let at = if path.is_empty() { "output".to_string() } else { path.to_string() };
            if !withheld.contains(&at) {
                withheld.push(at);
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use Class::*;
    use Verdict::*;

    const ALL_SEEN: &[&str] = &["public", "internal", "confidential", "personal", "sensitive_personal"];

    /// The table of ADR-004 §4, written once more by hand: a change of the decision has to change it here too.
    #[test]
    fn the_table_of_the_adr() {
        let processor = Destination::Model(Conditions::Processor);
        let unverified = Destination::Model(Conditions::Unverified);
        let rows = [
            // class, processor answering, processor routing, processor writing, unverified, notice
            (Public, Goes, Goes, Goes, Goes, Goes),
            (Internal, Goes, Goes, Goes, Never, Goes),
            (Confidential, Never, Never, Never, Never, Never),
            (Personal, OnlyCounted, Covered, Covered, Never, Never),
            (SensitivePersonal, OnlyCounted, OnlyCounted, OnlyCounted, Never, Never),
        ];
        for (class, answer, route, write, unverified_any, notice) in rows {
            assert_eq!(decide(class, processor, Purpose::Answer), answer, "{class:?}");
            assert_eq!(decide(class, processor, Purpose::Route), route, "{class:?}");
            assert_eq!(decide(class, processor, Purpose::Write), write, "{class:?}");
            for purpose in [Purpose::Answer, Purpose::Route, Purpose::Write] {
                assert_eq!(decide(class, unverified, purpose), unverified_any, "{class:?}");
                assert_eq!(decide(class, Destination::Notice, purpose), notice, "{class:?}");
                assert_eq!(decide(class, Destination::Audit, purpose), Never, "the audit never keeps a value");
                assert_eq!(decide(class, Destination::Person { sees: ALL_SEEN }, purpose), Goes);
            }
        }
    }

    #[test]
    fn the_person_sees_the_classes_of_their_role() {
        let sees = &["public", "internal"];
        assert_eq!(decide(Internal, Destination::Person { sees }, Purpose::Answer), Goes);
        assert_eq!(decide(Personal, Destination::Person { sees }, Purpose::Answer), Never);
        assert_eq!(decide(Public, Destination::Person { sees: &[] }, Purpose::Answer), Never);
    }

    #[test]
    fn three_or_more_count_as_internal() {
        let processor = Destination::Model(Conditions::Processor);
        let unverified = Destination::Model(Conditions::Unverified);
        assert_eq!(decide_count(SensitivePersonal, 3, processor, Purpose::Answer), Goes);
        assert_eq!(decide_count(SensitivePersonal, 2, processor, Purpose::Answer), Never, "fewer than three say who");
        assert_eq!(decide_count(Personal, 2, processor, Purpose::Route), Never, "a count is never covered");
        assert_eq!(decide_count(Internal, 1, processor, Purpose::Answer), Goes);
        // a count is internal: an unverified provider does not get it either
        assert_eq!(decide_count(Personal, 10, unverified, Purpose::Answer), Never);
        assert_eq!(decide_count(Public, 1, unverified, Purpose::Answer), Goes);
    }

    #[test]
    fn the_classes_are_the_ones_of_the_dictionary() {
        let ids: Vec<&str> = Class::ALL.iter().map(|c| c.id()).collect();
        assert_eq!(ids, kernel::CLASSES);
        assert!(Class::ALL.iter().all(|c| Class::from_id(c.id()) == Some(*c)));
        assert_eq!(Class::from_id("secret"), None);
    }

    fn processor() -> Provider {
        Provider { id: "anthropic".into(), conditions: Conditions::Processor }
    }

    fn unverified() -> Provider {
        Provider { id: "nuevo".into(), conditions: Conditions::Unverified }
    }

    fn field(id: &str, class: Class, value: &str) -> Datum {
        Datum::Field { id: id.into(), class, value: json!(value) }
    }

    /// The data of a fictitious institution, as a context manager would gather them.
    fn gathered() -> Vec<Datum> {
        vec![
            field("demo.org.name", Public, "Casa Hogar Demo"),
            field("demo.org.mission", Internal, "Acompañar a niñas y niños"),
            field("demo.org.rfc", Confidential, "CHD010101AB1"),
            field("demo.org.legal_rep", Personal, "Ana Ejemplo Ficticia"),
            Datum::Count { what: "beneficiaries_with_a_diagnosis".into(), class: SensitivePersonal, n: 4 },
            Datum::Count { what: "staff_on_leave".into(), class: Personal, n: 2 },
        ]
    }

    fn ids(m: &ToModel) -> Vec<String> {
        m.parts()
            .iter()
            .map(|p| match p {
                Part::Field { id, .. } => id.clone(),
                Part::Text { .. } => "text".into(),
                Part::Count { what, .. } => format!("count:{what}"),
            })
            .collect()
    }

    #[test]
    fn an_unverified_provider_receives_only_the_public() {
        let m = to_model(&unverified(), Purpose::Route, gathered());
        assert_eq!(ids(&m), ["demo.org.name"]);
        assert_eq!(
            m.left_out(),
            ["demo.org.mission", "demo.org.rfc", "demo.org.legal_rep", "count:beneficiaries_with_a_diagnosis", "count:staff_on_leave"]
        );
        // what the person wrote, with the data of a person in it, does not go at all
        let m = to_model(&unverified(), Purpose::Route, vec![Datum::Text("Llamen a Ana al 55 1234 5678".into())]);
        assert!(m.parts().is_empty() && m.left_out() == ["text"], "{:?}", m.parts());
        let m = to_model(&unverified(), Purpose::Route, vec![Datum::Text("¿Qué me falta?".into())]);
        assert_eq!(m.parts(), [Part::Text { text: "¿Qué me falta?".into() }]);
    }

    #[test]
    fn a_processor_receives_what_the_table_says() {
        let m = to_model(&processor(), Purpose::Answer, gathered());
        assert_eq!(ids(&m), ["demo.org.name", "demo.org.mission", "count:beneficiaries_with_a_diagnosis"]);
        assert_eq!(m.left_out(), ["demo.org.rfc", "demo.org.legal_rep", "count:staff_on_leave"]);
        assert_eq!(m.provider(), "anthropic");
        assert_eq!(m.legend(), None, "nothing covered, nothing to tell");
    }

    #[test]
    fn what_goes_covered_comes_back_only_toward_the_person() {
        let m = to_model(&processor(), Purpose::Write, gathered());
        let rep = m.parts().iter().find_map(|p| match p {
            Part::Field { id, value } if id == "demo.org.legal_rep" => Some(value.clone()),
            _ => None,
        });
        let mark = rep.and_then(|v| v.as_str().map(String::from)).expect("the representative goes covered");
        assert!(!mark.contains("Ana"), "{mark}");
        assert!(m.legend().unwrap().contains(&mark));
        assert_eq!(m.restore(&format!("Firma: {mark}")), "Firma: Ana Ejemplo Ficticia");
        assert!(m.left_out().contains(&"demo.org.rfc".to_string()), "the RFC never goes, not even covered");

        let m = to_model(&processor(), Purpose::Route, vec![Datum::Text("Cambien el teléfono a 55 1234 5678".into())]);
        let [Part::Text { text }] = m.parts() else { panic!("{:?}", m.parts()) };
        assert!(!text.contains("1234") && text.contains("[TELEFONO_1]"), "{text}");
        assert_eq!(m.restore(text), "Cambien el teléfono a 55 1234 5678");
        // the RFC of an organization may not go even covered: the whole text stays
        let m = to_model(&processor(), Purpose::Route, vec![Datum::Text("Nuestro RFC es CHD010101AB1".into())]);
        assert_eq!(m.left_out(), ["text"]);
    }

    fn named(path: &str, class: &'static str) -> OutputField {
        OutputField { path: path.into(), field: format!("demo.org.{path}"), class }
    }

    #[test]
    fn an_output_is_handed_over_field_by_field() {
        let fields = [named("name", "public"), named("contacts[].phone", "personal")];
        let value = json!({ "id": "org_1", "name": "Casa Hogar Demo", "contacts": [{ "phone": "55 1234 5678", "since": 2020 }] });
        let data = [DataClass::Public, DataClass::Internal, DataClass::Personal];

        let everything = Destination::Person { sees: &["public", "internal", "confidential", "personal", "sensitive_personal"] };
        assert_eq!(hand_over(&data, &fields, value.clone(), everything), (value.clone(), vec![]));

        // a role that does not see what is personal: the phone is taken out, and so is what names no field, which is
        // as strict as the strictest class of the capability
        let some = Destination::Person { sees: &["public", "internal"] };
        let (kept, mut withheld) = hand_over(&data, &fields, value.clone(), some);
        withheld.sort();
        assert_eq!(kept, json!({ "name": "Casa Hogar Demo", "contacts": [{}] }));
        assert_eq!(withheld, ["contacts[].phone", "contacts[].since", "id"]);

        let (kept, withheld) = hand_over(&[DataClass::Public, DataClass::Internal], &fields[..1], json!({ "id": "x", "name": "N" }), Destination::Notice);
        assert_eq!((kept, withheld), (json!({ "id": "x", "name": "N" }), vec![]));
        let (kept, _) = hand_over(&data, &fields, value, Destination::Audit);
        assert_eq!(kept, json!({ "contacts": [{}] }), "the audit keeps no value");
    }

    #[test]
    fn a_capability_runs_only_for_who_sees_all_its_classes() {
        let data = [DataClass::Internal, DataClass::Personal];
        assert!(may_touch(&data, ALL_SEEN));
        assert!(!may_touch(&data, &["public", "internal"]));
        assert!(may_touch(&[], &[]), "a capability that touches no data");
    }

    #[test]
    fn what_the_detector_finds_takes_its_class() {
        assert_eq!(class_of(FindingKind::PersonName), Personal);
        assert_eq!(class_of(FindingKind::RfcCompany), Confidential);
        assert_eq!(class_of(FindingKind::HealthContext), SensitivePersonal);
    }
}
