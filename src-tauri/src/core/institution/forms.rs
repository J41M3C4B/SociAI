//! The forms of the institution described as data (ADR-033 §1): what «Mi institución» asks, field by field. The
//! screen draws them; the words live in `src/i18n/es-MX.ts` under `forms.fields` with the same id.
//!
//! The four windows of the profile are not written here: they are the module `mi_institucion` (ERP-1), its windows
//! over its entity of the dictionary. The screen keeps seeing them as forms, with a field named `institution.` and its
//! id in the dictionary (`institution.legal_name`). The lists of the institution (its board, its accounts and its
//! record) are still described here.

use std::sync::LazyLock;

use super::catalog::{BANK_PURPOSES, BOARD_ROLES, RECORD_STATUSES};
use crate::common::forms::{AiUse, Condition, FieldKind, FieldSpec, FormSpec, Rule, SectionSpec, Sensitivity};
use mi_institucion::catalog::PROFILE;
use mi_institucion::windows::{ESSENTIAL, WINDOWS};

/// The institution's own and protected: never to the AI (its contact, address, folios and keys).
const fn private(f: FieldSpec) -> FieldSpec {
    f.sensitivity(Sensitivity::InstitutionalPrivate, AiUse::Never)
}

/// The institution's own, not personal, that may reach the AI as it is (its figures, its regime).
const fn internal(f: FieldSpec) -> FieldSpec {
    f.sensitivity(Sensitivity::Internal, AiUse::AsIs)
}

// ------------------------------------------------------------------ the profile, from the module (ERP-1)

/// How the screen names a field of the profile: `institution.` and its id in the dictionary.
pub const PREFIX: &str = "institution.";

/// The id of a field of the profile on the screen (`legal_name` → `institution.legal_name`).
pub fn screen_id(field: &str) -> String {
    format!("{PREFIX}{field}")
}

/// The id in the dictionary of a field of the profile on the screen (`institution.legal_name` → `legal_name`).
pub fn module_id(id: &str) -> Option<&str> {
    id.strip_prefix(PREFIX).filter(|f| PROFILE.field(f).is_some())
}

/// The id of a window of the profile on the screen (`legal` → `institution.legal`).
pub fn screen_form(window: &str) -> String {
    format!("{PREFIX}{window}")
}

/// Who uses each datum of the profile: the «what is it for» of the manual. It is about the program, not the datum, so
/// it is not in the dictionary.
const USED_BY: &[(&str, &[&str])] = &[
    ("name", &["documents", "projects", "ai"]),
    ("mission", &["projects", "documents", "ai"]),
    ("populations", &["care", "hr", "facilities", "projects", "ai"]),
    ("sex_served", &["care", "projects", "ai"]),
    ("modalities", &["care", "facilities", "projects", "ai"]),
    ("care_areas", &["care", "hr", "projects", "ai"]),
    ("services", &["projects", "care", "ai"]),
    ("age_min", &["care", "projects", "ai"]),
    ("age_max", &["care", "projects", "ai"]),
    ("admission_criteria", &["care", "projects", "ai"]),
    ("contact_phone", &["documents"]),
    ("contact_email", &["documents"]),
    ("street", &["documents", "finance"]),
    ("ext_number", &["documents", "finance"]),
    ("int_number", &["documents", "finance"]),
    ("neighborhood", &["documents", "finance"]),
    ("postal_code", &["documents", "finance"]),
    ("municipality", &["hr", "finance", "projects", "ai"]),
    ("state", &["hr", "finance", "projects", "ai"]),
    ("legal_name", &["documents", "finance", "projects", "ai"]),
    ("purpose", &["projects", "ai"]),
    ("legal_form", &["finance", "projects", "ai"]),
    ("founded_year", &["projects", "ai"]),
    ("junta_folio", &["finance", "documents"]),
    ("legal_rfc", &["documents", "finance"]),
    ("tax_regime", &["finance", "projects", "ai"]),
    ("fiscal_postal_code", &["finance", "documents"]),
    ("authorized_donee", &["finance", "projects", "ai"]),
    ("cluni", &["projects", "ai"]),
    ("donee_category", &["finance", "projects", "ai"]),
    ("donee_letter_number", &["finance", "documents"]),
    ("donee_letter_date", &["finance", "documents"]),
    ("cluni_key", &["projects", "documents"]),
    ("legal_rep_name", &["documents", "projects"]),
    ("legal_rep_valid_until", &["documents", "projects"]),
    ("capacity_total", &["care", "facilities", "projects", "ai"]),
    ("served_estimate", &["care", "projects", "ai"]),
    ("served_last_year", &["projects", "ai"]),
    ("staff_paid_estimate", &["hr", "projects", "ai"]),
    ("staff_volunteer_estimate", &["hr", "projects", "ai"]),
    ("notes", &["projects", "ai"]),
];

