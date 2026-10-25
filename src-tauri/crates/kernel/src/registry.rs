//! The registry of modules (K12): every manifest that `module!` declared, gathered when the program is linked, with
//! no list anywhere. It checks each manifest and the dependencies between them; with one that breaks a rule there is
//! no registry, so a module that does not keep the shape does not enter.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::access::CLASSES;
use crate::dictionary::{Condition, Entity, Field, Kind};
use crate::module::Manifest;

/// Where `module!` puts each manifest.
#[linkme::distributed_slice]
pub static MODULES: [Manifest];

/// Ids no module may take, because other owners' tables start with them: the kernel's (`kernel_`), the AI platform's
/// (`ai_`, ADR-003 A3) and SQLite's (`sqlite_`).
const RESERVED: &[&str] = &["kernel", "ai", "sqlite"];

/// A rule of the shape of a module that a manifest breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// Not lowercase letters, digits and `_`, starting with a letter.
    BadId,
    /// An id the kernel keeps for itself.
    Reserved,
    /// Another module has the same id.
    Repeated,
    NoName,
    NoVersion,
    /// It depends on a module that is not registered.
    UnknownDependency(&'static str),
    /// It depends on itself, directly or through others.
    Cycle,
    /// A permission that is not `module.what`, with the id of this module first.
    BadPermission(&'static str),
    /// The same permission twice.
    RepeatedPermission(&'static str),
    /// Its migrations are not numbered 1, 2, 3… in order, or one has no SQL.
    MigrationsOutOfOrder,
    /// An entity of its catalog whose id is not lowercase letters, digits and `_`.
    BadEntity(&'static str),
    /// The same entity twice.
    RepeatedEntity(&'static str),
    /// A field of its catalog breaks a rule of the dictionary.
    Field { entity: &'static str, field: &'static str, fault: FieldFault },
}

/// A rule of the dictionary (K3) that a field breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldFault {
    /// Not lowercase letters, digits and `_`, starting with a letter.
    BadId,
    /// The same field twice in its entity.
    Repeated,
    /// It has no class: every datum declares one.
    NoClass,
    /// Its class is none of `kernel::CLASSES`.
    UnknownClass,
    /// A select or a multi-select without options, or options on a field of another kind.
    Options,
    /// A range whose least is above its most.
    BadRange,
    /// Its condition looks at a field the entity does not have, or at itself, or waits for no value.
    BadCondition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManifestError {
    pub module: &'static str,
    pub fault: Fault,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "module `{}`: {:?}", self.module, self.fault)
    }
}

pub struct Modules {
    /// Each module after the ones it depends on; between those that do not depend on each other, by id.
    order: Vec<&'static Manifest>,
    on: BTreeSet<&'static str>,
}

impl Modules {
    /// The registry of every module linked into the program.
    pub fn discover() -> Result<Modules, Vec<ManifestError>> {
        Modules::of(MODULES.iter())
    }

    /// The registry of these manifests, if all of them keep the rules.
    pub fn of(manifests: impl IntoIterator<Item = &'static Manifest>) -> Result<Modules, Vec<ManifestError>> {
        let mut by_id: BTreeMap<&'static str, &'static Manifest> = BTreeMap::new();
        let mut errors = Vec::new();
        for m in manifests {
            errors.extend(faults(m).into_iter().map(|fault| ManifestError { module: m.id, fault }));
            if by_id.insert(m.id, m).is_some() {
                errors.push(ManifestError { module: m.id, fault: Fault::Repeated });
            }
        }
        for m in by_id.values() {
            for dep in m.depends_on.iter().filter(|d| !by_id.contains_key(*d)) {
                errors.push(ManifestError { module: m.id, fault: Fault::UnknownDependency(dep) });
            }
        }
        let (order, stuck) = sorted(&by_id);
        errors.extend(in_cycle(&by_id, &stuck).into_iter().map(|module| ManifestError { module, fault: Fault::Cycle }));
        if !errors.is_empty() {
            return Err(errors);
        }
        // in this order a module's dependencies are decided before it
        let mut on = BTreeSet::new();
        for m in &order {
            if m.on && m.depends_on.iter().all(|d| on.contains(d)) {
                on.insert(m.id);
            }
        }
        Ok(Modules { order, on })
    }

    pub fn get(&self, id: &str) -> Option<&'static Manifest> {
        self.order.iter().copied().find(|m| m.id == id)
    }

    /// Every module, each after the ones it depends on.
    pub fn all(&self) -> impl Iterator<Item = &'static Manifest> + '_ {
        self.order.iter().copied()
    }

    /// Whether the program has this module on: its manifest says so, and so do those of everything it depends on.
    pub fn is_on(&self, id: &str) -> bool {
        self.on.contains(id)
    }

    /// The modules that are on, each after the ones it depends on.
    pub fn on(&self) -> impl Iterator<Item = &'static Manifest> + '_ {
        self.all().filter(|m| self.is_on(m.id))
    }

    /// The catalog of K2: the kernel's general permissions, then every permission the modules define.
    pub fn permissions(&self) -> impl Iterator<Item = &'static crate::Permission> + '_ {
        crate::access::GENERAL.iter().chain(self.all().flat_map(|m| m.permissions.iter()))
    }

    /// The permission with this id, if the kernel or a module defines it.
    pub fn permission(&self, id: &str) -> Option<&'static crate::Permission> {
        self.permissions().find(|p| p.id == id)
    }

    /// The entity of the dictionary this module declares with this id.
    pub fn entity(&self, module: &str, entity: &str) -> Option<&'static Entity> {
        self.get(module)?.entities.iter().find(|e| e.id == entity)
    }

    /// The field of the dictionary with this full id: `module.entity.field` (`muestra.note.phone`). With it the
    /// policy and the output of a capability take the class of a datum from where it is declared (ADR-003 A5).
    pub fn field(&self, id: &str) -> Option<&'static Field> {
        let mut parts = id.split('.');
        let (Some(module), Some(entity), Some(field), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else { return None };
        self.entity(module, entity)?.field(field)
    }
}

