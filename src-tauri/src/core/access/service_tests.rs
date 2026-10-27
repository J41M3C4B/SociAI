//! The accounts with the real database: the first account, entering and its mistakes, the administration rules,
//! the recovery code and the deletions that wait for the administrator.

use super::*;
use crate::core::access::domain::{Need, Permission};
use crate::core::profile::domain::{InstitutionInput, InstitutionKind, ProfileInput};
use crate::modules::hr::domain::person::PersonData;
use crate::storage::open_encrypted;

const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
const ADMIN_PASSWORD: &str = "una clave larga";

fn conn() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let c = open_encrypted(&dir.path().join("t.db"), KEY).unwrap();
    (dir, c)
}

fn with_admin(c: &mut Connection) -> (Session, SetupOutcome) {
    let session = Session::default();
    let out = setup_admin(c, &session, &Attempts::default(), "Jaime Caballero", "jaime", ADMIN_PASSWORD, None).unwrap();
    (session, out)
}

fn admin(session: &Session) -> CurrentUser {
    session.current().unwrap()
}

fn events(c: &Connection) -> Vec<(String, Option<String>, String)> {
    c.prepare("SELECT event, actor_id, details_json FROM audit_log ORDER BY rowid").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().collect::<Result<_, _>>().unwrap()
}

/// A person of the staff and an account for them as direction, already with their own password.
fn manager(c: &mut Connection, admin_session: &Session) -> (String, Session) {
    with_profile(c);
    let pos = crate::core::bridge::staff::overview(c).unwrap().hr.positions[0].position.id.clone();
    let data = PersonData { first_names: "Rosa María".into(), last_name_1: Some("Hernández".into()), position_id: Some(pos), modality: "indefinite".into(), status: "active".into(), ..Default::default() };
    let crate::core::bridge::staff::PersonOutcome::Saved { person, .. } = crate::core::bridge::staff::save_person(c, None, data).unwrap() else { panic!("saved") };
    let a = admin(admin_session);
    create_user(c, &a, &NewAccount { person_id: Some(&person.id), display_name: "", username: "rosa.hernandez", role: Role::Manager, temporary_password: "temporal123" }).unwrap();
    let session = Session::default();
    let LoginOutcome::Ok { session: view } = login(c, &session, "rosa.hernandez", "temporal123").unwrap() else { panic!("in") };
    assert!(view.must_change_password, "a temporary password is changed first");
    change_password(c, &session, "temporal123", "mi clave propia").unwrap();
    (person.id, session)
}

fn with_profile(c: &mut Connection) {
    if crate::core::profile::storage::load_current(c).unwrap().is_none() {
        let input = ProfileInput { institution: InstitutionInput { name: "Asilo Ficticio".into(), kind: InstitutionKind::ElderlyHome, ..Default::default() }, ..Default::default() };
        crate::core::profile::storage::save(c, &input, &crate::core::profile::storage::fixtures()).unwrap();
    }
}

#[test]
fn the_first_account_is_the_administrator_and_it_can_be_made_only_once() {
    let (_d, mut c) = conn();
    let session = Session::default();
    assert!(status(&c, &session).unwrap().needs_setup);
    let (session, out) = with_admin(&mut c);
    assert_eq!(out.session.role, Role::Admin);
    assert_eq!(out.recovery_code.len(), 19, "XXXX-XXXX-XXXX-XXXX: {}", out.recovery_code);
    let st = status(&c, &session).unwrap();
    assert!(!st.needs_setup && st.session.is_some());
    assert!(matches!(setup_admin(&mut c, &Session::default(), &Attempts::default(), "Otra", "otra", ADMIN_PASSWORD, None), Err(ServiceError::Access("already_set_up"))));
    let stored: String = c.query_row("SELECT password_hash FROM app_user", [], |r| r.get(0)).unwrap();
    assert!(stored.starts_with("$argon2id$") && !stored.contains(ADMIN_PASSWORD));
    let (event, actor, _) = events(&c).into_iter().next().unwrap();
    assert_eq!((event.as_str(), actor.is_some()), ("access.setup", true));
}

