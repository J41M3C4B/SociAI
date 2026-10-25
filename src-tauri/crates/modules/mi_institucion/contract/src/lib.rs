//! The published types of Mi institución (its `contract`, the ERP blueprint §4): the only part of it another module
//! may use. Changing them is changing the module's version.
//!
//! The codes of its lists travel as short codes; the screen turns them into words (`src/i18n/es-MX.ts`).
//!
//! The contract does not reach into its module: it does not depend on it, so this does not compile.
//!
//! ```compile_fail
//! use mi_institucion as _;
//! ```

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// What saving a window of the profile asks: the window and its values, by the id of each field of the dictionary
/// (`legal_name`). The same input prepares the change and applies it. A field of the window left out is emptied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WindowInput {
    /// `identity`, `contact`, `legal` or `capacity`.
    pub window: String,
    pub values: Map<String, Value>,
}

/// The codes of the 32 states, in alphabetical order of their names.
pub const STATE_CODES: &[&str] = &[
    "ags", "bc", "bcs", "camp", "coah", "col", "chis", "chih", "cdmx", "dgo", "gto", "gro", "hgo", "jal", "mex", "mich", "mor",
    "nay", "nl", "oax", "pue", "qro", "qroo", "slp", "sin", "son", "tab", "tamps", "tlax", "ver", "yuc", "zac",
];
/// Asociación civil, institución de asistencia privada, institución de beneficencia privada, sociedad civil,
/// asociación de beneficencia privada, asociación religiosa, other.
pub const LEGAL_FORMS: &[&str] = &["ac", "iap", "ibp", "sc", "abp", "religious", "other"];
/// The legal forms watched over by the Junta de Asistencia (o Beneficencia) Privada of their state.
pub const UNDER_A_JUNTA: &[&str] = &["iap", "ibp", "abp"];
/// Answers for «donataria autorizada» and «CLUNI».
pub const REGISTRY: &[&str] = &["yes", "in_progress", "no"];
/// The tax regime of the institution (its constancia de situación fiscal): personas morales con fines no
/// lucrativos (603), régimen general de ley (601), or another.
pub const TAX_REGIMES: &[&str] = &["non_profit", "general", "other"];
/// The activity an authorized donee is authorized for (rubro autorizado, Ley del ISR art. 79 and 82).
pub const DONEE_CATEGORIES: &[&str] = &[
    "assistance", "education", "research", "culture", "scholarships", "ecology", "species", "support_donees", "public_works",
    "libraries", "museums", "social_development",
];
/// The oldest year an institution can have been founded, and the latest a year may say.
pub const OLDEST_YEAR: i64 = 1800;
pub const LATEST_YEAR: i64 = 2100;
/// The oldest a person served can be said to be.
pub const MAX_AGE: i64 = 120;
/// The most people a quick figure may say.
pub const MAX_ESTIMATE: i64 = 100_000;

/// Whom the institution serves: by stage of life, the public at large (no group of age), or other organizations.
/// Several may be marked.
pub const POPULATIONS: &[&str] = &["early_childhood", "childhood", "adolescence", "youth", "adults", "older_adults", "general_public", "organizations"];
/// The stages of life: with one of them, the sex and the ages served are asked.
pub const AGE_GROUPS: &[&str] = &["early_childhood", "childhood", "adolescence", "youth", "adults", "older_adults"];
/// The stages of life under 18.
pub const MINORS: &[&str] = &["early_childhood", "childhood", "adolescence"];
/// Whether it serves women, men or both.
pub const SEXES_SERVED: &[&str] = &["women", "men", "all"];
/// How it serves them. Several may be marked.
pub const MODALITIES: &[&str] = &["residential", "day_care", "outpatient", "community", "home_care", "remote", "support"];
/// The ways where people stay in the institution, so it has places: its capacity and occupancy, and a count of
/// people served today.
pub const LODGING: &[&str] = &["residential", "day_care"];
/// The ways where people come and go: how many it served in the last year is asked (`served_last_year`). With
/// `LODGING` they are every way, each in one of the two.
pub const BY_PROGRAM: &[&str] = &["outpatient", "community", "home_care", "remote", "support"];
/// What it attends to. Several may be marked.
pub const CARE_AREAS: &[&str] = &[
    "care", "health", "disability", "education", "food", "violence", "addictions", "street", "migration", "mental_health", "environment", "culture",
    "human_rights", "community_development", "employment", "other",
];

/// The kind of institution the modules still go by (`elderly_home`, `children_home`, `other`), from whom it serves:
/// only older adults (with or without other adults) is an elderly home; only minors (with or without youth) is a
/// children's home; anything mixed, or nothing marked, is `other`.
pub fn kind_of(populations: &[String]) -> &'static str {
    let has = |list: &[&str]| populations.iter().any(|p| list.contains(&p.as_str()));
    match (has(MINORS), has(&["older_adults"])) {
        (true, false) => "children_home",
        (false, true) => "elderly_home",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kind_comes_from_whom_it_serves() {
        let k = |p: &[&str]| kind_of(&p.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(k(&["older_adults"]), "elderly_home");
        assert_eq!(k(&["older_adults", "adults"]), "elderly_home");
        assert_eq!(k(&["childhood", "youth"]), "children_home");
        assert_eq!(k(&["childhood", "older_adults"]), "other");
        assert_eq!(k(&[]), "other");
    }

    #[test]
    fn every_way_of_serving_is_lodging_or_by_program_once() {
        for m in MODALITIES {
            assert!(LODGING.contains(m) ^ BY_PROGRAM.contains(m), "{m}");
        }
    }
}