/// A name the program keeps for as long as it runs: the forms of the profile are built once, from the module.
fn kept(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// A field of the dictionary as the screen draws it. What is not public nor internal never reaches the AI.
fn spec_of(f: &kernel::Field) -> FieldSpec {
    let kind = match f.kind {
        kernel::Kind::Text => FieldKind::Text,
        kernel::Kind::LongText => FieldKind::LongText,
        kernel::Kind::Number => FieldKind::Number,
        kernel::Kind::Money => FieldKind::Money,
        kernel::Kind::Year => FieldKind::Year,
        kernel::Kind::Date => FieldKind::Date,
        kernel::Kind::Select => FieldKind::Select,
        kernel::Kind::MultiSelect => FieldKind::MultiSelect,
        kernel::Kind::YesNo => FieldKind::YesNo,
        kernel::Kind::Email => FieldKind::Email,
        kernel::Kind::Phone => FieldKind::Phone,
    };
    let (sensitivity, ai) = match f.class {
        "public" => (Sensitivity::Public, AiUse::AsIs),
        "internal" => (Sensitivity::Internal, AiUse::AsIs),
        "confidential" => (Sensitivity::InstitutionalPrivate, AiUse::Never),
        _ => (Sensitivity::Personal, AiUse::Never),
    };
    let applies_when = match f.applies_when {
        kernel::Condition::Always => Condition::Always,
        kernel::Condition::Filled { field } => Condition::Filled { field: kept(screen_id(field)) },
        kernel::Condition::AnyOf { field, values } => Condition::AnyOf { field: kept(screen_id(field)), values },
    };
    FieldSpec {
        id: kept(screen_id(f.id)),
        kind,
        options: f.options,
        required: f.presence != kernel::Presence::Optional,
        essential: ESSENTIAL.contains(&f.id),
        applies_when,
        sensitivity,
        ai,
        used_by: USED_BY.iter().find(|(id, _)| *id == f.id).map_or(&[], |(_, by)| by),
        min: f.min,
        max: f.max,
        rule: f.rule.map(|r| match r {
            kernel::Rule::PostalCode => Rule::PostalCode,
            kernel::Rule::RfcMoral => Rule::RfcMoral,
            kernel::Rule::Clabe => Rule::Clabe,
            kernel::Rule::PlaceName => Rule::PlaceName,
        }),
    }
}

/// The windows of the profile as forms, built once.
static PROFILE_FORMS: LazyLock<Vec<&'static FormSpec>> = LazyLock::new(|| {
    WINDOWS
        .iter()
        .map(|w| {
            let sections: Vec<SectionSpec> = w
                .sections
                .iter()
                .map(|s| SectionSpec {
                    id: s.id,
                    columns: s.columns,
                    fields: Box::leak(s.fields.iter().map(|f| spec_of(PROFILE.field(f).expect("a window shows fields of the catalog"))).collect()),
                })
                .collect();
            &*Box::leak(Box::new(FormSpec { id: kept(screen_form(w.id)), sections: Box::leak(sections.into_boxed_slice()) }))
        })
        .collect()
});

/// Every window of the profile, as a form: `institution.identity`, `institution.contact`, `institution.legal` and
/// `institution.capacity`.
pub fn forms() -> &'static [&'static FormSpec] {
    &PROFILE_FORMS
}

pub fn form(id: &str) -> Option<&'static FormSpec> {
    forms().iter().copied().find(|f| f.id == id)
}

// ------------------------------------------------------------------ the lists of the institution (ADR-033 §3)

/// «Patronato u órgano de gobierno», one form per person. The names never reach the AI: it learns how many people
/// there are and which roles (`core::protected`).
pub const BOARD_MEMBER: FormSpec = FormSpec {
    id: "institution.board_member",
    sections: &[SectionSpec {
        id: "board_member",
        columns: 2,
        fields: &[
            private(FieldSpec::new("institution.board.name", FieldKind::Text)).required().used_by(&["documents"]),
            internal(FieldSpec::new("institution.board.role", FieldKind::Select)).options(BOARD_ROLES).required().used_by(&["documents", "ai"]),
            private(FieldSpec::new("institution.board.role_other", FieldKind::Text))
                .required()
                .when(Condition::AnyOf { field: "institution.board.role", values: &["other"] })
                .used_by(&["documents"]),
            private(FieldSpec::new("institution.board.since", FieldKind::Date)).used_by(&["documents"]),
        ],
    }],
};

