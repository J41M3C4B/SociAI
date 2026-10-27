//! Access profiles (ADR-028): who may do what, by code. A role is a list of permissions, and every command of the app
//! says which permission it needs. Nothing here touches the database.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The technical administrator: everything, including the administration panel.
    Admin,
    /// Direction and accounting: the whole app, but not the panel, the technical settings or permanent deletions.
    Manager,
}

impl Role {
    pub fn as_db(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Manager => "manager",
        }
    }
    pub fn from_db(s: &str) -> Option<Self> {
        match s {
            "admin" => Some(Role::Admin),
            "manager" => Some(Role::Manager),
            _ => None,
        }
    }
    /// What the screens are told the role may do; a test keeps it equal to what the kernel says.
    pub fn permissions(self) -> &'static [Permission] {
        match self {
            Role::Admin => &[Permission::Use, Permission::Delete, Permission::Settings, Permission::Administer],
            Role::Manager => &[Permission::Use],
        }
    }
    /// The roles and what they may do live in the kernel (K2, ERP-10).
    pub fn can(self, p: Permission) -> bool {
        kernel::role(self.as_db()).is_some_and(|r| r.grants(p.id()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Work with the app: profile, staff, projects, AI, documents, backups.
    Use,
    /// Delete something for good. Without it, a deletion becomes a request and the thing is hidden meanwhile.
    Delete,
    /// Technical settings: AI provider, key and spending cap, restoring a backup, example data.
    Settings,
    /// The administration panel: accounts, requests, audit log.
    Administer,
}

impl Permission {
    pub const ALL: [Permission; 4] = [Permission::Use, Permission::Delete, Permission::Settings, Permission::Administer];

    /// Its id in the catalog of the kernel (K2), where it is one of the general permissions.
    pub fn id(self) -> &'static str {
        match self {
            Permission::Use => "kernel.usar",
            Permission::Delete => "kernel.borrar",
            Permission::Settings => "kernel.ajustes",
            Permission::Administer => "kernel.administrar",
        }
    }
}

/// What a command needs before it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// Nothing: it is what lets a person in (or says whether the app needs its first account).
    Open,
    /// A session, even one that must change its password first.
    Session,
    Permission(Permission),
    /// A deletion: done when the person may delete, turned into a request otherwise.
    DeleteOrRequest,
}