#[test]
fn a_computer_with_a_pin_asks_it_before_making_the_administrator_and_then_the_pin_retires() {
    let (_d, mut c) = conn();
    let attempts = Attempts::default();
    crate::core::security::pin_set(&c, &attempts, "4821", None).unwrap();
    assert!(status(&c, &Session::default()).unwrap().setup_needs_pin);
    assert!(matches!(setup_admin(&mut c, &Session::default(), &attempts, "Jaime", "jaime", ADMIN_PASSWORD, Some("1111")), Err(ServiceError::Access("wrong_pin"))));
    setup_admin(&mut c, &Session::default(), &attempts, "Jaime", "jaime", ADMIN_PASSWORD, Some("4821")).unwrap();
    assert!(!crate::core::security::pin_enabled(&c).unwrap());
}

#[test]
fn wrong_passwords_make_the_account_wait_and_say_nothing_of_which_part_was_wrong() {
    let (_d, mut c) = conn();
    let (s, _) = with_admin(&mut c);
    logout(&c, &s).unwrap();
    let session = Session::default();
    assert!(matches!(login(&c, &session, "nadie", "x").unwrap(), LoginOutcome::Wrong));
    for _ in 0..4 {
        assert!(matches!(login(&c, &session, "jaime", "mala clave").unwrap(), LoginOutcome::Wrong));
    }
    assert!(matches!(login(&c, &session, "jaime", "mala clave").unwrap(), LoginOutcome::Waiting { wait_secs: 30 }));
    assert!(matches!(login(&c, &session, "JAIME", ADMIN_PASSWORD).unwrap(), LoginOutcome::Waiting { .. }), "even the right one waits");
    assert!(session.current().is_none());
    c.execute("UPDATE app_user SET locked_until = 0", []).unwrap();
    assert!(matches!(login(&c, &session, "Jaime", ADMIN_PASSWORD).unwrap(), LoginOutcome::Ok { .. }));
    let all = events(&c);
    assert!(all.iter().any(|(e, _, _)| e == "auth.locked"));
    assert!(all.iter().all(|(_, _, d)| !d.contains("mala") && !d.contains(ADMIN_PASSWORD)), "no password in the log");
}

#[test]
fn a_temporary_password_is_changed_before_anything_else() {
    let (_d, mut c) = conn();
    let (s, _) = with_admin(&mut c);
    let a = admin(&s);
    create_user(&c, &a, &NewAccount { person_id: None, display_name: "Contador Ficticio", username: "contador", role: Role::Manager, temporary_password: "temporal123" }).unwrap();
    let session = Session::default();
    login(&c, &session, "contador", "temporal123").unwrap();
    let user = session.current().unwrap();
    assert_eq!(check(Some(user.clone()), false, Need::Permission(Permission::Use)), Err("must_change_password"));
    assert!(check(Some(user), false, Need::Session).is_ok(), "changing the password is allowed");
    assert!(matches!(change_password(&c, &session, "otra", "nueva clave 1"), Err(ServiceError::Access("wrong_password"))));
    assert!(matches!(change_password(&c, &session, "temporal123", "temporal123"), Err(ServiceError::Access("password_same"))));
    let view = change_password(&c, &session, "temporal123", "nueva clave 1").unwrap();
    assert!(!view.must_change_password);
    assert!(check(session.current(), false, Need::Permission(Permission::Use)).is_ok());
}

#[test]
fn the_check_of_every_command() {
    let manager = CurrentUser { id: "u".into(), username: "rosa".into(), display_name: "Rosa".into(), role: Role::Manager, must_change_password: false };
    let admin = CurrentUser { role: Role::Admin, ..manager.clone() };
    assert_eq!(check(None, false, Need::Permission(Permission::Use)), Err("not_signed_in"));
    assert!(check(None, false, Need::Open).is_ok());
    assert_eq!(check(Some(manager.clone()), true, Need::Permission(Permission::Use)), Err("session_locked"));
    assert!(check(Some(manager.clone()), false, Need::Permission(Permission::Use)).is_ok());
    assert!(check(Some(manager.clone()), false, Need::DeleteOrRequest).is_ok(), "it passes and becomes a request");
    for p in [Permission::Settings, Permission::Administer, Permission::Delete] {
        assert_eq!(check(Some(manager.clone()), false, Need::Permission(p)), Err("access_denied"), "{p:?}");
        assert!(check(Some(admin.clone()), false, Need::Permission(p)).is_ok(), "{p:?}");
    }
}