/// The rules one manifest breaks by itself.
fn faults(m: &'static Manifest) -> Vec<Fault> {
    let mut out = Vec::new();
    if !well_formed(m.id) {
        out.push(Fault::BadId);
    }
    if RESERVED.contains(&m.id) {
        out.push(Fault::Reserved);
    }
    if m.name.trim().is_empty() {
        out.push(Fault::NoName);
    }
    if m.version.trim().is_empty() {
        out.push(Fault::NoVersion);
    }
    let mut seen = BTreeSet::new();
    for p in m.permissions {
        let ok = p.id.split_once('.').is_some_and(|(module, rest)| module == m.id && rest.split('.').all(well_formed));
        if !ok {
            out.push(Fault::BadPermission(p.id));
        }
        if !seen.insert(p.id) {
            out.push(Fault::RepeatedPermission(p.id));
        }
    }
    let numbered = m.migrations.iter().enumerate().all(|(i, mig)| mig.version == i as i64 + 1 && !mig.sql.trim().is_empty());
    if !numbered {
        out.push(Fault::MigrationsOutOfOrder);
    }
    let mut entities = BTreeSet::new();
    for e in m.entities {
        out.extend(field_faults(e).into_iter().map(|(field, fault)| Fault::Field { entity: e.id, field, fault }));
        if !well_formed(e.id) {
            out.push(Fault::BadEntity(e.id));
        }
        if !entities.insert(e.id) {
            out.push(Fault::RepeatedEntity(e.id));
        }
    }
    out
}

