//! Accounts, roles and permissions (K2). Every permission has an id: the kernel's general ones (`kernel.usar`) and the
//! ones each module declares in its manifest, named after it (`contabilidad.polizas.registrar`). A capability names
//! the permission it needs by that id (ADR-003, A2), so the contract does not depend on the kernel. The roles group
//! permissions and say which classes of data they see, and `authorize` is the one decision: may this account do this,
//! in this organization? It is pure: whoever asks brings the account as it is now.

use crate::registry::Modules;

/// A permission, in the catalog of K2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permission {
    /// `owner.what[.more]`: `kernel` or the id of the module, then lowercase parts.
    pub id: &'static str,
    /// What it lets a person do, in Spanish, for whoever gives the roles.
    pub description: &'static str,
}

/// The kernel's own permissions: what every module shares. They were the four general permissions of the app.
pub const GENERAL: &[Permission] = &[
    Permission { id: "kernel.usar", description: "Trabajar con el programa: el perfil, los documentos, los respaldos y lo demás." },
    Permission { id: "kernel.borrar", description: "Borrar algo para siempre. Sin este permiso, borrar se vuelve una solicitud." },
    Permission { id: "kernel.ajustes", description: "Los ajustes técnicos: el proveedor de IA, su llave y su tope, restaurar un respaldo." },
    Permission { id: "kernel.administrar", description: "El panel de administración: las cuentas, las solicitudes y la bitácora." },
];

/// The classes of data, by id, the same as `DataClass` of the capability contract. What each one allows is fixed by
/// ADR-004 and decided by the platform's policy (IA-3).
pub const CLASSES: &[&str] = &["public", "internal", "confidential", "personal", "sensitive_personal"];

/// The classes whose values the history keeps. A field of any other class is protected: its history says it changed,
/// never its value (K5).
pub const KEPT_IN_HISTORY: &[&str] = &["public", "internal"];

/// What a role may do or see: all of it, only these ids, or all but these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    All,
    Only(&'static [&'static str]),
    Except(&'static [&'static str]),
}

impl Scope {
    fn covers(self, id: &str) -> bool {
        match self {
            Scope::All => true,
            Scope::Only(ids) => ids.contains(&id),
            Scope::Except(ids) => !ids.contains(&id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Role {
    /// As the accounts keep it.
    pub id: &'static str,
    /// Its name for people, in Spanish.
    pub name: &'static str,
    /// The permissions it has, by id.
    pub grants: Scope,
    /// The classes of data it sees, by id.
    pub sees: Scope,
}

impl Role {
    /// Whether the role has this permission, without asking whether the permission exists.
    pub fn grants(&self, permission: &str) -> bool {
        self.grants.covers(permission)
    }
}

/// The roles of today, which are the two of the app.
pub const ROLES: &[Role] = &[
    // the technical administrator: everything, including the administration panel
    Role { id: "admin", name: "Administración", grants: Scope::All, sees: Scope::All },
    // direction and accounting: the whole program, the permissions of every module too, but not the panel, the
    // technical settings nor deleting for good
    Role { id: "manager", name: "Dirección", grants: Scope::Except(&["kernel.borrar", "kernel.ajustes", "kernel.administrar"]), sees: Scope::All },
];

pub fn role(id: &str) -> Option<&'static Role> {
    ROLES.iter().find(|r| r.id == id)
}

/// An account as it is now, for the decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    /// The organization it belongs to (D4: one database per institution).
    pub organization: String,
    /// The id of its role.
    pub role: String,
    /// A deactivated account may do nothing.
    pub active: bool,
    /// Until it changes its password, it may do nothing that needs a permission.
    pub must_change_password: bool,
}

/// Why `authorize` says no.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denial {
    /// No module defines the permission, nor the kernel.
    UnknownPermission,
    UnknownRole,
    Inactive,
    MustChangePassword,
    /// The account belongs to another organization.
    OtherOrganization,
    /// The role of the account does not have the permission.
    NotGranted,
}

impl Denial {
    /// The code of the reason, for the audit and for the screens.
    pub fn code(self) -> &'static str {
        match self {
            Denial::UnknownPermission => "unknown_permission",
            Denial::UnknownRole => "unknown_role",
            Denial::Inactive => "account_inactive",
            Denial::MustChangePassword => "must_change_password",
            Denial::OtherOrganization => "other_organization",
            Denial::NotGranted => "access_denied",
        }
    }
}