/// The CLABE of an account: asked once, when the account is added. To change it, the account is closed and another
/// one is added; afterwards it is only shown covered.
const BANK_CLABE: SectionSpec = SectionSpec {
    id: "bank_clabe",
    columns: 1,
    fields: &[private(FieldSpec::new("institution.bank.clabe", FieldKind::Text)).rule(Rule::Clabe).required().used_by(&["documents", "finance"])],
};

/// The bank (read from the CLABE when it is left empty) and what the account is for.
const BANK_USE: SectionSpec = SectionSpec {
    id: "bank_use",
    columns: 1,
    fields: &[
        private(FieldSpec::new("institution.bank.bank", FieldKind::Text)).used_by(&["documents", "finance"]),
        private(FieldSpec::new("institution.bank.purpose", FieldKind::MultiSelect)).options(BANK_PURPOSES).required().used_by(&["documents", "finance"]),
        private(FieldSpec::new("institution.bank.in_documents", FieldKind::YesNo)).used_by(&["documents"]),
    ],
};

/// «Cuentas bancarias» of the institution, one form per account: never to the AI, shown covered.
pub const BANK_ACCOUNT: FormSpec = FormSpec { id: "institution.bank_account", sections: &[BANK_CLABE, BANK_USE] };

/// The same account once it is saved: its CLABE is no longer asked (`BANK_CLABE`).
pub const BANK_ACCOUNT_SAVED: FormSpec = FormSpec { id: "institution.bank_account", sections: &[BANK_USE] };

/// What applies once the institution has the document.
const HAS_IT: Condition = Condition::AnyOf { field: "institution.record.status", values: &["have"] };
/// What applies while it is in process or once it has it.
const UNDER_WAY: Condition = Condition::AnyOf { field: "institution.record.status", values: &["have", "in_progress"] };

const RECORD_NAME: SectionSpec = SectionSpec {
    id: "record_name",
    columns: 1,
    fields: &[internal(FieldSpec::new("institution.record.custom_name", FieldKind::Text)).required().used_by(&["projects", "ai"])],
};
const RECORD_STATE: SectionSpec = SectionSpec {
    id: "record_state",
    columns: 1,
    fields: &[internal(FieldSpec::new("institution.record.status", FieldKind::Select)).options(RECORD_STATUSES).required().used_by(&["projects", "ai"])],
};
const RECORD_DATES: SectionSpec = SectionSpec {
    id: "record_dates",
    columns: 2,
    fields: &[
        internal(FieldSpec::new("institution.record.issued_on", FieldKind::Date)).when(HAS_IT).used_by(&["projects", "ai"]),
        internal(FieldSpec::new("institution.record.valid_until", FieldKind::Date)).when(HAS_IT).used_by(&["projects", "ai"]),
        private(FieldSpec::new("institution.record.reference", FieldKind::Text)).when(UNDER_WAY).used_by(&["documents"]),
    ],
};
const RECORD_NOTES: SectionSpec = SectionSpec {
    id: "record_notes",
    columns: 1,
    fields: &[private(FieldSpec::new("institution.record.notes", FieldKind::LongText))],
};

/// A document of the record (ADR-033 §5) the institution names itself (`other`): its name, then what every document
/// asks.
pub const RECORD: FormSpec = FormSpec { id: "institution.record", sections: &[RECORD_NAME, RECORD_STATE, RECORD_DATES, RECORD_NOTES] };

/// A document of a type of the catalog: its name comes from the type.
pub const RECORD_KNOWN: FormSpec = FormSpec { id: "institution.record", sections: &[RECORD_STATE, RECORD_DATES, RECORD_NOTES] };

/// The forms of the lists, one row each: they do not belong to the profile, so they are kept apart from `forms()`.
pub const LIST_FORMS: &[&FormSpec] = &[&BOARD_MEMBER, &BANK_ACCOUNT, &RECORD];

/// The words of the screen (`src/i18n/es-MX.ts`, the one source of the words), read by the program too: the AI is
/// shown each code of a field with its name, so it can tell that «ASISTENCIAL» in a document is `assistance`.
const SCREEN_WORDS: &str = include_str!("../../../../src/i18n/es-MX.ts");

/// The words of the screens (`es-MX.ts`), as they are.
pub(crate) fn screen_words() -> &'static str {
    SCREEN_WORDS
}