impl PartialEq for CurrentUser {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[test]
fn administration_rules_keep_one_administrator_and_one_account_per_person() {
    let (_d, mut c) = conn();
    let (s, _) = with_admin(&mut c);
    let a = admin(&s);
    let (person, _) = manager(&mut c, &s);
    assert!(matches!(
        create_user(&c, &a, &NewAccount { person_id: None, display_name: "Otra", username: "otra.admin", role: Role::Admin, temporary_password: "temporal123" }),
        Err(ServiceError::Access("role_not_allowed"))
    ));
    assert!(matches!(
        create_user(&c, &a, &NewAccount { person_id: Some(&person), display_name: "", username: "rosa2", role: Role::Manager, temporary_password: "temporal123" }),
        Err(ServiceError::Access("person_has_account"))
    ));
    assert!(matches!(
        create_user(&c, &a, &NewAccount { person_id: None, display_name: "X", username: "Rosa.Hernandez", role: Role::Manager, temporary_password: "temporal123" }),
        Err(ServiceError::Access("username_taken"))
    ));
    assert!(matches!(update_user(&c, &a, &a.id, Role::Manager, true), Err(ServiceError::Access("last_admin"))));
    assert!(matches!(update_user(&c, &a, &a.id, Role::Admin, false), Err(ServiceError::Access("last_admin"))));

    let overview = admin_overview(&c).unwrap();
    let rosa = overview.users.iter().find(|u| u.username == "rosa.hernandez").unwrap();
    assert_eq!(rosa.display_name, "Rosa María Hernández", "the name comes from the staff record");
    assert!(overview.people.iter().any(|p| p.person_id == person && p.user_id.as_deref() == Some(rosa.id.as_str())));
    let off = update_user(&c, &a, &rosa.id, Role::Manager, false).unwrap();
    assert!(!off.users.iter().find(|u| u.id == rosa.id).unwrap().active);
    assert!(matches!(login(&c, &Session::default(), "rosa.hernandez", "mi clave propia").unwrap(), LoginOutcome::Disabled));
}

#[test]
fn the_team_lists_only_the_accounts_that_are_active() {
    let (_d, mut c) = conn();
    let (s, _) = with_admin(&mut c);
    let (_, _) = manager(&mut c, &s);
    let a = admin(&s);
    let all = team(&c).unwrap();
    assert_eq!(all.len(), 2, "the administrator and the direction");
    assert!(all.iter().any(|m| m.display_name == "Jaime Caballero" && m.role == Role::Admin));
    let rosa = admin_overview(&c).unwrap().users.into_iter().find(|u| u.role == Role::Manager).unwrap();
    update_user(&c, &a, &rosa.id, Role::Manager, false).unwrap();
    assert_eq!(team(&c).unwrap().len(), 1, "an account that was turned off is no longer in");
}

#[test]
fn a_person_who_leaves_the_institution_loses_access() {
    let (_d, mut c) = conn();
    let (s, _) = with_admin(&mut c);
    let (person, _) = manager(&mut c, &s);
    let mut data = crate::modules::hr::service::get_person(&c, &person).unwrap().unwrap().data;
    data.status = "left".into();
    data.left_date = Some("2026-10-01".into());
    crate::core::bridge::staff::save_person(&mut c, Some(&person), data).unwrap();
    assert!(matches!(login(&c, &Session::default(), "rosa.hernandez", "mi clave propia").unwrap(), LoginOutcome::Disabled));
}

#[test]
fn the_recovery_code_opens_once_and_is_renewed() {
    let (_d, mut c) = conn();
    let (s, out) = with_admin(&mut c);
    logout(&c, &s).unwrap();
    let session = Session::default();
    assert!(matches!(recover(&c, &session, "jaime", "AAAA-BBBB-CCCC-DDDD", "nueva clave larga"), Err(ServiceError::Access("recovery_wrong"))));
    let lower = out.recovery_code.to_lowercase().replace('-', " ");
    let again = recover(&c, &session, "jaime", &lower, "nueva clave larga").unwrap();
    assert_ne!(again.recovery_code, out.recovery_code);
    assert!(matches!(recover(&c, &Session::default(), "jaime", &out.recovery_code, "otra clave larga"), Err(ServiceError::Access("recovery_wrong"))), "the old code no longer works");
    assert!(matches!(login(&c, &Session::default(), "jaime", "nueva clave larga").unwrap(), LoginOutcome::Ok { .. }));
}

#[test]
fn the_locked_screen_opens_only_with_the_password_of_who_was_inside() {
    let (_d, mut c) = conn();
    let (s, _) = with_admin(&mut c);
    lock(&s);
    assert!(status(&c, &s).unwrap().session.unwrap().locked);
    assert_eq!(check(s.current(), true, Need::Permission(Permission::Use)), Err("session_locked"));
    assert!(matches!(unlock(&c, &s, "otra").unwrap(), LoginOutcome::Wrong));
    assert!(matches!(unlock(&c, &s, ADMIN_PASSWORD).unwrap(), LoginOutcome::Ok { .. }));
    assert!(!s.state.lock().unwrap().locked);
}

#[test]
fn a_deletion_asked_by_direction_hides_the_person_until_the_administrator_decides() {
    let (_d, mut c) = conn();
    let (s, _) = with_admin(&mut c);
    let a = admin(&s);
    let (_, rosa_session) = manager(&mut c, &s);
    let rosa = rosa_session.current().unwrap();
    let pos = crate::core::bridge::staff::overview(&c).unwrap().hr.positions[0].position.id.clone();
    let data = PersonData { first_names: "Ana".into(), position_id: Some(pos), modality: "indefinite".into(), status: "active".into(), pay_amount_mxn: Some(9_000), ..Default::default() };
    let crate::core::bridge::staff::PersonOutcome::Saved { person, .. } = crate::core::bridge::staff::save_person(&mut c, None, data).unwrap() else { panic!() };
    let payroll = |c: &Connection| crate::core::profile::storage::load_current(c).unwrap().unwrap().input.staff.iter().map(|l| l.monthly_salary_mxn.unwrap_or(0) * l.count).sum::<i64>();
    assert_eq!(payroll(&c), 9_000);

    request_deletion(&mut c, &rosa, DeletionKind::HrPerson, &person.id).unwrap();
    assert!(!crate::core::bridge::staff::overview(&c).unwrap().hr.people.iter().any(|p| p.id == person.id), "gone from the list for everyone");
    assert_eq!(payroll(&c), 0, "and from the sums");
    assert!(matches!(request_deletion(&mut c, &rosa, DeletionKind::HrPerson, &person.id), Err(ServiceError::Access("already_requested"))));
    let pending = admin_overview(&c).unwrap().pending;
    assert_eq!((pending.len(), pending[0].target_label.as_str(), pending[0].requested_by_name.as_str()), (1, "Ana", "Rosa María Hernández"));

    // rejected: it comes back
    resolve_request(&mut c, &a, &pending[0].id, false).unwrap();
    assert!(crate::core::bridge::staff::overview(&c).unwrap().hr.people.iter().any(|p| p.id == person.id));
    assert_eq!(payroll(&c), 9_000);

    // asked again and approved: deleted for good
    request_deletion(&mut c, &rosa, DeletionKind::HrPerson, &person.id).unwrap();
    let id = admin_overview(&c).unwrap().pending[0].id.clone();
    let after = resolve_request(&mut c, &a, &id, true).unwrap();
    assert!(after.pending.is_empty() && after.resolved.iter().any(|r| r.status == "approved"));
    assert!(crate::modules::hr::service::get_person(&c, &person.id).unwrap().is_none());
    let kinds: Vec<String> = events(&c).into_iter().map(|e| e.0).collect();
    for k in ["request.created", "request.rejected", "request.approved", "hr.person_deleted"] {
        assert!(kinds.iter().any(|e| e == k), "{k}");
    }
}

/// ADR-033 §3: a person of the board or an account written by mistake follows the same rules of deletion.
#[test]
fn a_board_member_or_an_account_deleted_by_direction_waits_for_the_administrator() {
    use crate::core::protected::{self, ListKind, ListOutcome};
    let (_d, mut c) = conn();
    let (s, _) = with_admin(&mut c);
    let a = admin(&s);
    let (_, rosa_session) = manager(&mut c, &s);
    let rosa = rosa_session.current().unwrap();
    let v = serde_json::from_value(serde_json::json!({ "institution.board.name": "Vocal Ficticia", "institution.board.role": "member" })).unwrap();
    let ListOutcome::Saved { lists } = protected::save(&mut c, ListKind::BoardMember, None, v).unwrap() else { panic!("saved") };
    let member = lists.board[0].id.clone();
    let v = serde_json::from_value(serde_json::json!({ "institution.bank.clabe": "002010077777777771", "institution.bank.purpose": ["donations"] })).unwrap();
    let ListOutcome::Saved { lists } = protected::save(&mut c, ListKind::BankAccount, None, v).unwrap() else { panic!("saved") };
    let account = lists.accounts[0].id.clone();

    request_deletion(&mut c, &rosa, DeletionKind::BoardMember, &member).unwrap();
    request_deletion(&mut c, &rosa, DeletionKind::BankAccount, &account).unwrap();
    let l = protected::lists(&c).unwrap();
    assert!(l.board.is_empty() && l.accounts.is_empty(), "hidden for everyone meanwhile");
    let pending = admin_overview(&c).unwrap().pending;
    let labels: Vec<&str> = pending.iter().map(|r| r.target_label.as_str()).collect();
    assert!(labels.contains(&"Vocal Ficticia") && labels.contains(&"Cuenta ••••••••••••••7771"), "{labels:?}");
    assert!(!labels.iter().any(|l| l.contains("002010")), "the administrator sees the account covered");

    // the member comes back; the account goes for good
    let of = |k: DeletionKind| pending.iter().find(|r| r.kind == k).unwrap().id.clone();
    resolve_request(&mut c, &a, &of(DeletionKind::BoardMember), false).unwrap();
    resolve_request(&mut c, &a, &of(DeletionKind::BankAccount), true).unwrap();
    let l = protected::lists(&c).unwrap();
    assert_eq!((l.board.len(), l.accounts.len()), (1, 0));
    let left: i64 = c.query_row("SELECT count(*) FROM core_bank_account", [], |r| r.get(0)).unwrap();
    assert_eq!(left, 0);
}

#[test]
fn a_hidden_field_keeps_its_values_and_a_document_hides_too() {
    use crate::modules::care::domain::person::BeneficiaryData;
    let (_d, mut c) = conn();
    let (s, _) = with_admin(&mut c);
    let (_, rosa_session) = manager(&mut c, &s);
    let rosa = rosa_session.current().unwrap();
    let fields = crate::core::bridge::care::save_field(&c, None, "Alergias", "text", &[]).unwrap();
    let key = fields.iter().find(|f| f.title == "Alergias").unwrap().key.clone();
    let mut data = BeneficiaryData { first_names: "Luz".into(), approx_age: Some(80), status: "active".into(), ..Default::default() };
    data.extra.insert(key.clone(), "nueces".into());
    let crate::core::bridge::care::PersonOutcome::Saved { person, .. } = crate::core::bridge::care::save_person(&mut c, None, data.clone()).unwrap() else { panic!("saved") };

    request_deletion(&mut c, &rosa, DeletionKind::CareField, &key).unwrap();
    // the record is edited while the field is hidden: its value survives
    let mut again = person.data.clone();
    again.extra.remove(&key);
    crate::core::bridge::care::save_person(&mut c, Some(&person.id), again).unwrap();
    let stored = crate::modules::care::storage::person(&c, &person.id).unwrap().unwrap();
    assert_eq!(stored.data.extra.get(&key).map(String::as_str), Some("nueces"));

    let doc = crate::core::archive::storage::add_text_document(&mut c, crate::core::archive::storage::Drawer::Record, "Reglamento", "Texto del reglamento interno.", 0).unwrap();
    request_deletion(&mut c, &rosa, DeletionKind::Document, &doc.id).unwrap();
    assert!(crate::core::archive::storage::list_drawer(&c, crate::core::archive::storage::Drawer::Record).unwrap().is_empty());
}

/// A project is no longer a module of the program, but its data is still the person's: when the administrator approves
/// its deletion, it goes with everything of it (needs, sections, and the call with its files).
#[test]
fn approving_the_deletion_of_a_project_removes_it_with_its_call_and_its_data() {
    let (_d, mut c) = conn();
    let (s, _) = with_admin(&mut c);
    let a = admin(&s);
    let (_, rosa_session) = manager(&mut c, &s);
    let rosa = rosa_session.current().unwrap();
    c.execute_batch(
        "INSERT INTO call_reading (id,name,status,created_at) VALUES ('r','Una','ready','t');
         INSERT INTO document (id,kind,display_name,mime,data_level,clean_hash,extracted_text,redactions_count,created_at) VALUES ('d','call','a.pdf','application/pdf','green','h','texto de la convocatoria',0,'t');
         INSERT INTO call_reading_file (reading_id,document_id,position,role) VALUES ('r','d',0,'main');
         INSERT INTO project (id,institution_id,profile_id,title,stage,kind,call_reading_id,created_at,updated_at)
           SELECT 'proj', institution_id, id, 'Baño', 'PROFILE', 'call', 'r', 't', 't' FROM institution_profile LIMIT 1;
         INSERT INTO need (id,project_id,title,origin) VALUES ('n','proj','Una necesidad','user');
         INSERT INTO project_section (id,project_id,section_key,content,needs_review,updated_at,origin) VALUES ('s','proj','what','Texto',0,'t','user');",
    )
    .unwrap();
    let count = |c: &Connection, table: &str| -> i64 { c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)).unwrap() };

    request_deletion(&mut c, &rosa, DeletionKind::Project, "proj").unwrap();
    let id = admin_overview(&c).unwrap().pending[0].id.clone();
    assert_eq!(count(&c, "project"), 1, "it waits for the administrator");
    resolve_request(&mut c, &a, &id, true).unwrap();
    for table in ["project", "need", "project_section", "call_reading", "call_reading_file", "document"] {
        assert_eq!(count(&c, table), 0, "{table}");
    }
    assert!(events(&c).into_iter().any(|e| e.0 == "emergency.delete"), "the deletion is in the log");
}