/// Every command of the app and what it needs. A test checks this list against the registered commands, so a new
/// command cannot slip in without saying it.
pub const COMMANDS: &[(&str, Need)] = &[
    ("app_info", Need::Open),
    // access
    ("access_status", Need::Open),
    ("access_setup_admin", Need::Open),
    ("access_login", Need::Open),
    ("access_recover", Need::Open),
    ("access_unlock", Need::Open),
    ("access_lock", Need::Session),
    ("access_logout", Need::Session),
    ("access_change_password", Need::Session),
    ("access_team", Need::Permission(Permission::Use)),
    ("admin_overview", Need::Permission(Permission::Administer)),
    ("admin_user_create", Need::Permission(Permission::Administer)),
    ("admin_user_update", Need::Permission(Permission::Administer)),
    ("admin_user_reset_password", Need::Permission(Permission::Administer)),
    ("admin_request_resolve", Need::Permission(Permission::Administer)),
    ("admin_audit", Need::Permission(Permission::Administer)),
    ("admin_recovery_code_new", Need::Permission(Permission::Administer)),
    // the institution
    ("profile_get", Need::Permission(Permission::Use)),
    ("form_get", Need::Permission(Permission::Use)),
    ("institution_overview", Need::Permission(Permission::Use)),
    ("form_save", Need::Permission(Permission::Use)),
    // the board and the bank accounts of the institution (ADR-033 §3)
    ("institution_lists", Need::Permission(Permission::Use)),
    ("institution_list_form", Need::Permission(Permission::Use)),
    ("institution_list_save", Need::Permission(Permission::Use)),
    ("institution_list_set_active", Need::Permission(Permission::Use)),
    ("institution_list_delete", Need::DeleteOrRequest),
    // the record of the institution (ADR-033 §5)
    ("institution_records", Need::Permission(Permission::Use)),
    ("institution_record_form", Need::Permission(Permission::Use)),
    ("institution_record_save", Need::Permission(Permission::Use)),
    ("institution_record_attach", Need::Permission(Permission::Use)),
    ("institution_record_place", Need::Permission(Permission::Use)),
    ("institution_record_detach", Need::Permission(Permission::Use)),
    ("institution_record_open", Need::Permission(Permission::Use)),
    ("institution_clabe_reveal", Need::Permission(Permission::Use)),
    // the manual of the program (ADR-034): help for anyone who is in, with no data of the institution
    ("manual_entry", Need::Session),
    ("app_places", Need::Session),
    ("manual_search", Need::Session),
    // the suggestions per field (the reading by rules leaves them); accepting one is the usual `form_save`
    ("proposals_reject", Need::Permission(Permission::Use)),
    // the money (ADR-032)
    ("finance_get", Need::Permission(Permission::Use)),
    ("finance_save", Need::Permission(Permission::Use)),
    ("documents_list", Need::Permission(Permission::Use)),
    ("documents_search", Need::Permission(Permission::Use)),
    ("document_emergency_delete", Need::DeleteOrRequest),
    ("dev_load_fixture", Need::Permission(Permission::Settings)),
    // the entry to the runtime (ADR-003): a session is enough, the permission is checked per capability inside
    ("runtime_submit", Need::Session),
    ("runtime_task", Need::Session),
    ("care_overview", Need::Permission(Permission::Use)),
    ("care_person_get", Need::Permission(Permission::Use)),
    ("care_person_save", Need::Permission(Permission::Use)),
    ("care_person_delete", Need::DeleteOrRequest),
    ("care_person_reveal", Need::Permission(Permission::Use)),
    ("care_group_save", Need::Permission(Permission::Use)),
    ("care_field_save", Need::Permission(Permission::Use)),
    ("care_field_delete", Need::DeleteOrRequest),
    ("care_waitlist_save", Need::Permission(Permission::Use)),
    ("care_waitlist_admit", Need::Permission(Permission::Use)),
    ("care_waitlist_delete", Need::Permission(Permission::Delete)),
    ("onboarding_status", Need::Session),
    ("onboarding_save", Need::Permission(Permission::Use)),
    ("onboarding_finish", Need::Permission(Permission::Use)),
    ("onboarding_welcome_done", Need::Session),
    ("facilities_overview", Need::Permission(Permission::Use)),
    ("facilities_site_save", Need::Permission(Permission::Use)),
    ("facilities_space_save", Need::Permission(Permission::Use)),
    ("facilities_space_delete", Need::Permission(Permission::Use)),
    ("facilities_equipment_save", Need::Permission(Permission::Use)),
    ("facilities_equipment_delete", Need::Permission(Permission::Use)),
    ("hr_overview", Need::Permission(Permission::Use)),
    ("hr_person_get", Need::Permission(Permission::Use)),
    ("hr_person_save", Need::Permission(Permission::Use)),
    ("hr_person_delete", Need::DeleteOrRequest),
    ("hr_person_reveal", Need::Permission(Permission::Use)),
    ("hr_position_save", Need::Permission(Permission::Use)),
    ("hr_position_set_active", Need::Permission(Permission::Use)),
    ("hr_modality_create", Need::Permission(Permission::Use)),
    ("hr_field_save", Need::Permission(Permission::Use)),
    ("hr_field_delete", Need::DeleteOrRequest),
    ("ai_status", Need::Permission(Permission::Use)),
    ("ai_set_provider", Need::Permission(Permission::Settings)),
    ("ai_models", Need::Permission(Permission::Use)),
    ("ai_set_models", Need::Permission(Permission::Settings)),
    ("ai_set_key", Need::Permission(Permission::Settings)),
    ("ai_clear_key", Need::Permission(Permission::Settings)),
    ("ai_check", Need::Permission(Permission::Settings)),
    ("ai_usage_report", Need::Permission(Permission::Use)),
    ("ai_set_cap", Need::Permission(Permission::Settings)),
    // security
    ("security_scan", Need::Permission(Permission::Use)),
    ("backup_create", Need::Permission(Permission::Use)),
    ("backup_restore", Need::Permission(Permission::Settings)),
];

pub fn need_of(command: &str) -> Option<Need> {
    COMMANDS.iter().find(|(c, _)| *c == command).map(|(_, n)| *n)
}

