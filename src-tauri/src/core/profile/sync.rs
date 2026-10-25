//! The profile keeps anonymous lines of the staff (ADR-027) and of the people served (ADR-029), computed by their
//! modules. This writes them into the current profile after every change, and gives the sums with them. No name,
//! phone, mail or identifier ever comes in here.

use crate::core::profile::domain::{ContractKind, DependencyLevel, PopulationGroupInput, ProfileInput, ProfileTotals, StaffGroupInput};
use crate::core::profile::service::ProfileView;
use crate::core::error::ServiceError;
use crate::core::profile::storage as profile_store;
use rusqlite::Connection;

/// The anonymous lines of both modules, as the profile keeps them.
pub fn derive(conn: &Connection) -> Result<(Vec<StaffGroupInput>, Vec<PopulationGroupInput>), ServiceError> {
    let staff = crate::modules::hr::api::staff_lines(conn)?
        .into_iter()
        .map(|l| StaffGroupInput {
            role: l.role,
            count: l.count,
            shift: l.shift,
            paid: l.paid,
            monthly_salary_mxn: l.monthly_pay,
            contract: l.contract.and_then(ContractKind::from_db),
            start_year: l.start_year,
            notes: None,
            relation: Some(l.relation.as_str().into()),
        })
        .collect();
    let population = crate::modules::care::api::population_lines(conn, crate::core::institution::care_flavor(conn))?
        .into_iter()
        .map(|l| PopulationGroupInput {
            label: l.label,
            age_min: l.age_min,
            age_max: l.age_max,
            count: l.count,
            dependency_level: l.dependency.as_deref().and_then(DependencyLevel::from_db),
            notes: None,
            paying_count: (l.paying > 0).then_some(l.paying),
            monthly_fee_mxn: l.monthly_fee,
        })
        .collect();
    Ok((staff, population))
}

/// The totals with what the modules say now, and the rest of the profile (its income) when there is one.
pub(crate) fn totals(conn: &Connection) -> Result<ProfileTotals, ServiceError> {
    let (staff, population) = derive(conn)?;
    let (input, year) = match profile_store::load_current(conn)? {
        Some(p) => (p.input, p.as_of_year),
        None => (ProfileInput::default(), profile_store::current_year(conn)?),
    };
    Ok(ProfileInput { staff, population, ..input }.totals(year))
}

/// Writes the new lines in place (audit D1).
pub(crate) fn sync_profile(conn: &mut Connection) -> Result<Option<ProfileView>, ServiceError> {
    let (staff, population) = derive(conn)?;
    Ok(profile_store::refresh_lines(conn, &staff, &population)?.map(Into::into))
}

/// Development only: replaces the staff and the people served with the fictitious ones of an example
/// (`fixtures/padron-*.json`), moved as the old roster was.
#[cfg(debug_assertions)]
pub fn seed_examples(conn: &mut Connection, raw: &str) -> Result<(), ServiceError> {
    use std::collections::BTreeMap;
    #[derive(serde::Deserialize)]
    struct Example {
        staff: Vec<BTreeMap<String, String>>,
        beneficiary: Vec<BTreeMap<String, String>>,
    }
    let example: Example = serde_json::from_str(raw).map_err(|e| ServiceError::Internal(e.to_string()))?;
    let flavor = crate::core::institution::hr_flavor(conn);
    let year = profile_store::current_year(conn)?;
    let tx = conn.transaction()?;
    crate::modules::hr::api::load_example(&tx, flavor, &example.staff)?;
    crate::modules::care::api::load_example(&tx, year, example.beneficiary)?;
    tx.commit()?;
    Ok(())
}