/// The kernel decides with the account as it is now (ERP-10): the administrator may administer in its own institution,
/// direction may not, nobody passes in another organization, and a deactivated account may do nothing and sees nothing.
#[test]
fn the_kernel_decides_with_the_account_as_it_is_now() {
    let (_d, mut c) = conn();
    let (session, _) = with_admin(&mut c);
    let (_, rosa_session) = manager(&mut c, &session);
    let modules = kernel::Modules::discover().unwrap();
    let org = crate::core::institution::id(&c).unwrap();
    let boss = account(&c, &admin(&session).id).unwrap().unwrap();
    let rosa = account(&c, &rosa_session.current().unwrap().id).unwrap().unwrap();
    assert_eq!(kernel::authorize(&modules, &boss, &org, "kernel.administrar"), Ok(()));
    assert_eq!(kernel::authorize(&modules, &boss, &org, "muestra.notas.escribir"), Ok(()));
    assert_eq!(kernel::authorize(&modules, &rosa, &org, "kernel.usar"), Ok(()));
    assert_eq!(kernel::authorize(&modules, &rosa, &org, "kernel.administrar"), Err(kernel::Denial::NotGranted));
    assert_eq!(kernel::authorize(&modules, &rosa, "inst_otra", "kernel.usar"), Err(kernel::Denial::OtherOrganization));
    assert_eq!(kernel::classes_seen(&rosa), kernel::CLASSES);
    update_user(&c, &admin(&session), &rosa.id, Role::Manager, false).unwrap();
    let rosa = account(&c, &rosa.id).unwrap().unwrap();
    assert_eq!(kernel::authorize(&modules, &rosa, &org, "kernel.usar"), Err(kernel::Denial::Inactive));
    assert!(kernel::classes_seen(&rosa).is_empty());
    assert!(account(&c, "usr_nadie").unwrap().is_none());
}