/// The kinds of deletion that become a request when the person may not delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletionKind {
    Document,
    HrPerson,
    /// A person served (ADR-029).
    Beneficiary,
    Project,
    /// A field of their own of the form of the people served (stored as `roster_field`, its name before ADR-029).
    CareField,
    HrField,
    /// A person of the board of the institution (ADR-033 §3).
    BoardMember,
    /// A bank account of the institution.
    BankAccount,
}

impl DeletionKind {
    pub const ALL: [DeletionKind; 8] = [
        DeletionKind::Document,
        DeletionKind::HrPerson,
        DeletionKind::Beneficiary,
        DeletionKind::Project,
        DeletionKind::CareField,
        DeletionKind::HrField,
        DeletionKind::BoardMember,
        DeletionKind::BankAccount,
    ];
    pub fn as_db(self) -> &'static str {
        match self {
            DeletionKind::Document => "document",
            DeletionKind::HrPerson => "hr_person",
            DeletionKind::Beneficiary => "beneficiary",
            DeletionKind::Project => "project",
            DeletionKind::CareField => "roster_field",
            DeletionKind::HrField => "hr_field",
            DeletionKind::BoardMember => "board_member",
            DeletionKind::BankAccount => "bank_account",
        }
    }
    pub fn from_db(s: &str) -> Option<Self> {
        DeletionKind::ALL.into_iter().find(|k| k.as_db() == s)
    }
}

pub const MIN_PASSWORD: usize = 8;
pub const MAX_PASSWORD: usize = 128;
/// Minutes without use before the session locks (the work stays; the same person enters their password again).
pub const IDLE_MINUTES: u64 = 15;
/// Wrong tries in a row before an account has to wait.
pub const FREE_TRIES: i64 = 5;

/// Why a password is not accepted, if it is not.
pub fn password_problem(password: &str, username: &str) -> Option<&'static str> {
    let n = password.chars().count();
    if n < MIN_PASSWORD {
        return Some("password_short");
    }
    if n > MAX_PASSWORD {
        return Some("password_long");
    }
    if password.trim().to_lowercase() == username.trim().to_lowercase() {
        return Some("password_is_username");
    }
    None
}

/// A user name: 3 to 32 letters, digits, dots, dashes or underscores; kept in lower case.
pub fn normalize_username(s: &str) -> Option<String> {
    let u = s.trim().to_lowercase();
    let ok = (3..=32).contains(&u.chars().count()) && u.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    ok.then_some(u)
}

/// Seconds an account waits after `failed` wrong tries in a row: nothing for the first ones, then 30 seconds that
/// double with each new mistake, up to 15 minutes.
pub fn wait_after(failed: i64) -> i64 {
    if failed < FREE_TRIES {
        return 0;
    }
    let doublings = (failed - FREE_TRIES).min(5) as u32;
    (30 * 2_i64.pow(doublings)).min(15 * 60)
}

