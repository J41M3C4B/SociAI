//! The fixed catalogs of the institution (ADR-031): its state, legal form and federal registries travel as short
//! codes; the screen and the sheet of the AI turn them into words. The profile and the first start both use them.

/// The 32 states: (code, name).
pub const STATES: &[(&str, &str)] = &[
    ("ags", "Aguascalientes"),
    ("bc", "Baja California"),
    ("bcs", "Baja California Sur"),
    ("camp", "Campeche"),
    ("coah", "Coahuila"),
    ("col", "Colima"),
    ("chis", "Chiapas"),
    ("chih", "Chihuahua"),
    ("cdmx", "Ciudad de México"),
    ("dgo", "Durango"),
    ("gto", "Guanajuato"),
    ("gro", "Guerrero"),
    ("hgo", "Hidalgo"),
    ("jal", "Jalisco"),
    ("mex", "Estado de México"),
    ("mich", "Michoacán"),
    ("mor", "Morelos"),
    ("nay", "Nayarit"),
    ("nl", "Nuevo León"),
    ("oax", "Oaxaca"),
    ("pue", "Puebla"),
    ("qro", "Querétaro"),
    ("qroo", "Quintana Roo"),
    ("slp", "San Luis Potosí"),
    ("sin", "Sinaloa"),
    ("son", "Sonora"),
    ("tab", "Tabasco"),
    ("tamps", "Tamaulipas"),
    ("tlax", "Tlaxcala"),
    ("ver", "Veracruz"),
    ("yuc", "Yucatán"),
    ("zac", "Zacatecas"),
];

// the codes of the profile are the module's (ERP-1): its contract publishes them
pub use mi_institucion::contract::{
    kind_of, CARE_AREAS, LEGAL_FORMS, LODGING, MAX_ESTIMATE, MINORS, MODALITIES, OLDEST_YEAR, POPULATIONS, REGISTRY, SEXES_SERVED, UNDER_A_JUNTA,
};

/// The roles of the board of trustees or governing body (ADR-033 §3): presidencia, vicepresidencia, secretaría,
/// tesorería, vocal, or another one the institution names.
pub const BOARD_ROLES: &[&str] = &["president", "vice_president", "secretary", "treasurer", "member", "other"];
/// What a bank account of the institution is used for: receiving donations, paying for its operation.
pub const BANK_PURPOSES: &[&str] = &["donations", "operations"];

// ------------------------------------------------------------------ the record of the institution (ADR-033 §5)

/// The state of a document of the record: the institution has it, it is in process, it is missing, or it does not
/// apply. The file is optional: having it without uploading it counts.
pub const RECORD_STATUSES: &[&str] = &["have", "in_progress", "missing", "not_applicable"];

/// When a type of document applies to an institution, from what it says of itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applies {
    Always,
    /// An authorized donee, or one in process (`authorized_donee`).
    Donee,
    /// With the CLUNI, or in process (`cluni`).
    Cluni,
    /// Watched over by a Junta de Asistencia (o Beneficencia) Privada (`UNDER_A_JUNTA`).
    Junta,
    /// Girls, boys or teenagers live there (the Registro Nacional de Centros de Asistencia Social is of the centers
    /// where they live; an afternoon tutoring does not need it).
    MinorsLiving,
    /// People live or spend the day there (the sanitary notice).
    Housing,
    /// People are served in a building of its own: they live, spend the day or come for a consultation there (the land
    /// use license and the civil protection program). Another institution may still keep them as an «other».
    Premises,
}

impl Applies {
    /// The data of the institution it depends on: they are indispensable (ADR-042), or the Expediente would ask for
    /// the wrong documents.
    pub const fn fields(self) -> &'static [&'static str] {
        match self {
            Applies::Always => &[],
            Applies::Donee => &["institution.authorized_donee"],
            Applies::Cluni => &["institution.cluni"],
            Applies::Junta => &["institution.legal_form"],
            Applies::MinorsLiving => &["institution.populations", "institution.modalities"],
            Applies::Housing | Applies::Premises => &["institution.modalities"],
        }
    }
}