/// The name of a type of document of the Expediente on the screen (`articles_of_incorporation` → «Acta constitutiva»),
/// from `records.types` of the words of the screen.
pub fn record_type_label(code: &str) -> Option<&'static str> {
    let es = SCREEN_WORDS;
    let block = &es[es.find("records: {")?..];
    let types = &block[block.find("types: {")? + "types: {".len()..];
    let types = &types[..types.find('}')?];
    types.lines().find_map(|l| {
        let (k, v) = l.trim().split_once(':')?;
        (k.trim() == code).then(|| v.trim().trim_end_matches(',').trim_matches('"'))
    })
}

/// The name of a code of a field on the screen («assistance» of «Rubro autorizado» → «Asistencial»), from the entry of
/// the field in `forms.fields` or from the shared list it names (`options: STATES`).
pub fn option_label(field: &str, code: &str) -> Option<&'static str> {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static LABELS: OnceLock<HashMap<String, HashMap<String, &'static str>>> = OnceLock::new();
    let labels = LABELS.get_or_init(|| {
        let es = SCREEN_WORDS;
        let Some(start) = es.find("forms: {") else { return HashMap::new() };
        let block = &es[start..];
        // `code: "Name"`, one per line or several on one line, inside the options of the field
        let pair = regex::Regex::new(r#""?\b([a-z0-9_]+)"?:\s*"([^"]+)""#).unwrap();
        let mut out = HashMap::new();
        for f in forms().iter().chain(LIST_FORMS).flat_map(|f| f.fields()).filter(|f| !f.options.is_empty()) {
            let head = format!("\"{}\": {{", f.id);
            let Some(at) = block.find(&head) else { continue };
            let rest = &block[at + head.len()..];
            let entry = &rest[..rest.find("\n      \"").or_else(|| rest.find("\n    }")).unwrap_or(rest.len())];
            let Some(options) = entry.split("options: ").nth(1).map(str::trim_start) else { continue };
            let words: &str = if let Some(inner) = options.strip_prefix('{') {
                &inner[..inner.find('}').unwrap_or(inner.len())]
            } else {
                let name: String = options.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                let Some(from) = es.find(&format!("const {name}")) else { continue };
                &es[from..from + es[from..].find("};").unwrap_or(es.len() - from)]
            };
            let codes: HashMap<String, &'static str> = pair.captures_iter(words).map(|c| (c[1].to_string(), c.get(2).unwrap().as_str())).collect();
            out.insert(f.id.to_string(), codes);
        }
        out
    });
    labels.get(field)?.get(code).copied()
}

