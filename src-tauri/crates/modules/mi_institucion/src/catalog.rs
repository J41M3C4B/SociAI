//! The part of the data dictionary (K3) Mi institución declares: the profile of the institution, one entity, each field
//! with its class, once. The kernel validates the profile with it, lists what is missing and knows what is protected.
//! The windows of the screen are views of this one entity (`windows`), so a field may apply by what another window
//! says.
//!
//! What the institution says of itself in public is `public`; its figures and its regime, `internal`; its contact,
//! address, folios and keys, `confidential`; the name of its legal representative, a person, `personal`.

use kernel::{Condition, Entity, Field, Kind, Rule};

use crate::contract::{
    AGE_GROUPS, BY_PROGRAM, CARE_AREAS, DONEE_CATEGORIES, LATEST_YEAR, LEGAL_FORMS, LODGING, MAX_AGE, MAX_ESTIMATE, MODALITIES, OLDEST_YEAR,
    POPULATIONS, REGISTRY, SEXES_SERVED, STATE_CODES, TAX_REGIMES, UNDER_A_JUNTA,
};

/// What applies once it serves a stage of life: the public at large or other organizations have no sex nor ages.
const BY_AGE: Condition = Condition::AnyOf { field: "populations", values: AGE_GROUPS };
/// What applies where people live or spend the day: the places, and how many people are there today.
const LODGES: Condition = Condition::AnyOf { field: "modalities", values: LODGING };
/// What applies where people come and go: how many it served in the last year. A residence with community programs
/// is asked both.
const COMES_AND_GOES: Condition = Condition::AnyOf { field: "modalities", values: BY_PROGRAM };
/// What applies to an authorized donee.
const DONEE: Condition = Condition::AnyOf { field: "authorized_donee", values: &["yes"] };

pub const PROFILE: Entity = Entity {
    id: "profile",
    fields: &[
        // who it is, and whom and how it serves
        Field::new("name", Kind::Text, "public").mandatory(),
        Field::new("mission", Kind::LongText, "public").expected(),
        Field::new("populations", Kind::MultiSelect, "public").options(POPULATIONS).expected(),
        Field::new("sex_served", Kind::Select, "public").options(SEXES_SERVED).when(BY_AGE),
        Field::new("modalities", Kind::MultiSelect, "public").options(MODALITIES).expected(),
        Field::new("care_areas", Kind::MultiSelect, "public").options(CARE_AREAS),
        Field::new("services", Kind::LongText, "public"),
        Field::new("age_min", Kind::Number, "public").range(0, MAX_AGE).when(BY_AGE),
        Field::new("age_max", Kind::Number, "public").range(0, MAX_AGE).when(BY_AGE),
        Field::new("admission_criteria", Kind::LongText, "public"),
        // how to reach it and where it is: only the municipality and the state are public
        Field::new("contact_phone", Kind::Phone, "confidential"),
        Field::new("contact_email", Kind::Email, "confidential"),
        Field::new("street", Kind::Text, "confidential").expected(),
        Field::new("ext_number", Kind::Text, "confidential"),
        Field::new("int_number", Kind::Text, "confidential"),
        Field::new("neighborhood", Kind::Text, "confidential"),
        Field::new("postal_code", Kind::Text, "confidential").rule(Rule::PostalCode).expected(),
        Field::new("municipality", Kind::Text, "public").rule(Rule::PlaceName).expected(),
        Field::new("state", Kind::Select, "public").options(STATE_CODES).expected(),
        // how it stands before the law, the SAT and its Junta
        Field::new("legal_name", Kind::Text, "public").expected(),
        Field::new("purpose", Kind::LongText, "public").expected(),
        Field::new("legal_form", Kind::Select, "public").options(LEGAL_FORMS).expected(),
        Field::new("founded_year", Kind::Year, "public").range(OLDEST_YEAR, LATEST_YEAR).expected(),
        Field::new("junta_folio", Kind::Text, "confidential").expected().when(Condition::AnyOf { field: "legal_form", values: UNDER_A_JUNTA }),
        Field::new("legal_rfc", Kind::Text, "confidential").rule(Rule::RfcMoral).expected(),
        Field::new("tax_regime", Kind::Select, "internal").options(TAX_REGIMES).expected(),
        Field::new("fiscal_postal_code", Kind::Text, "confidential").rule(Rule::PostalCode),
        Field::new("authorized_donee", Kind::Select, "public").options(REGISTRY).expected(),
        Field::new("cluni", Kind::Select, "public").options(REGISTRY).expected(),
        Field::new("donee_category", Kind::Select, "public").options(DONEE_CATEGORIES).when(DONEE),
        Field::new("donee_letter_number", Kind::Text, "confidential").when(DONEE),
        Field::new("donee_letter_date", Kind::Date, "confidential").when(DONEE),
        Field::new("cluni_key", Kind::Text, "confidential").when(Condition::AnyOf { field: "cluni", values: &["yes"] }),
        Field::new("legal_rep_name", Kind::Text, "personal"),
        Field::new("legal_rep_valid_until", Kind::Date, "confidential"),
        // how many fit, and how many are served and work there while their records are not in their modules
        Field::new("capacity_total", Kind::Number, "internal").range(0, MAX_ESTIMATE).expected().when(LODGES),
        // two counts, so a number never changes its meaning when the ways it serves change
        Field::new("served_estimate", Kind::Number, "internal").range(0, MAX_ESTIMATE).expected().when(LODGES),
        Field::new("served_last_year", Kind::Number, "internal").range(0, MAX_ESTIMATE).expected().when(COMES_AND_GOES),
        Field::new("staff_paid_estimate", Kind::Number, "internal").range(0, MAX_ESTIMATE).expected(),
        Field::new("staff_volunteer_estimate", Kind::Number, "internal").range(0, MAX_ESTIMATE).expected(),
        Field::new("notes", Kind::LongText, "internal"),
    ],
};