/// A suggested user name from a person's names: «Rosa María Hernández» -> «rosa.hernandez».
pub fn suggest_username(first_names: &str, last_name: Option<&str>) -> String {
    let plain = |s: &str| -> String {
        s.to_lowercase()
            .chars()
            .map(|c| match c {
                'á' | 'à' | 'ä' => 'a',
                'é' | 'è' | 'ë' => 'e',
                'í' | 'ì' | 'ï' => 'i',
                'ó' | 'ò' | 'ö' => 'o',
                'ú' | 'ù' | 'ü' => 'u',
                'ñ' => 'n',
                c => c,
            })
            .filter(|c| c.is_ascii_alphanumeric())
            .collect()
    };
    let first = first_names.split_whitespace().next().map(plain).unwrap_or_default();
    let last = last_name.and_then(|l| l.split_whitespace().last()).map(plain).unwrap_or_default();
    [first, last].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manager_uses_everything_but_cannot_delete_administer_or_change_settings() {
        assert!(Role::Manager.can(Permission::Use));
        for p in [Permission::Delete, Permission::Settings, Permission::Administer] {
            assert!(!Role::Manager.can(p), "{p:?}");
            assert!(Role::Admin.can(p), "{p:?}");
        }
    }

    /// The four permissions of the app are the kernel's general ones, and what each role may do is decided there: the
    /// list the screens receive says the same.
    #[test]
    fn the_permissions_and_roles_of_the_app_are_the_kernels() {
        let general: Vec<&str> = kernel::GENERAL.iter().map(|p| p.id).collect();
        assert_eq!(Permission::ALL.map(Permission::id).to_vec(), general);
        for role in [Role::Admin, Role::Manager] {
            assert!(kernel::role(role.as_db()).is_some(), "{role:?}");
            let can: Vec<Permission> = Permission::ALL.into_iter().filter(|p| role.can(*p)).collect();
            assert_eq!(role.permissions(), can.as_slice(), "{role:?}");
        }
    }

    /// The kernel names the classes of data of today by the same ids as the capability contract (IA-3 brings the
    /// definitive ones).
    #[test]
    fn the_classes_of_the_kernel_are_the_classes_of_the_contract() {
        use capability::DataClass::{self, *};
        // a new class has to be added here: without it this does not compile
        let _every = |c: DataClass| match c {
            Public | Internal | Confidential | Personal | SensitivePersonal => (),
        };
        let ids: Vec<String> = [Public, Internal, Confidential, Personal, SensitivePersonal]
            .iter()
            .map(|c| serde_json::to_value(c).unwrap().as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids, kernel::CLASSES);
    }

    #[test]
    fn every_command_is_listed_once() {
        let mut names: Vec<&str> = COMMANDS.iter().map(|(c, _)| *c).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n, "a command is listed twice");
    }

    #[test]
    fn every_registered_command_says_what_it_needs_and_checks_it() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let lib = std::fs::read_to_string(root.join("lib.rs")).unwrap();
        let start = lib.find("generate_handler![").unwrap();
        let end = start + lib[start..].find(']').unwrap();
        let registered: Vec<&str> =
            lib[start..end].split(|c: char| c == ',' || c == '[' || c.is_whitespace()).filter_map(|s| s.rsplit("::").next()).filter(|s| !s.is_empty() && *s != "generate_handler!").collect();
        let mut missing = Vec::new();
        for name in &registered {
            if need_of(name).is_none() {
                missing.push(*name);
            }
        }
        assert!(missing.is_empty(), "commands without a permission in domain::access::COMMANDS: {missing:?}");
        for (name, _) in COMMANDS {
            assert!(registered.contains(name), "«{name}» is listed but not registered");
        }

        // and every command body checks it with `guard(` (the open ones too: they say so)
        let mut unchecked = Vec::new();
        for entry in std::fs::read_dir(root.join("commands")).unwrap() {
            let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
            let mut rest = text.as_str();
            while let Some(at) = rest.find("#[tauri::command]") {
                rest = &rest[at + 17..];
                let fn_at = rest.find("fn ").unwrap();
                let name: String = rest[fn_at + 3..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                let body_end = rest[fn_at..].find("\n}").map_or(rest.len(), |e| fn_at + e);
                if !rest[fn_at..body_end].contains(&format!("guard(&session, \"{name}\")")) {
                    unchecked.push(name);
                }
            }
        }
        assert!(unchecked.is_empty(), "commands that do not call guard(&session, \"<their name>\"): {unchecked:?}");
    }

    #[test]
    fn passwords_and_user_names() {
        assert_eq!(password_problem("corta", "ana"), Some("password_short"));
        assert_eq!(password_problem("Ana.Lopez", "ana.lopez"), Some("password_is_username"));
        assert_eq!(password_problem("una frase larga", "ana"), None);
        assert_eq!(normalize_username(" Rosa.Hernandez "), Some("rosa.hernandez".into()));
        assert_eq!(normalize_username("ro"), None);
        assert_eq!(normalize_username("rosa hernández"), None);
        assert_eq!(suggest_username("Rosa María", Some("Hernández")), "rosa.hernandez");
        assert_eq!(suggest_username("Ñoño", None), "nono");
    }

    #[test]
    fn mistakes_make_the_account_wait_longer_up_to_a_limit() {
        let waits: Vec<i64> = [0, 4, 5, 6, 7, 10, 30].iter().map(|&n| wait_after(n)).collect();
        assert_eq!(waits, vec![0, 0, 30, 60, 120, 900, 900]);
    }
}