/// A field of any form of the core, of the profile or of its lists.
pub fn field(id: &str) -> Option<&'static FieldSpec> {
    forms().iter().chain(LIST_FORMS).find_map(|f| f.field(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every field has its words in the screen (ADR-033 §1): a label under `forms.fields` with its id, and a label
    /// for each of its codes, in its own entry or in the shared list it names (`options: STATES`).
    /// The AI knows a document by its type, with the name the screen gives it: every type of the catalog has one.
    #[test]
    fn every_type_of_the_expediente_has_its_name_on_the_screen() {
        assert_eq!(record_type_label("articles_of_incorporation"), Some("Acta constitutiva"));
        assert_eq!(record_type_label("tax_compliance_opinion"), Some("Opinión de cumplimiento (32-D)"));
        for (code, _) in crate::core::institution::catalog::RECORD_TYPES {
            assert!(record_type_label(code).is_some_and(|l| !l.is_empty() && !l.contains('"')), "{code}");
        }
        assert_eq!(record_type_label("nope"), None);
    }

    #[test]
    fn the_ai_is_shown_each_code_with_its_name_on_the_screen() {
        assert_eq!(option_label("institution.donee_category", "assistance"), Some("Asistencial"));
        assert_eq!(option_label("institution.legal_form", "iap"), option_label("institution.legal_form", "iap").filter(|l| !l.is_empty()));
        for f in forms().iter().flat_map(|f| f.fields()).filter(|f| f.id != "institution.state") {
            for code in f.options {
                assert!(option_label(f.id, code).is_some(), "{}: the code {code} has no name for the AI", f.id);
            }
        }
        assert_eq!(option_label("institution.legal_form", "nope"), None);
    }

    #[test]
    fn every_field_of_every_form_has_its_words() {
        let es = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/i18n/es-MX.ts")).unwrap();
        let start = es.find("forms: {").expect("es-MX.ts has a `forms` block");
        let block = &es[start..];
        for f in forms().iter().chain(LIST_FORMS).flat_map(|f| f.fields()) {
            let head = format!("\"{}\": {{", f.id);
            let at = block.find(&head).unwrap_or_else(|| panic!("{} has no words in es-MX.ts (forms.fields)", f.id));
            let rest = &block[at + head.len()..];
            let entry = &rest[..rest.find("\n      \"").or_else(|| rest.find("\n    }")).unwrap_or(rest.len())];
            if f.options.is_empty() {
                continue;
            }
            // the codes are in the entry, or in a list of their own the entry names
            let shared = entry.split("options: ").nth(1).map(|x| x.trim_start()).filter(|x| !x.starts_with('{')).map(|x| {
                let name: String = x.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                let decl = format!("const {name}");
                let from = es.find(&decl).unwrap_or_else(|| panic!("{}: the list {name} is not declared in es-MX.ts", f.id));
                &es[from..from + es[from..].find("};").unwrap_or(es.len() - from)]
            });
            let words = shared.unwrap_or(entry);
            for code in f.options {
                assert!(words.contains(&format!("{code}: ")) || words.contains(&format!("\"{code}\": ")), "{}: the code {code} has no label", f.id);
            }
        }
    }

    #[test]
    fn the_forms_have_unique_ids() {
        let ids: Vec<&str> = forms().iter().chain(LIST_FORMS).flat_map(|f| f.fields()).map(|f| f.id).collect();
        assert!((1..ids.len()).all(|i| !ids[..i].contains(&ids[i])), "{ids:?}");
        assert!(form("institution.identity").is_some() && form("nope").is_none());
        assert!(form("institution.board_member").is_none(), "a list is not a window of the profile");
        assert!(field("institution.board.name").is_some() && field("institution.legal_rfc").is_some() && field("nope").is_none());
        // the saved account asks the same, but its CLABE
        assert!(BANK_ACCOUNT_SAVED.fields().all(|f| BANK_ACCOUNT.field(f.id) == Some(f)) && BANK_ACCOUNT_SAVED.field("institution.bank.clabe").is_none());
        // a document of the catalog asks the same as one the institution names, but its name
        assert!(RECORD_KNOWN.fields().all(|f| RECORD.field(f.id) == Some(f)) && RECORD_KNOWN.field("institution.record.custom_name").is_none());
    }

    /// The windows of the profile are the module's, field by field: what it asks, when and how protected, and the
    /// «what is it for» of each one.
    #[test]
    fn the_windows_of_the_profile_are_the_modules() {
        let ids: Vec<&str> = forms().iter().map(|f| f.id).collect();
        assert_eq!(ids, ["institution.identity", "institution.contact", "institution.legal", "institution.capacity"]);
        let capacity = field("institution.capacity_total").unwrap();
        assert!(capacity.required && capacity.essential);
        assert_eq!(capacity.applies_when.field(), Some("institution.modalities"), "a condition looks at the screen's id");
        assert!(!field("institution.services").unwrap().required);
        assert_eq!(module_id("institution.legal_rfc"), Some("legal_rfc"));
        assert_eq!(module_id("institution.board.name"), None);
        for f in forms().iter().flat_map(|f| f.fields()) {
            assert!(!f.used_by.is_empty(), "{}: who uses it?", f.id);
        }
    }

    /// ADR-033 §3: of the board only the roles reach the AI (counted); the names, the CLABE and the rest never.
    #[test]
    fn of_the_board_and_the_accounts_only_the_roles_reach_the_ai() {
        for f in [&BOARD_MEMBER, &BANK_ACCOUNT].iter().flat_map(|f| f.fields()) {
            if f.id == "institution.board.role" {
                assert_eq!((f.sensitivity, f.ai), (Sensitivity::Internal, AiUse::AsIs));
            } else {
                assert_eq!((f.sensitivity, f.ai), (Sensitivity::InstitutionalPrivate, AiUse::Never), "{}", f.id);
            }
        }
    }

    /// The contact, the address, the RFC, the folios and the keys of the institution never reach the AI (ADR-033 §3),
    /// nor the name of its legal representative, a person (ERP-1).
    #[test]
    fn what_is_the_institutions_own_never_reaches_the_ai() {
        for id in [
            "institution.contact_phone", "institution.contact_email", "institution.street", "institution.postal_code", "institution.legal_rfc",
            "institution.junta_folio", "institution.donee_letter_number", "institution.cluni_key",
        ] {
            let f = field(id).unwrap();
            assert_eq!((f.sensitivity, f.ai), (Sensitivity::InstitutionalPrivate, AiUse::Never), "{id}");
        }
        let rep = field("institution.legal_rep_name").unwrap();
        assert_eq!((rep.sensitivity, rep.ai), (Sensitivity::Personal, AiUse::Never));
    }
}
