//! The onboarding of the institution (ADR-031, ADR-042): the first start asks only what is indispensable, the fields
//! the catalog of forms marks `essential`, in short steps. Every one of them lives in a window of «Mi institución»,
//! so finishing the first start and filling those windows are the same thing. The data belong to the institution, not
//! to a person: while one indispensable datum is missing, whoever enters with access sees the steps instead of the app
//! (the administrator may leave them to the direction). The rules live here; the screen only shows what is missing.

use crate::common::forms::{is_filled, FieldSpec, Values};
use serde::Serialize;

/// A step of the first start: a few indispensable fields of one window, asked together.
pub struct Step {
    pub key: &'static str,
    /// The window of «Mi institución» its fields belong to.
    pub form: &'static str,
    /// How many columns the screen uses for them.
    pub columns: u8,
    pub fields: &'static [&'static str],
}

/// The steps, in order (ADR-042 §3). The last screen (review and confirm) is not a step: it needs all of them complete.
pub const STEPS: &[Step] = &[
    Step { key: "who", form: "institution.identity", columns: 1, fields: &["institution.name", "institution.mission"] },
    Step { key: "attention", form: "institution.identity", columns: 1, fields: &["institution.populations", "institution.modalities"] },
    Step { key: "place", form: "institution.contact", columns: 2, fields: &["institution.state", "institution.municipality"] },
    Step {
        key: "constitution",
        form: "institution.legal",
        columns: 2,
        fields: &["institution.legal_form", "institution.founded_year", "institution.authorized_donee", "institution.cluni"],
    },
    Step {
        key: "figures",
        form: "institution.capacity",
        columns: 2,
        fields: &[
            "institution.capacity_total",
            "institution.served_estimate",
            "institution.served_last_year",
            "institution.staff_paid_estimate",
            "institution.staff_volunteer_estimate",
        ],
    },
];

