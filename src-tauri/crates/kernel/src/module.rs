//! The manifest: what a module says about itself (the ERP blueprint §4). The registry reads it, and nothing else of
//! the module.

use crate::access::Permission;
use crate::dictionary::Entity;

#[derive(Debug)]
pub struct Manifest {
    /// `mi_institucion`, `contabilidad`: lowercase letters, digits and `_`. Its tables and its permissions start with it.
    pub id: &'static str,
    /// Its name for people, in Spanish.
    pub name: &'static str,
    /// Its version: changing what it publishes (its `contract`) is changing it.
    pub version: &'static str,
    /// The modules it uses, by id: their published types and their capabilities, never anything else.
    pub depends_on: &'static [&'static str],
    /// The permissions it defines.
    pub permissions: &'static [Permission],
    /// Its migrations, numbered inside the module from 1, in order. Once applied, a migration is never edited.
    pub migrations: &'static [Migration],
    /// Its part of the data dictionary (K3): its entities and their fields, each with its class.
    pub entities: &'static [Entity],
    /// Whether the program has it on. A module that depends on one that is off is off too.
    pub on: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub version: i64,
    pub sql: &'static str,
}