/// May this account use this permission in this organization? Nothing passes by default: the permission has to
/// exist, the account has to be active, of this organization and with a role that has it.
pub fn authorize(modules: &Modules, account: &Account, organization: &str, permission: &str) -> Result<(), Denial> {
    if modules.permission(permission).is_none() {
        return Err(Denial::UnknownPermission);
    }
    let role = role(&account.role).ok_or(Denial::UnknownRole)?;
    if !account.active {
        return Err(Denial::Inactive);
    }
    if account.must_change_password {
        return Err(Denial::MustChangePassword);
    }
    if account.organization != organization {
        return Err(Denial::OtherOrganization);
    }
    if !role.grants(permission) {
        return Err(Denial::NotGranted);
    }
    Ok(())
}

/// The classes of data this account sees, by id: none if it is deactivated or its role does not exist.
pub fn classes_seen(account: &Account) -> Vec<&'static str> {
    match role(&account.role) {
        Some(role) if account.active => CLASSES.iter().copied().filter(|c| role.sees.covers(c)).collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Manifest;

    static SAMPLE: Manifest = Manifest {
        id: "contabilidad",
        name: "Contabilidad",
        version: "0.1.0",
        depends_on: &[],
        permissions: &[Permission { id: "contabilidad.polizas.registrar", description: "Registrar pólizas." }],
        migrations: &[],
        entities: &[],
        on: true,
    };

    fn account(role: &str) -> Account {
        Account { id: "usr_01".into(), organization: "inst_a".into(), role: role.into(), active: true, must_change_password: false }
    }

    #[test]
    fn the_catalog_has_the_kernels_permissions_and_the_modules() {
        let modules = Modules::of([&SAMPLE]).unwrap();
        let ids: Vec<_> = modules.permissions().map(|p| p.id).collect();
        assert_eq!(ids, ["kernel.usar", "kernel.borrar", "kernel.ajustes", "kernel.administrar", "contabilidad.polizas.registrar"]);
    }

    #[test]
    fn the_two_roles_of_today_can_what_they_could() {
        let admin = role("admin").unwrap();
        let manager = role("manager").unwrap();
        for p in GENERAL {
            assert!(admin.grants(p.id), "{}", p.id);
        }
        assert!(manager.grants("kernel.usar"));
        assert!(!manager.grants("kernel.borrar") && !manager.grants("kernel.ajustes") && !manager.grants("kernel.administrar"));
        // the permissions of a module are the program's: both roles have them (ERP-1, Mi institución)
        assert!(admin.grants("contabilidad.polizas.registrar") && manager.grants("contabilidad.polizas.registrar"));
        assert!(role("contador").is_none());
    }

    #[test]
    fn the_question_says_yes_only_when_everything_fits() {
        let modules = Modules::of([&SAMPLE]).unwrap();
        let ask = |a: &Account, org: &str, p: &str| authorize(&modules, a, org, p);
        assert_eq!(ask(&account("manager"), "inst_a", "kernel.usar"), Ok(()));
        assert_eq!(ask(&account("admin"), "inst_a", "contabilidad.polizas.registrar"), Ok(()));
        assert_eq!(ask(&account("manager"), "inst_a", "contabilidad.polizas.registrar"), Ok(()));
        assert_eq!(ask(&account("manager"), "inst_a", "kernel.administrar"), Err(Denial::NotGranted));
        assert_eq!(ask(&account("admin"), "inst_a", "contabilidad.polizas.borrar"), Err(Denial::UnknownPermission));
        assert_eq!(ask(&account("contador"), "inst_a", "kernel.usar"), Err(Denial::UnknownRole));
        assert_eq!(ask(&Account { active: false, ..account("admin") }, "inst_a", "kernel.usar"), Err(Denial::Inactive));
        assert_eq!(ask(&Account { must_change_password: true, ..account("admin") }, "inst_a", "kernel.usar"), Err(Denial::MustChangePassword));
        assert_eq!(ask(&account("admin"), "inst_b", "kernel.usar"), Err(Denial::OtherOrganization));
    }

    #[test]
    fn an_account_sees_the_classes_of_its_role() {
        assert_eq!(classes_seen(&account("manager")), CLASSES);
        assert_eq!(classes_seen(&account("admin")), CLASSES);
        assert!(classes_seen(&Account { active: false, ..account("admin") }).is_empty());
        assert!(classes_seen(&account("contador")).is_empty());
    }

    #[test]
    fn every_denial_has_a_code() {
        use Denial::*;
        let codes: std::collections::BTreeSet<_> =
            [UnknownPermission, UnknownRole, Inactive, MustChangePassword, OtherOrganization, NotGranted].map(Denial::code).into();
        assert_eq!(codes.len(), 6);
    }
}