/// The documents of the record, in the order the screen lists them, and when each one applies (audit §4.1). A
/// document the institution names itself is an `other`, which is not listed here.
pub const RECORD_TYPES: &[(&str, Applies)] = &[
    ("articles_of_incorporation", Applies::Always),
    ("bylaws_amendments", Applies::Always),
    ("power_of_attorney", Applies::Always),
    ("tax_status_certificate", Applies::Always),
    ("tax_compliance_opinion", Applies::Always),
    ("donee_authorization", Applies::Donee),
    ("cluni_certificate", Applies::Cluni),
    ("junta_registration", Applies::Junta),
    ("social_care_registry", Applies::MinorsLiving),
    ("sanitary_notice", Applies::Housing),
    ("land_use_license", Applies::Premises),
    ("civil_protection_program", Applies::Premises),
    ("privacy_notice", Applies::Always),
    ("internal_rules", Applies::Always),
    ("org_chart", Applies::Always),
    ("financial_statements", Applies::Always),
    ("annual_report", Applies::Always),
    ("junta_budget", Applies::Junta),
];

/// A document the institution names itself.
pub const RECORD_OTHER: &str = "other";

/// The municipalities and alcaldías of Mexico by state, with their official names: `code\tname`, one per line, from
/// the Catálogo Único de Claves de Áreas Geoestadísticas of INEGI (public, 2,478 names, `municipios.tsv`).
const MUNICIPALITIES: &str = include_str!("municipios.tsv");

/// A name read as a person reads it: without capitals, accents nor punctuation, and without the word that says what
/// it is («municipio de», «alcaldía», «delegación»).
fn place_key(s: &str) -> String {
    let folded = crate::scanner::normalize::fold_str(s);
    let words: Vec<&str> = folded.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let skip = match words.as_slice() {
        ["MUNICIPIO", "DE", ..] => 2,
        ["MUNICIPIO" | "ALCALDIA" | "DELEGACION", ..] => 1,
        _ => 0,
    };
    words[skip..].join(" ")
}

/// The official name of a municipality written another way («BENITO JUAREZ», «Alcaldía Benito Juárez»), from the
/// catalog of INEGI: the one name that reads the same, in its state when it is known. `None` when none does (a name
/// the catalog does not have stays as it was written).
pub fn municipality_name(state: Option<&str>, said: &str) -> Option<&'static str> {
    let key = place_key(said);
    if key.is_empty() {
        return None;
    }
    let mut names: Vec<&'static str> = MUNICIPALITIES
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .filter(|(code, name)| state.is_none_or(|s| s == *code) && place_key(name) == key)
        .map(|(_, name)| name)
        .collect();
    names.dedup();
    match names[..] {
        [one] => Some(one),
        _ => None,
    }
}

pub fn state_name(code: &str) -> Option<&'static str> {
    STATES.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

// ------------------------------------------------------------------ the attention profile (ADR-033 §2)

/// The ways that need a building where people are served: its land use license and civil protection program.
pub const PREMISES: &[&str] = &["residential", "day_care", "outpatient"];

/// What a kind of institution means as an attention profile, for the data written before the profile existed and
/// for the first start, which still asks for the kind: (populations, modalities, areas).
pub fn attention_of_kind(kind: &str) -> (&'static [&'static str], &'static [&'static str], &'static [&'static str]) {
    match kind {
        "elderly_home" => (&["older_adults"], &["residential"], &["care"]),
        "children_home" => (&["early_childhood", "childhood", "adolescence"], &["residential"], &["care"]),
        _ => (&[], &[], &[]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_codes_are_the_states() {
        use mi_institucion::contract::STATE_CODES;
        assert_eq!(STATE_CODES.len(), STATES.len());
        assert!(STATES.iter().zip(STATE_CODES).all(|((c, _), code)| c == code));
    }

    fn v(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn every_kind_turns_into_a_profile_that_gives_the_kind_back() {
        for kind in ["elderly_home", "children_home"] {
            let (populations, modalities, areas) = attention_of_kind(kind);
            assert_eq!(kind_of(&v(populations)), kind);
            assert!(populations.iter().all(|p| POPULATIONS.contains(p)) && modalities.iter().all(|m| MODALITIES.contains(m)) && areas.iter().all(|a| CARE_AREAS.contains(a)));
        }
        assert_eq!(attention_of_kind("other"), (&[][..], &[][..], &[][..]));
    }
}