/// The quick figures the records of a module answer: with people registered in «Beneficiarios» the people served are
/// known, and with people in «Personal», the team (ADR-031 §3: the records win).
pub fn answered_by_records(served_in_module: i64, staff_in_module: i64) -> Vec<&'static str> {
    let mut out = Vec::new();
    if served_in_module > 0 {
        out.push("institution.served_estimate");
    }
    if staff_in_module > 0 {
        out.extend(["institution.staff_paid_estimate", "institution.staff_volunteer_estimate"]);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StepStatus {
    pub key: &'static str,
    /// The indispensable data still missing, by field id.
    pub missing: Vec<&'static str>,
    pub complete: bool,
}

/// What each step still needs, from the values of the windows of «Mi institución» (by field id) and what the records
/// already answer. A field that does not apply with what was said before is not needed (the capacity, where nobody
/// stays).
pub fn steps(values: &Values, answered: &[&str]) -> Vec<StepStatus> {
    STEPS
        .iter()
        .map(|s| {
            let missing: Vec<&'static str> =
                s.fields.iter().copied().filter(|f| !answered.contains(f) && field(f).applies_when.holds(values) && !is_filled(values.get(*f))).collect();
            StepStatus { key: s.key, complete: missing.is_empty(), missing }
        })
        .collect()
}

/// Whether every step is complete.
pub fn complete(steps: &[StepStatus]) -> bool {
    steps.iter().all(|s| s.complete)
}

/// The field of the catalog behind an id of a step.
pub fn field(id: &str) -> &'static FieldSpec {
    crate::core::institution::forms::field(id).unwrap_or_else(|| panic!("{id} is not in the catalog"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::institution::catalog::{state_name, RECORD_TYPES, STATES};
    use crate::core::institution::forms::{form, forms, LIST_FORMS};
    use serde_json::json;

    fn values(v: serde_json::Value) -> Values {
        serde_json::from_value(v).unwrap()
    }

    fn missing(values: &Values, answered: &[&str]) -> Vec<(&'static str, Vec<&'static str>)> {
        steps(values, answered).into_iter().filter(|s| !s.complete).map(|s| (s.key, s.missing)).collect()
    }

    /// ADR-042 §1: the steps ask exactly the fields the catalog marks indispensable, each one once, each step inside
    /// one window. ADR-047: each one is always asked, or hides only behind another indispensable datum asked in an
    /// earlier step (the capacity, behind how it serves), so nothing hides behind an answer still to come.
    #[test]
    fn the_steps_are_the_indispensable_fields_of_the_catalog() {
        let essential: Vec<&str> = forms().iter().flat_map(|f| f.fields()).filter(|f| f.essential).map(|f| f.id).collect();
        let asked: Vec<&str> = STEPS.iter().flat_map(|s| s.fields.iter().copied()).collect();
        let mut a = essential.clone();
        let mut b = asked.clone();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b, "the first start and the catalog disagree");
        assert!((1..asked.len()).all(|i| !asked[..i].contains(&asked[i])), "a field is asked twice: {asked:?}");
        for (n, s) in STEPS.iter().enumerate() {
            let window = form(s.form).unwrap_or_else(|| panic!("{} is not a window", s.form));
            let before: Vec<&str> = STEPS[..n].iter().flat_map(|s| s.fields.iter().copied()).collect();
            for f in s.fields {
                let spec = window.field(f).unwrap_or_else(|| panic!("{f} is not in {}", s.form));
                assert!(spec.required, "{f}: an indispensable field is required");
                if let Some(decider) = spec.applies_when.field() {
                    assert!(before.contains(&decider), "{f} hides behind {decider}, which is not asked in an earlier step");
                }
            }
        }
        assert!(LIST_FORMS.iter().flat_map(|f| f.fields()).all(|f| !f.essential), "the lists are filled after the first start");
        assert_eq!(asked.len(), 15);
    }

    /// ADR-042 §1, criterion A: a datum that changes what the program asks or shows is indispensable. Without it the
    /// windows would hide or show the wrong fields and the Expediente would ask for the wrong documents.
    #[test]
    fn what_changes_what_the_program_asks_is_indispensable() {
        let profile = |id: &str| forms().iter().any(|f| f.field(id).is_some());
        let mut deciders: Vec<&str> = forms()
            .iter()
            .chain(LIST_FORMS)
            .flat_map(|f| f.fields())
            .flat_map(|f| f.looks_at())
            .filter(|id| profile(id))
            .collect();
        deciders.extend(RECORD_TYPES.iter().flat_map(|(_, a)| a.fields().iter().copied()));
        // the kind the modules go by comes out of whom it serves
        deciders.push("institution.populations");
        for id in deciders {
            assert!(field(id).essential, "{id} decides what the program asks, so it must be indispensable");
        }
    }

    #[test]
    fn an_empty_start_names_everything_it_needs() {
        assert_eq!(
            missing(&Values::new(), &[]),
            vec![
                ("who", vec!["institution.name", "institution.mission"]),
                ("attention", vec!["institution.populations", "institution.modalities"]),
                ("place", vec!["institution.state", "institution.municipality"]),
                ("constitution", vec!["institution.legal_form", "institution.founded_year", "institution.authorized_donee", "institution.cluni"]),
                // the capacity and the counts of people wait for how it serves
                ("figures", vec!["institution.staff_paid_estimate", "institution.staff_volunteer_estimate"]),
            ]
        );
    }

    #[test]
    fn a_full_institution_is_complete_and_zero_volunteers_is_an_answer() {
        let v = values(json!({
            "institution.name": "Asilo Ficticio", "institution.mission": "Un hogar digno.",
            "institution.populations": ["older_adults"], "institution.modalities": ["residential"],
            "institution.state": "jal", "institution.municipality": "Zapopan",
            "institution.legal_form": "ac", "institution.founded_year": 1987, "institution.authorized_donee": "yes", "institution.cluni": "in_progress",
            "institution.capacity_total": 25, "institution.served_estimate": 22, "institution.staff_paid_estimate": 6, "institution.staff_volunteer_estimate": 0,
        }));
        assert!(complete(&steps(&v, &[])), "{:?}", missing(&v, &[]));
        let mut blank = v.clone();
        blank.insert("institution.municipality".into(), json!("   "));
        blank.insert("institution.populations".into(), json!([]));
        assert_eq!(missing(&blank, &[]), vec![("attention", vec!["institution.populations"]), ("place", vec!["institution.municipality"])]);
        assert_eq!(state_name("qroo"), Some("Quintana Roo"));
        assert_eq!(STATES.len(), 32);
    }

    /// ERP-2: an organization where nobody stays is not asked its places; a residence still is.
    #[test]
    fn the_capacity_is_asked_only_where_people_stay() {
        let community = values(json!({
            "institution.name": "Red Comunitaria Ficticia", "institution.mission": "Talleres y despensas en la colonia.",
            "institution.populations": ["general_public"], "institution.modalities": ["community", "support"],
            "institution.state": "pue", "institution.municipality": "Puebla",
            "institution.legal_form": "ac", "institution.founded_year": 2012, "institution.authorized_donee": "no", "institution.cluni": "yes",
            "institution.served_last_year": 340, "institution.staff_paid_estimate": 2, "institution.staff_volunteer_estimate": 15,
        }));
        assert!(complete(&steps(&community, &[])), "{:?}", missing(&community, &[]));
        // a day care with community programs: its places and both counts
        let mut day_care = community.clone();
        day_care.insert("institution.modalities".into(), json!(["community", "day_care"]));
        assert_eq!(missing(&day_care, &[]), vec![("figures", vec!["institution.capacity_total", "institution.served_estimate"])]);
        // only a day care: no count of the last year
        day_care.insert("institution.modalities".into(), json!(["day_care"]));
        day_care.remove("institution.served_last_year");
        assert_eq!(missing(&day_care, &[]), vec![("figures", vec!["institution.capacity_total", "institution.served_estimate"])]);
    }

    #[test]
    fn the_records_of_the_modules_count_instead_of_the_quick_figures() {
        let v = values(json!({ "institution.modalities": ["residential"], "institution.capacity_total": 25 }));
        let figures = |answered: &[&str]| steps(&v, answered).into_iter().find(|s| s.key == "figures").unwrap().missing;
        assert_eq!(figures(&answered_by_records(0, 0)), vec!["institution.served_estimate", "institution.staff_paid_estimate", "institution.staff_volunteer_estimate"]);
        assert_eq!(figures(&answered_by_records(18, 0)), vec!["institution.staff_paid_estimate", "institution.staff_volunteer_estimate"]);
        assert!(figures(&answered_by_records(18, 7)).is_empty());
    }
}
