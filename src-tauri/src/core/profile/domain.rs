//! Institution profile: only aggregated data (how many, never who). Staff are one anonymous line per position
//! (role, pay, contract; never a name), and people served are grouped, never one by one.

use crate::modules::finance::domain::money::MAX_MXN;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InstitutionKind {
    ElderlyHome,
    ChildrenHome,
    #[default]
    Other,
}

impl InstitutionKind {
    pub fn as_db(self) -> &'static str {
        match self {
            InstitutionKind::ElderlyHome => "elderly_home",
            InstitutionKind::ChildrenHome => "children_home",
            InstitutionKind::Other => "other",
        }
    }
    pub fn from_db(s: &str) -> Self {
        match s {
            "elderly_home" => InstitutionKind::ElderlyHome,
            "children_home" => InstitutionKind::ChildrenHome,
            _ => InstitutionKind::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyLevel {
    Low,
    Medium,
    High,
    Total,
}

impl DependencyLevel {
    pub fn as_db(self) -> &'static str {
        match self {
            DependencyLevel::Low => "low",
            DependencyLevel::Medium => "medium",
            DependencyLevel::High => "high",
            DependencyLevel::Total => "total",
        }
    }
    pub fn from_db(s: &str) -> Option<Self> {
        Some(match s {
            "low" => DependencyLevel::Low,
            "medium" => DependencyLevel::Medium,
            "high" => DependencyLevel::High,
            "total" => DependencyLevel::Total,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractKind {
    /// Fixed position with benefits.
    Permanent,
    /// For a set time or project.
    Temporary,
    /// Paid against an invoice (honorarios).
    Fees,
}

impl ContractKind {
    pub fn as_db(self) -> &'static str {
        match self {
            ContractKind::Permanent => "permanent",
            ContractKind::Temporary => "temporary",
            ContractKind::Fees => "fees",
        }
    }
    pub fn from_db(s: &str) -> Option<Self> {
        Some(match s {
            "permanent" => ContractKind::Permanent,
            "temporary" => ContractKind::Temporary,
            "fees" => ContractKind::Fees,
            _ => return None,
        })
    }
}

/// Institutional data. Contact fields are institutional, never sent to the AI,
/// and the scanner does not look at them (see `03-gobernanza-datos.md`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstitutionInput {
    pub name: String,
    pub kind: InstitutionKind,
    pub mission: Option<String>,
    pub legal_rfc: Option<String>,
    pub contact_phone: Option<String>,
    pub contact_email: Option<String>,
    pub legal_rep_name: Option<String>,
    /// Where it is (ADR-031): a code of `institution::catalog::STATES` and the municipality (a place name, `Rule::PlaceName`). Not personal: it reaches the AI.
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub municipality: Option<String>,
    #[serde(default)]
    pub founded_year: Option<i64>,
    /// A code of `institution::catalog::LEGAL_FORMS`.
    #[serde(default)]
    pub legal_form: Option<String>,
    /// Donataria autorizada (SAT) and CLUNI: `yes`, `in_progress` or `no`.
    #[serde(default)]
    pub authorized_donee: Option<String>,
    #[serde(default)]
    pub cluni: Option<String>,
    /// Whom and how it serves (ADR-033 §2). `None` when the screen did not send it: what is saved stays.
    #[serde(default)]
    pub attention: Option<Attention>,
    /// The data kept as fields of the catalog (ADR-033 §1), by field id (`institution.legal_name`); the columns are
    /// in the module Mi institución (ERP-1). `None` when the screen did not send them: what is saved stays.
    #[serde(default)]
    pub details: Option<crate::common::forms::Values>,
}

/// The attention profile: codes of `institution::catalog`. It reaches the AI (nothing in it is personal).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attention {
    #[serde(default)]
    pub populations: Vec<String>,
    #[serde(default)]
    pub sex_served: Option<String>,
    #[serde(default)]
    pub modalities: Vec<String>,
    #[serde(default)]
    pub care_areas: Vec<String>,
}

impl Attention {
    /// What a kind of institution means as a profile (for data older than the profile, and the first start).
    pub fn of_kind(kind: InstitutionKind) -> Self {
        let (p, m, a) = crate::core::institution::catalog::attention_of_kind(kind.as_db());
        let owned = |l: &[&str]| l.iter().map(|s| s.to_string()).collect();
        Attention { populations: owned(p), sex_served: None, modalities: owned(m), care_areas: owned(a) }
    }

    /// The kind the modules go by, from whom it serves (`None` with nobody marked).
    pub fn kind(&self) -> Option<InstitutionKind> {
        (!self.populations.is_empty()).then(|| InstitutionKind::from_db(crate::core::institution::catalog::kind_of(&self.populations)))
    }
}

/// The kind and the attention profile to keep when saving, from what was saved before and what came in. The profile
/// decides the kind when the person wrote it; while the first start still asks for the kind, a kind that changes (or
/// a profile still empty) fills the populations from the kind. A mixed profile is an institution of kind `other`.
pub fn reconcile_attention(previous: Option<(InstitutionKind, Attention)>, kind: InstitutionKind, sent: Option<&Attention>) -> (InstitutionKind, Attention) {
    if let Some(a) = sent {
        return (a.kind().unwrap_or(kind), a.clone());
    }
    let (kind_before, mut a) = previous.unwrap_or((kind, Attention::default()));
    if kind != kind_before || a.populations.is_empty() {
        let from_kind = Attention::of_kind(kind);
        a.populations = from_kind.populations;
        if a.modalities.is_empty() {
            a.modalities = from_kind.modalities;
        }
        if a.care_areas.is_empty() {
            a.care_areas = from_kind.care_areas;
        }
    }
    (kind, a)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PopulationGroupInput {
    pub label: String,
    pub age_min: Option<i64>,
    pub age_max: Option<i64>,
    pub count: i64,
    pub dependency_level: Option<DependencyLevel>,
    pub notes: Option<String>,
    /// How many of the group pay a stay fee (the rest do not).
    #[serde(default)]
    pub paying_count: Option<i64>,
    /// Monthly stay fee per paying person, in pesos.
    #[serde(default)]
    pub monthly_fee_mxn: Option<i64>,
}

/// A position: one person when `count` is 1 (what the app writes); older data may group several.
/// The pay is per person. There is never a name here.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StaffGroupInput {
    pub role: String,
    pub count: i64,
    pub shift: Option<String>,
    pub paid: bool,
    #[serde(default)]
    pub monthly_salary_mxn: Option<i64>,
    #[serde(default)]
    pub contract: Option<ContractKind>,
    #[serde(default)]
    pub start_year: Option<i64>,
    #[serde(default)]
    pub notes: Option<String>,
    /// The kind of relation of the staff module (`employee`, `fees`, `religious`, `volunteer`, `trainee`,
    /// `external`; ADR-027): it says where the money of the line counts. `None` in data older than the module.
    #[serde(default)]
    pub relation: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileInput {
    pub institution: InstitutionInput,
    pub capacity_total: Option<i64>,
    pub notes: Option<String>,
    /// Quick figures said by the person (ADR-031): how many people are served and work there while their records
    /// are not in the modules yet. The records win when they exist.
    #[serde(default)]
    pub served_estimate: Option<i64>,
    #[serde(default)]
    pub staff_paid_estimate: Option<i64>,
    #[serde(default)]
    pub staff_volunteer_estimate: Option<i64>,
    #[serde(default)]
    pub population: Vec<PopulationGroupInput>,
    #[serde(default)]
    pub staff: Vec<StaffGroupInput>,
}

/// Something the person should look at. The UI turns `code` into friendly text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileIssue {
    pub code: &'static str,
    pub field: String,
    /// `true` blocks saving; `false` is only a heads-up ("algo no cuadra").
    pub blocking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ProfileTotals {
    pub population: i64,
    pub staff_paid: i64,
    pub staff_volunteer: i64,
    /// Sum of the monthly pay of the people who receive a salary.
    pub payroll_monthly_mxn: i64,
    /// The pay of twelve months, without benefits.
    pub payroll_annual_mxn: i64,
    /// Aguinaldo and vacation premium of the jobs that carry them (not fees).
    pub payroll_benefits_annual_mxn: i64,
    /// What the payroll costs in a year: pay plus benefits.
    pub payroll_cost_annual_mxn: i64,
    /// Paid people whose benefits rest on an assumption (no contract or no start year in the roster).
    pub benefits_assumed: i64,
    /// Contributions to the congregation and grants of social service, in a year (not payroll).
    pub staff_support_annual_mxn: i64,
    /// What the outside companies bill for their staff, in a year (not payroll).
    pub external_staff_annual_mxn: i64,
    /// People who pay a stay fee, and what they bring in.
    pub fee_payers: i64,
    pub fees_monthly_mxn: i64,
    pub fees_annual_mxn: i64,
}

/// The pay rules of the law live in the staff module (ADR-027).
pub use crate::modules::hr::api::annual_benefits;

/// Whether a contract carries the benefits of the law. Fees (honorarios) do not; an unknown contract is taken as
/// a job with benefits, which is the safe side for a budget.
pub fn has_benefits(contract: Option<ContractKind>) -> bool {
    !matches!(contract, Some(ContractKind::Fees))
}

/// A Mexican RFC: three letters (moral person) or four (physical), six digits of the date and a three-character
/// key. Spaces and dashes are ignored.
fn rfc_looks_right(s: &str) -> bool {
    let c: Vec<char> = s.to_uppercase().chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    if !(12..=13).contains(&c.len()) {
        return false;
    }
    let letters = c.len() - 9;
    c[..letters].iter().all(|c| c.is_ascii_uppercase() || *c == 'Ñ' || *c == '&')
        && c[letters..letters + 6].iter().all(|c| c.is_ascii_digit())
        && c[letters + 6..].iter().all(|c| c.is_ascii_alphanumeric())
}

/// Ten digits (a Mexican number), or up to thirteen with the country code or an extension.
fn phone_looks_right(s: &str) -> bool {
    (10..=13).contains(&s.chars().filter(char::is_ascii_digit).count())
}

fn email_looks_right(s: &str) -> bool {
    let s = s.trim();
    match s.split_once('@') {
        Some((user, domain)) => {
            !user.is_empty()
                && !domain.contains('@')
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !s.contains(char::is_whitespace)
        }
        None => false,
    }
}

impl ProfileInput {
    /// The sums of the profile in `year` (the year sets how long each person has worked, for the vacation premium).
    pub fn totals(&self, year: i64) -> ProfileTotals {
        let payroll_monthly_mxn: i64 =
            self.staff.iter().filter(|s| s.paid).map(|s| s.count * s.monthly_salary_mxn.unwrap_or(0)).sum();
        let mut payroll_benefits_annual_mxn = 0;
        let mut benefits_assumed = 0;
        for s in self.staff.iter().filter(|s| s.paid && s.monthly_salary_mxn.unwrap_or(0) > 0) {
            if !has_benefits(s.contract) {
                continue;
            }
            let years = s.start_year.map_or(1, |y| year - y);
            payroll_benefits_annual_mxn += s.count * annual_benefits(s.monthly_salary_mxn.unwrap_or(0), years);
            if s.contract.is_none() || s.start_year.is_none() {
                benefits_assumed += s.count;
            }
        }
        let fees_monthly_mxn: i64 =
            self.population.iter().map(|g| g.paying_count.unwrap_or(0) * g.monthly_fee_mxn.unwrap_or(0)).sum();
        ProfileTotals {
            population: self.population.iter().map(|g| g.count).sum(),
            staff_paid: self.staff.iter().filter(|s| s.paid).map(|s| s.count).sum(),
            staff_volunteer: self.staff.iter().filter(|s| !s.paid).map(|s| s.count).sum(),
            payroll_monthly_mxn,
            payroll_annual_mxn: payroll_monthly_mxn * 12,
            payroll_benefits_annual_mxn,
            payroll_cost_annual_mxn: payroll_monthly_mxn * 12 + payroll_benefits_annual_mxn,
            benefits_assumed,
            staff_support_annual_mxn: self.unpaid_staff_annual(&["religious", "trainee"]),
            external_staff_annual_mxn: self.unpaid_staff_annual(&["external"]),
            fee_payers: self.population.iter().map(|g| g.paying_count.unwrap_or(0)).sum(),
            fees_monthly_mxn,
            fees_annual_mxn: fees_monthly_mxn * 12,
        }
    }

    /// What the lines of staff outside the payroll with one of these relations cost in a year.
    fn unpaid_staff_annual(&self, relations: &[&str]) -> i64 {
        self.staff
            .iter()
            .filter(|s| !s.paid && s.relation.as_deref().is_some_and(|r| relations.contains(&r)))
            .map(|s| s.count * s.monthly_salary_mxn.unwrap_or(0) * 12)
            .sum()
    }

    /// People served counted by group (the same group may come in several lines when its fees differ).
    pub fn population_by_label(&self) -> Vec<(String, i64)> {
        let mut out: Vec<(String, i64)> = Vec::new();
        for g in &self.population {
            match out.iter_mut().find(|(l, _)| *l == g.label) {
                Some((_, n)) => *n += g.count,
                None => out.push((g.label.clone(), g.count)),
            }
        }
        out
    }

    /// Problems that stop the save (`blocking`) and heads-ups, in `year` (see `totals`).
    pub fn validate(&self, year: i64) -> Vec<ProfileIssue> {
        let mut v = Vec::new();
        let mut add = |code, field: String, blocking| v.push(ProfileIssue { code, field, blocking });
        let too_large = |n: Option<i64>| n.map_or(false, |x| x > MAX_MXN);

        if self.institution.name.trim().is_empty() {
            add("name_missing", "institution.name".into(), true);
        }
        let neg = |n: Option<i64>| n.map_or(false, |x| x < 0);
        if neg(self.capacity_total) {
            add("negative_number", "capacity_total".into(), true);
        }
        for (field, n) in [("served_estimate", self.served_estimate), ("staff_paid_estimate", self.staff_paid_estimate), ("staff_volunteer_estimate", self.staff_volunteer_estimate)] {
            if neg(n) {
                add("negative_number", field.into(), true);
            } else if n.is_some_and(|x| x > crate::core::institution::catalog::MAX_ESTIMATE) {
                add("number_too_large", field.into(), true);
            }
        }
        let inst = &self.institution;
        if let Some(d) = &inst.details {
            let age = |id: &str| d.get(id).and_then(serde_json::Value::as_i64);
            if let (Some(min), Some(max)) = (age("institution.age_min"), age("institution.age_max")) {
                if min > max {
                    add("age_range", "institution.age_max".into(), true);
                }
            }
        }
        if inst.founded_year.is_some_and(|y| !(crate::core::institution::catalog::OLDEST_YEAR..=year).contains(&y)) {
            add("year_invalid", "institution.founded_year".into(), true);
        }
        let known = |v: &Option<String>, list: &[&str]| v.as_deref().is_none_or(|x| x.trim().is_empty() || list.contains(&x));
        if !known(&inst.state, &crate::core::institution::catalog::STATES.iter().map(|(c, _)| *c).collect::<Vec<_>>()) {
            add("code_unknown", "institution.state".into(), true);
        }
        if inst.municipality.as_deref().is_some_and(|m| !m.trim().is_empty() && !crate::common::forms::is_place_name(m)) {
            add("place_name_invalid", "institution.municipality".into(), true);
        }
        for (field, value, list) in [
            ("institution.legal_form", &inst.legal_form, crate::core::institution::catalog::LEGAL_FORMS),
            ("institution.authorized_donee", &inst.authorized_donee, crate::core::institution::catalog::REGISTRY),
            ("institution.cluni", &inst.cluni, crate::core::institution::catalog::REGISTRY),
        ] {
            if !known(value, list) {
                add("code_unknown", field.into(), true);
            }
        }
        if let Some(a) = &inst.attention {
            use crate::core::institution::catalog::{CARE_AREAS, MODALITIES, POPULATIONS, SEXES_SERVED};
            for (field, codes, list) in [
                ("institution.populations", &a.populations, POPULATIONS),
                ("institution.modalities", &a.modalities, MODALITIES),
                ("institution.care_areas", &a.care_areas, CARE_AREAS),
            ] {
                if !codes.iter().all(|c| list.contains(&c.as_str())) {
                    add("code_unknown", field.into(), true);
                }
            }
            if !known(&a.sex_served, SEXES_SERVED) {
                add("code_unknown", "institution.sex_served".into(), true);
            }
        }
        if let (Some(served), Some(capacity)) = (self.served_estimate, self.capacity_total) {
            if served > capacity {
                add("served_over_capacity", "served_estimate".into(), false);
            }
        }
        for (i, g) in self.population.iter().enumerate() {
            let p = format!("population[{i}]");
            if g.label.trim().is_empty() {
                add("label_missing", format!("{p}.label"), true);
            }
            if g.count < 0 {
                add("negative_number", format!("{p}.count"), true);
            }
            if let (Some(a), Some(b)) = (g.age_min, g.age_max) {
                if a > b {
                    add("age_range", format!("{p}.age_min"), true);
                }
            }
            if neg(g.age_min) || neg(g.age_max) {
                add("negative_number", format!("{p}.age_min"), true);
            }
            if neg(g.paying_count) {
                add("negative_number", format!("{p}.paying_count"), true);
            }
            if neg(g.monthly_fee_mxn) {
                add("negative_number", format!("{p}.monthly_fee_mxn"), true);
            }
            if g.paying_count.map_or(false, |n| n > g.count) {
                add("paying_over_count", format!("{p}.paying_count"), true);
            }
        }
        for (i, s) in self.staff.iter().enumerate() {
            if s.role.trim().is_empty() {
                add("label_missing", format!("staff[{i}].role"), true);
            }
            if s.count < 0 {
                add("negative_number", format!("staff[{i}].count"), true);
            }
            if neg(s.monthly_salary_mxn) {
                add("negative_number", format!("staff[{i}].monthly_salary_mxn"), true);
            }
            if s.start_year.map_or(false, |y| !(1900..=2100).contains(&y)) {
                add("year_invalid", format!("staff[{i}].start_year"), true);
            }
        }
        for (i, s) in self.staff.iter().enumerate() {
            if too_large(s.monthly_salary_mxn) {
                add("amount_too_large", format!("staff[{i}].monthly_salary_mxn"), true);
            }
        }
        for (i, g) in self.population.iter().enumerate() {
            if too_large(g.monthly_fee_mxn) {
                add("amount_too_large", format!("population[{i}].monthly_fee_mxn"), true);
            }
        }
        if v.iter().any(|i| i.blocking) {
            // the sums below would rest on numbers that are about to be corrected
            return v;
        }
        let mut add = |code, field: String, blocking| v.push(ProfileIssue { code, field, blocking });
        // Heads-ups
        let t = self.totals(year);
        if let Some(cap) = self.capacity_total {
            if t.population > cap {
                add("population_over_capacity", "population".into(), false);
            }
        }
        let filled = |o: &Option<String>| o.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let inst = &self.institution;
        if filled(&inst.legal_rfc).is_some_and(|r| !rfc_looks_right(&r)) {
            add("rfc_format", "institution.legal_rfc".into(), false);
        }
        if filled(&inst.contact_phone).is_some_and(|p| !phone_looks_right(&p)) {
            add("phone_format", "institution.contact_phone".into(), false);
        }
        if filled(&inst.contact_email).is_some_and(|e| !email_looks_right(&e)) {
            add("email_format", "institution.contact_email".into(), false);
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> ProfileInput {
        ProfileInput {
            institution: InstitutionInput { name: "Casa Hogar Ficticia".into(), ..Default::default() },
            capacity_total: Some(25),
            population: vec![
                PopulationGroupInput { label: "Adultos mayores".into(), count: 18, ..Default::default() },
                PopulationGroupInput { label: "Con dependencia alta".into(), count: 4, ..Default::default() },
            ],
            staff: vec![
                StaffGroupInput { role: "Enfermería".into(), count: 3, paid: true, ..Default::default() },
                StaffGroupInput { role: "Voluntariado".into(), count: 5, paid: false, ..Default::default() },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn totals_are_computed_in_code() {
        let t = base().totals(2026);
        assert_eq!(
            t,
            ProfileTotals {
                population: 22,
                staff_paid: 3,
                staff_volunteer: 5,
                payroll_monthly_mxn: 0,
                payroll_annual_mxn: 0,
                payroll_benefits_annual_mxn: 0,
                payroll_cost_annual_mxn: 0,
                benefits_assumed: 0,
                staff_support_annual_mxn: 0,
                external_staff_annual_mxn: 0,
                fee_payers: 0,
                fees_monthly_mxn: 0,
                fees_annual_mxn: 0,
            }
        );
    }

    #[test]
    fn payroll_counts_only_paid_people_and_fees_only_those_who_pay() {
        let mut p = base();
        p.staff[0].monthly_salary_mxn = Some(9_000); // 3 nurses with pay
        p.staff[1].monthly_salary_mxn = Some(5_000); // volunteers: never part of the payroll
        p.population[0].paying_count = Some(10);
        p.population[0].monthly_fee_mxn = Some(2_500);
        let t = p.totals(2026);
        assert_eq!(t.payroll_monthly_mxn, 27_000);
        assert_eq!(t.payroll_annual_mxn, 324_000);
        assert_eq!((t.fee_payers, t.fees_monthly_mxn, t.fees_annual_mxn), (10, 25_000, 300_000));
    }

    #[test]
    fn more_payers_than_people_and_odd_numbers_are_blocked() {
        let mut p = base();
        p.population[0].paying_count = Some(19); // the group has 18
        p.staff[0].monthly_salary_mxn = Some(-1);
        p.staff[0].start_year = Some(20);
        let codes: Vec<_> = p.validate(2026).into_iter().filter(|i| i.blocking).map(|i| (i.code, i.field)).collect();
        assert!(codes.contains(&("paying_over_count", "population[0].paying_count".into())));
        assert!(codes.contains(&("negative_number", "staff[0].monthly_salary_mxn".into())));
        assert!(codes.contains(&("year_invalid", "staff[0].start_year".into())));
    }

    #[test]
    fn valid_profile_has_no_issues() {
        assert!(base().validate(2026).is_empty());
    }

    #[test]
    fn blocking_problems() {
        let mut p = base();
        p.institution.name = "  ".into();
        p.population[0].count = -1;
        p.population[1].age_min = Some(80);
        p.population[1].age_max = Some(60);
        p.staff[0].role.clear();
        let codes: Vec<_> = p.validate(2026).into_iter().filter(|i| i.blocking).map(|i| (i.code, i.field)).collect();
        assert!(codes.contains(&("name_missing", "institution.name".into())));
        assert!(codes.contains(&("negative_number", "population[0].count".into())));
        assert!(codes.contains(&("age_range", "population[1].age_min".into())));
        assert!(codes.contains(&("label_missing", "staff[0].role".into())));
    }

    #[test]
    fn heads_up_when_numbers_do_not_add_up() {
        let mut p = base();
        p.capacity_total = Some(20); // 22 people, 20 beds
        let issues = p.validate(2026);
        assert!(issues.iter().all(|i| !i.blocking));
        let codes: Vec<_> = issues.iter().map(|i| i.code).collect();
        assert_eq!(codes, vec!["population_over_capacity"]);
        p.capacity_total = None;
        assert!(p.validate(2026).is_empty());
    }

    #[test]
    fn amounts_out_of_reason_are_blocked() {
        let mut p = base();
        p.staff[0].monthly_salary_mxn = Some(MAX_MXN + 1);
        let codes: Vec<_> = p.validate(2026).into_iter().filter(|i| i.blocking).map(|i| (i.code, i.field)).collect();
        assert!(codes.contains(&("amount_too_large", "staff[0].monthly_salary_mxn".into())));
    }

    #[test]
    fn contact_and_rfc_that_do_not_look_right_are_a_heads_up_never_a_block() {
        let mut p = base();
        for (rfc, phone, email) in [("AHE200101AB1", "55 5555 0101", "contacto@asilo.org"), ("ROHL800101AB1", "+52 1 55 5555 0101", "a@b.mx"), ("ahe-200101-ab1", "(55) 5555-0101 ext 12", " x@y.com ")] {
            p.institution.legal_rfc = Some(rfc.into());
            p.institution.contact_phone = Some(phone.into());
            p.institution.contact_email = Some(email.into());
            assert!(p.validate(2026).is_empty(), "{rfc} {phone} {email}: {:?}", p.validate(2026));
        }
        p.institution.legal_rfc = Some("AHE2001".into());
        p.institution.contact_phone = Some("5555".into());
        p.institution.contact_email = Some("contacto.asilo.org".into());
        let issues = p.validate(2026);
        assert!(issues.iter().all(|i| !i.blocking));
        let codes: Vec<_> = issues.iter().map(|i| i.code).collect();
        assert_eq!(codes, vec!["rfc_format", "phone_format", "email_format"]);
    }

    #[test]
    fn contributions_grants_and_outside_staff_are_not_payroll() {
        let mut p = base();
        p.staff = vec![
            StaffGroupInput { role: "Pastoral".into(), count: 2, paid: false, monthly_salary_mxn: Some(1_000), relation: Some("religious".into()), ..Default::default() },
            StaffGroupInput { role: "Psicología".into(), count: 1, paid: false, monthly_salary_mxn: Some(2_000), relation: Some("trainee".into()), ..Default::default() },
            StaffGroupInput { role: "Vigilancia".into(), count: 2, paid: false, monthly_salary_mxn: Some(8_000), relation: Some("external".into()), ..Default::default() },
            StaffGroupInput { role: "Acompañamiento".into(), count: 4, paid: false, monthly_salary_mxn: Some(500), relation: Some("volunteer".into()), ..Default::default() },
        ];
        let t = p.totals(2026);
        assert_eq!((t.payroll_cost_annual_mxn, t.staff_support_annual_mxn, t.external_staff_annual_mxn), (0, 48_000, 192_000));
    }

    #[test]
    fn only_fees_carry_no_benefits() {
        assert!(has_benefits(Some(ContractKind::Permanent)) && has_benefits(Some(ContractKind::Temporary)) && has_benefits(None));
        assert!(!has_benefits(Some(ContractKind::Fees)));
    }

    fn payroll() -> ProfileInput {
        ProfileInput {
            staff: vec![
                StaffGroupInput { role: "Cocina".into(), count: 1, paid: true, monthly_salary_mxn: Some(9_000), contract: Some(ContractKind::Permanent), start_year: Some(2018), ..Default::default() },
                StaffGroupInput { role: "Psicología".into(), count: 1, paid: true, monthly_salary_mxn: Some(6_000), contract: Some(ContractKind::Fees), ..Default::default() },
                StaffGroupInput { role: "Voluntariado".into(), count: 2, paid: false, ..Default::default() },
            ],
            ..base()
        }
    }

    #[test]
    fn payroll_cost_adds_the_benefits_only_to_jobs_that_carry_them() {
        let t = payroll().totals(2026);
        assert_eq!(t.payroll_monthly_mxn, 15_000);
        assert_eq!(t.payroll_annual_mxn, 180_000);
        // cook: 8 years in 2026 -> 6,150; the psychologist works for fees: nothing
        assert_eq!(t.payroll_benefits_annual_mxn, 6_150);
        assert_eq!(t.payroll_cost_annual_mxn, 186_150);
        assert_eq!(t.benefits_assumed, 0);
    }

    #[test]
    fn missing_contract_or_start_year_is_counted_with_benefits_and_flagged_as_assumed() {
        let mut p = payroll();
        p.staff[0].contract = None;
        p.staff[0].start_year = None;
        let t = p.totals(2026);
        assert_eq!(t.payroll_benefits_annual_mxn, 5_400, "first year, with benefits");
        assert_eq!(t.benefits_assumed, 1);
        // a group of several people with the same line counts once per person
        p.staff[0].count = 3;
        assert_eq!(p.totals(2026).payroll_benefits_annual_mxn, 16_200);
        assert_eq!(p.totals(2026).benefits_assumed, 3);
    }

    #[test]
    fn the_attention_profile_decides_the_kind_and_the_first_start_still_may_give_the_kind() {
        let elderly = Attention::of_kind(InstitutionKind::ElderlyHome);
        // what the person wrote decides the kind; a mixed profile is «other»
        let mixed = Attention { populations: vec!["childhood".into(), "older_adults".into()], ..elderly.clone() };
        assert_eq!(reconcile_attention(Some((InstitutionKind::ElderlyHome, elderly.clone())), InstitutionKind::ElderlyHome, Some(&mixed)), (InstitutionKind::Other, mixed.clone()));
        // nothing sent: the saved profile stays
        assert_eq!(reconcile_attention(Some((InstitutionKind::Other, mixed.clone())), InstitutionKind::Other, None), (InstitutionKind::Other, mixed.clone()));
        // nothing sent and a kind that changes (the first start): the populations follow the kind, the rest stays
        let with_areas = Attention { care_areas: vec!["health".into()], ..elderly.clone() };
        let (kind, a) = reconcile_attention(Some((InstitutionKind::ElderlyHome, with_areas)), InstitutionKind::ChildrenHome, None);
        assert_eq!((kind, a.populations.len(), a.care_areas), (InstitutionKind::ChildrenHome, 3, vec!["health".to_string()]));
        // the first save of an older institution: filled from its kind
        assert_eq!(reconcile_attention(None, InstitutionKind::ElderlyHome, None), (InstitutionKind::ElderlyHome, elderly));
        assert_eq!(reconcile_attention(None, InstitutionKind::Other, None).1, Attention::default());
    }

    #[test]
    fn codes_of_the_attention_profile_out_of_their_list_are_blocked() {
        let mut p = base();
        p.institution.attention = Some(Attention { populations: vec!["older_adults".into(), "aliens".into()], sex_served: Some("x".into()), modalities: vec!["residential".into()], care_areas: vec![] });
        let codes: Vec<_> = p.validate(2026).into_iter().filter(|i| i.blocking).map(|i| i.field).collect();
        assert_eq!(codes, vec!["institution.populations".to_string(), "institution.sex_served".to_string()]);
        p.institution.attention = Some(Attention::of_kind(InstitutionKind::ChildrenHome));
        assert!(p.validate(2026).is_empty());
    }
}