/// The rules of the dictionary the fields of one entity break.
fn field_faults(e: &Entity) -> Vec<(&'static str, FieldFault)> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for f in e.fields {
        let mut fault = |x| out.push((f.id, x));
        if !well_formed(f.id) {
            fault(FieldFault::BadId);
        }
        if !seen.insert(f.id) {
            fault(FieldFault::Repeated);
        }
        if f.class.trim().is_empty() {
            fault(FieldFault::NoClass);
        } else if !CLASSES.contains(&f.class) {
            fault(FieldFault::UnknownClass);
        }
        if matches!(f.kind, Kind::Select | Kind::MultiSelect) == f.options.is_empty() {
            fault(FieldFault::Options);
        }
        if f.min.zip(f.max).is_some_and(|(min, max)| min > max) {
            fault(FieldFault::BadRange);
        }
        let looks_wrong = f.applies_when.field().is_some_and(|other| other == f.id || e.field(other).is_none());
        if looks_wrong || matches!(f.applies_when, Condition::AnyOf { values: &[], .. }) {
            fault(FieldFault::BadCondition);
        }
    }
    out
}

/// Lowercase letters, digits and `_`, starting with a letter.
fn well_formed(part: &str) -> bool {
    part.starts_with(|c: char| c.is_ascii_lowercase()) && part.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The modules in dependency order, and the ones that could not be placed because a cycle holds them.
fn sorted(by_id: &BTreeMap<&'static str, &'static Manifest>) -> (Vec<&'static Manifest>, BTreeSet<&'static str>) {
    let known = |m: &Manifest| m.depends_on.iter().filter(|d| by_id.contains_key(*d)).count();
    let mut waiting: BTreeMap<&'static str, usize> = by_id.iter().map(|(id, m)| (*id, known(m))).collect();
    let mut order = Vec::new();
    while let Some(id) = waiting.iter().find(|(_, n)| **n == 0).map(|(id, _)| *id) {
        waiting.remove(id);
        order.push(by_id[id]);
        for (other, n) in waiting.iter_mut() {
            *n -= by_id[other].depends_on.iter().filter(|d| **d == id).count();
        }
    }
    (order, waiting.into_keys().collect())
}

/// Of the modules a cycle holds, the ones that are in it (and not only after it).
fn in_cycle(by_id: &BTreeMap<&'static str, &'static Manifest>, stuck: &BTreeSet<&'static str>) -> Vec<&'static str> {
    let reaches_itself = |start: &'static str| {
        let mut seen = BTreeSet::new();
        let mut next: Vec<&'static str> = by_id[start].depends_on.to_vec();
        while let Some(id) = next.pop() {
            if id == start {
                return true;
            }
            if stuck.contains(id) && seen.insert(id) {
                next.extend(by_id[id].depends_on);
            }
        }
        false
    };
    stuck.iter().copied().filter(|id| reaches_itself(id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::{Condition, Entity, Field, Kind};
    use crate::{Migration, Permission};

    const fn manifest(id: &'static str, depends_on: &'static [&'static str], on: bool) -> Manifest {
        Manifest { id, name: "Módulo", version: "0.1.0", depends_on, permissions: &[], migrations: &[], entities: &[], on }
    }

    fn faults_of(manifests: &'static [Manifest]) -> Vec<(&'static str, Fault)> {
        match Modules::of(manifests) {
            Ok(_) => vec![],
            Err(errors) => errors.into_iter().map(|e| (e.module, e.fault)).collect(),
        }
    }

    #[test]
    fn modules_come_after_the_ones_they_depend_on() {
        static ALL: [Manifest; 3] = [manifest("proyectos", &["contabilidad"], true), manifest("donantes", &["contabilidad"], true), manifest("contabilidad", &[], true)];
        let modules = Modules::of(&ALL).unwrap();
        let ids: Vec<_> = modules.all().map(|m| m.id).collect();
        assert_eq!(ids, ["contabilidad", "donantes", "proyectos"]);
        assert_eq!(modules.get("donantes").map(|m| m.id), Some("donantes"));
        assert!(modules.get("compras").is_none());
    }

    #[test]
    fn a_dependency_that_does_not_exist_is_refused() {
        static ALL: [Manifest; 1] = [manifest("proyectos", &["contabilidad"], true)];
        assert_eq!(faults_of(&ALL), [("proyectos", Fault::UnknownDependency("contabilidad"))]);
    }

    #[test]
    fn a_cycle_is_refused_and_names_who_is_in_it() {
        static ALL: [Manifest; 4] = [
            manifest("a", &["b"], true),
            manifest("b", &["c"], true),
            manifest("c", &["a"], true),
            // held by the cycle, but not in it
            manifest("d", &["a"], true),
        ];
        assert_eq!(faults_of(&ALL), [("a", Fault::Cycle), ("b", Fault::Cycle), ("c", Fault::Cycle)]);
        static ITSELF: [Manifest; 1] = [manifest("a", &["a"], true)];
        assert_eq!(faults_of(&ITSELF), [("a", Fault::Cycle)]);
    }

    #[test]
    fn a_module_is_off_when_what_it_depends_on_is_off() {
        static ALL: [Manifest; 4] = [
            manifest("contabilidad", &[], false),
            manifest("proyectos", &["contabilidad"], true),
            manifest("mi_institucion", &[], true),
            manifest("personal", &["mi_institucion"], false),
        ];
        let modules = Modules::of(&ALL).unwrap();
        let on: Vec<_> = modules.on().map(|m| m.id).collect();
        assert_eq!(on, ["mi_institucion"]);
        assert!(!modules.is_on("proyectos") && !modules.is_on("personal") && !modules.is_on("compras"));
    }

    #[test]
    fn the_same_id_twice_is_refused() {
        static ALL: [Manifest; 2] = [manifest("contabilidad", &[], true), manifest("contabilidad", &[], true)];
        assert_eq!(faults_of(&ALL), [("contabilidad", Fault::Repeated)]);
    }

    #[test]
    fn a_manifest_that_breaks_the_shape_says_what_it_breaks() {
        static ALL: [Manifest; 5] = [
            Manifest { id: "Mi-Institucion", ..manifest("x", &[], true) },
            Manifest { id: "kernel", name: " ", version: "", ..manifest("x", &[], true) },
            manifest("ai", &[], true),
            Manifest {
                permissions: &[
                    Permission { id: "contabilidad.polizas.registrar", description: "Registrar pólizas." },
                    Permission { id: "contabilidad.polizas.registrar", description: "Otra vez." },
                    Permission { id: "proyectos.leer", description: "De otro módulo." },
                    Permission { id: "contabilidad", description: "Sin qué." },
                    Permission { id: "contabilidad.Polizas", description: "Con mayúscula." },
                ],
                ..manifest("contabilidad", &[], true)
            },
            Manifest {
                migrations: &[Migration { version: 1, sql: "CREATE TABLE donantes_a (id TEXT);" }, Migration { version: 3, sql: "SELECT 1;" }],
                ..manifest("donantes", &[], true)
            },
        ];
        assert_eq!(
            faults_of(&ALL),
            [
                ("Mi-Institucion", Fault::BadId),
                ("kernel", Fault::Reserved),
                ("kernel", Fault::NoName),
                ("kernel", Fault::NoVersion),
                ("ai", Fault::Reserved),
                ("contabilidad", Fault::RepeatedPermission("contabilidad.polizas.registrar")),
                ("contabilidad", Fault::BadPermission("proyectos.leer")),
                ("contabilidad", Fault::BadPermission("contabilidad")),
                ("contabilidad", Fault::BadPermission("contabilidad.Polizas")),
                ("donantes", Fault::MigrationsOutOfOrder),
            ]
        );
    }

    #[test]
    fn the_permissions_of_every_module_are_known() {
        static ALL: [Manifest; 2] = [
            Manifest { permissions: &[Permission { id: "contabilidad.polizas.registrar", description: "Registrar pólizas." }], ..manifest("contabilidad", &[], true) },
            Manifest { permissions: &[Permission { id: "donantes.recibos.emitir", description: "Emitir recibos." }], ..manifest("donantes", &[], false) },
        ];
        let modules = Modules::of(&ALL).unwrap();
        let ids: Vec<_> = modules.permissions().map(|p| p.id).filter(|id| !id.starts_with("kernel.")).collect();
        assert_eq!(ids, ["contabilidad.polizas.registrar", "donantes.recibos.emitir"]);
        // a module that is off still defines its permissions: turning it on does not change the catalog
        assert_eq!(modules.permission("donantes.recibos.emitir").map(|p| p.description), Some("Emitir recibos."));
        assert!(modules.permission("donantes.recibos.borrar").is_none());
    }

    #[test]
    fn a_catalog_that_breaks_the_dictionary_says_what_it_breaks() {
        static ALL: [Manifest; 1] = [Manifest {
            entities: &[
                Entity {
                    id: "donor",
                    fields: &[
                        Field::new("name", Kind::Text, "personal"),
                        Field::new("name", Kind::Text, "personal"),
                        Field::new("Nombre", Kind::Text, "personal"),
                        Field::new("rfc", Kind::Text, ""),
                        Field::new("phone", Kind::Phone, "secreto"),
                        Field::new("kind", Kind::Select, "internal"),
                        Field::new("since", Kind::Year, "internal").options(&["a"]),
                        Field::new("amount", Kind::Money, "internal").range(10, 1),
                        Field::new("cause", Kind::Text, "internal").when(Condition::Filled { field: "color" }),
                        Field::new("again", Kind::Text, "internal").when(Condition::Filled { field: "again" }),
                        Field::new("which", Kind::Text, "internal").when(Condition::AnyOf { field: "name", values: &[] }),
                    ],
                },
                Entity { id: "Donativo", fields: &[] },
                Entity { id: "donor", fields: &[] },
            ],
            ..manifest("donantes", &[], true)
        }];
        let field = |field, fault| ("donantes", Fault::Field { entity: "donor", field, fault });
        assert_eq!(
            faults_of(&ALL),
            [
                field("name", FieldFault::Repeated),
                field("Nombre", FieldFault::BadId),
                field("rfc", FieldFault::NoClass),
                field("phone", FieldFault::UnknownClass),
                field("kind", FieldFault::Options),
                field("since", FieldFault::Options),
                field("amount", FieldFault::BadRange),
                field("cause", FieldFault::BadCondition),
                field("again", FieldFault::BadCondition),
                field("which", FieldFault::BadCondition),
                ("donantes", Fault::BadEntity("Donativo")),
                ("donantes", Fault::RepeatedEntity("donor")),
            ]
        );
    }

    #[test]
    fn a_field_is_found_by_its_full_id_with_its_class() {
        static ALL: [Manifest; 1] = [Manifest {
            entities: &[Entity { id: "donor", fields: &[Field::new("name", Kind::Text, "personal"), Field::new("since", Kind::Year, "internal")] }],
            ..manifest("donantes", &[], true)
        }];
        let modules = Modules::of(&ALL).unwrap();
        assert_eq!(modules.entity("donantes", "donor").map(|e| e.fields.len()), Some(2));
        assert_eq!(modules.field("donantes.donor.name").map(|f| f.class), Some("personal"));
        assert_eq!(modules.field("donantes.donor.since").map(|f| f.class), Some("internal"));
        for unknown in ["donantes.donor.color", "donantes.gift.name", "proyectos.donor.name", "donantes.donor", "donantes.donor.name.x", ""] {
            assert!(modules.field(unknown).is_none(), "{unknown}");
        }
    }

    #[test]
    fn nothing_is_registered_unless_a_module_is_linked() {
        assert_eq!(Modules::discover().unwrap().all().count(), 0);
    }
}
