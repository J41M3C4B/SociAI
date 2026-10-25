//! The registry (ADR-001 §6): every capability that `capability!` declared, gathered when the program is linked, with
//! no list anywhere. It checks each contract once; with one that breaks a rule there is no registry.

use std::collections::BTreeMap;
use std::fmt;

use super::contract::{Approval, Capability, Effect, Run};

/// Where `capability!` puts each contract.
#[linkme::distributed_slice]
pub static CAPABILITIES: [Capability];

/// A rule of ADR-001 §2 that a contract breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// Not `module.name` with lowercase letters, digits and `_`.
    BadId,
    /// Another capability has the same id.
    Repeated,
    NoDescription,
    NoDataClass,
    SensitiveWithoutApproval,
    NoTimeout,
    /// A change that is not idempotent may not be tried again.
    RetriesWithoutIdempotence,
    InputNotAnObject,
    /// A query or transformation that runs as a change, or the other way around.
    RunDoesNotFitEffect,
    /// The permission is not written as an id: lowercase letters, digits and `_`, in two parts or more joined by dots.
    BadPermission,
    /// Neither the kernel nor any module defines the permission. The registry does not know the catalog of
    /// permissions; the platform checks it against the kernel when the program starts.
    UnknownPermission,
    /// A modification whose `prepare` does not give the kernel's proposal (`kernel::Proposal`). The registry does not
    /// know the kernel either; the platform checks it when the program starts.
    PrepareWithoutProposal,
    /// Its output names a field the data dictionary does not have. The registry does not know the dictionary (K3);
    /// the platform checks it when the program starts.
    UnknownField,
    /// Its output names a field whose class is not in its `data`, which is the most it may touch (ADR-003 A5).
    ClassOutsideData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContractError {
    pub id: &'static str,
    pub fault: Fault,
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "capability `{}`: {:?}", self.id, self.fault)
    }
}

pub struct Registry {
    by_id: BTreeMap<&'static str, &'static Capability>,
}

impl Registry {
    /// The registry of every capability of the program.
    pub fn discover() -> Result<Registry, Vec<ContractError>> {
        Registry::of(CAPABILITIES.iter())
    }

    /// The registry of these contracts, if all of them keep the rules.
    pub fn of(contracts: impl IntoIterator<Item = &'static Capability>) -> Result<Registry, Vec<ContractError>> {
        let mut by_id = BTreeMap::new();
        let mut errors = Vec::new();
        for c in contracts {
            errors.extend(faults(c).into_iter().map(|fault| ContractError { id: c.id, fault }));
            if by_id.insert(c.id, c).is_some() {
                errors.push(ContractError { id: c.id, fault: Fault::Repeated });
            }
        }
        if errors.is_empty() {
            Ok(Registry { by_id })
        } else {
            Err(errors)
        }
    }

    pub fn get(&self, id: &str) -> Option<&'static Capability> {
        self.by_id.get(id).copied()
    }

    /// Every capability, by id.
    pub fn all(&self) -> impl Iterator<Item = &'static Capability> + '_ {
        self.by_id.values().copied()
    }

    pub fn of_module<'a>(&'a self, module: &'a str) -> impl Iterator<Item = &'static Capability> + 'a {
        self.all().filter(move |c| c.module() == module)
    }

    pub fn with_effect(&self, effect: Effect) -> impl Iterator<Item = &'static Capability> + '_ {
        self.all().filter(move |c| c.effect == effect)
    }
}

/// The rules one contract breaks by itself.
fn faults(c: &Capability) -> Vec<Fault> {
    let mut out = Vec::new();
    if !well_formed(c.id) {
        out.push(Fault::BadId);
    }
    if c.description.trim().is_empty() {
        out.push(Fault::NoDescription);
    }
    if c.data.is_empty() {
        out.push(Fault::NoDataClass);
    }
    if c.effect == Effect::Sensitive && c.approval != Approval::Required {
        out.push(Fault::SensitiveWithoutApproval);
    }
    if c.limits.timeout.is_zero() {
        out.push(Fault::NoTimeout);
    }
    if c.effect.changes() && !c.limits.idempotent && c.limits.retries > 0 {
        out.push(Fault::RetriesWithoutIdempotence);
    }
    if (c.input)().get("type").and_then(|t| t.as_str()) != Some("object") {
        out.push(Fault::InputNotAnObject);
    }
    if c.effect.changes() != matches!(c.run, Run::Change { .. }) {
        out.push(Fault::RunDoesNotFitEffect);
    }
    if !(c.permission.contains('.') && c.permission.split('.').all(part)) {
        out.push(Fault::BadPermission);
    }
    out
}

/// One part of an id: it starts with a lowercase letter, then lowercase letters, digits and `_`.
fn part(p: &str) -> bool {
    p.starts_with(|c: char| c.is_ascii_lowercase()) && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `module.name`: two parts.
fn well_formed(id: &str) -> bool {
    matches!(id.split_once('.'), Some((m, n)) if part(m) && part(n))
}
