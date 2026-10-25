//! The board and the bank accounts with the real database: what is asked, what is refused, what the screen gets,
//! the history, the audit log, the AI sheet, the scanner and what is missing.

use super::*;
use crate::core::profile::domain::{InstitutionInput, InstitutionKind, ProfileInput};
use crate::storage::open_encrypted;

const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
/// Fictitious, with their right check digit: Banamex and BBVA.
const CLABE: &str = "002010077777777771";
const OTHER_CLABE: &str = "012180001234567899";

fn conn() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let c = open_encrypted(&dir.path().join("t.db"), KEY).unwrap();
    (dir, c)
}

fn with_profile(c: &mut Connection) {
    let input = ProfileInput { institution: InstitutionInput { name: "Asilo Ficticio".into(), kind: InstitutionKind::ElderlyHome, ..Default::default() }, ..Default::default() };
    crate::core::profile::storage::save(c, &input, &crate::core::profile::storage::fixtures()).unwrap();
}

fn values(v: Value) -> Values {
    serde_json::from_value(v).unwrap()
}

fn saved(out: ListOutcome) -> Lists {
    match out {
        ListOutcome::Saved { lists } => lists,
        other => panic!("not saved: {other:?}"),
    }
}

fn refused(out: ListOutcome) -> Vec<(&'static str, String)> {
    match out {
        ListOutcome::Invalid { issues } => issues.into_iter().map(|i| (i.code, i.field)).collect(),
        other => panic!("not refused: {other:?}"),
    }
}

fn member(c: &mut Connection, name: &str, role: &str) -> String {
    let lists = saved(save(c, ListKind::BoardMember, None, values(json!({ "institution.board.name": name, "institution.board.role": role }))).unwrap());
    lists.board.iter().find(|m| m.name == name).unwrap().id.clone()
}

fn account(c: &mut Connection, clabe: &str) -> String {
    let v = values(json!({ "institution.bank.clabe": clabe, "institution.bank.purpose": ["donations"], "institution.bank.in_documents": true }));
    saved(save(c, ListKind::BankAccount, None, v).unwrap()).accounts.last().unwrap().id.clone()
}

fn history(c: &Connection) -> Vec<(String, Option<String>, i64)> {
    c.prepare("SELECT field, value_json, protected FROM core_change ORDER BY id").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().collect::<Result<_, _>>().unwrap()
}

#[test]
fn a_board_member_needs_a_name_and_a_role_and_other_needs_its_name() {
    let (_d, mut c) = conn();
    assert_eq!(
        refused(save(&mut c, ListKind::BoardMember, None, values(json!({ "institution.board.since": "2023-02-30" }))).unwrap()),
        vec![("required", "institution.board.name".to_string()), ("required", "institution.board.role".to_string()), ("date_invalid", "institution.board.since".to_string())]
    );
    let other = values(json!({ "institution.board.name": "Laura Ficticia", "institution.board.role": "other" }));
    assert_eq!(refused(save(&mut c, ListKind::BoardMember, None, other).unwrap()), vec![("required", "institution.board.role_other".to_string())]);
    let wrong = values(json!({ "institution.board.name": "Laura Ficticia", "institution.board.role": "king" }));
    assert_eq!(refused(save(&mut c, ListKind::BoardMember, None, wrong).unwrap()), vec![("code_unknown", "institution.board.role".to_string())]);
    assert!(lists(&c).unwrap().board.is_empty(), "nothing refused is saved");

    // the name of another role is kept only with «other»
    let v = values(json!({ "institution.board.name": "Laura Ficticia", "institution.board.role": "member", "institution.board.role_other": "Comisaria", "institution.board.since": "2020-05-01" }));
    let m = &saved(save(&mut c, ListKind::BoardMember, None, v).unwrap()).board[0];
    assert_eq!((m.role.as_str(), m.role_other.as_deref(), m.since.as_deref(), m.active), ("member", None, Some("2020-05-01"), true));
    assert_eq!((m.origin.as_str(), m.confirmed_at.is_some()), ("user", true), "every datum carries its origin");
}

#[test]
fn the_board_goes_by_role_and_who_left_goes_last() {
    let (_d, mut c) = conn();
    let vocal = member(&mut c, "Vocal Ficticia", "member");
    member(&mut c, "Tesorero Ficticio", "treasurer");
    let president = member(&mut c, "Presidenta Ficticia", "president");
    set_active(&mut c, ListKind::BoardMember, &president, false).unwrap();
    let names: Vec<String> = lists(&c).unwrap().board.into_iter().map(|m| m.name).collect();
    assert_eq!(names, vec!["Tesorero Ficticio", "Vocal Ficticia", "Presidenta Ficticia"]);

    // the window of a saved member holds what it has; an edit keeps the rest
    let f = form(&c, ListKind::BoardMember, Some(&vocal)).unwrap();
    assert_eq!((f.values["institution.board.name"].clone(), f.masked.clone()), (json!("Vocal Ficticia"), None));
    let edit = values(json!({ "institution.board.name": "Vocal Ficticia", "institution.board.role": "secretary" }));
    saved(save(&mut c, ListKind::BoardMember, Some(&vocal), edit).unwrap());
    assert_eq!(lists(&c).unwrap().board[0].role, "secretary");
    assert!(matches!(save(&mut c, ListKind::BoardMember, Some("brd_nope"), values(json!({ "institution.board.name": "X", "institution.board.role": "member" }))), Err(ServiceError::NotFound)));
}

#[test]
fn an_account_needs_a_right_clabe_once_and_is_shown_covered() {
    let (_d, mut c) = conn();
    let wrong = values(json!({ "institution.bank.clabe": "002010077777777772", "institution.bank.purpose": ["donations"] }));
    assert_eq!(refused(save(&mut c, ListKind::BankAccount, None, wrong).unwrap()), vec![("clabe_invalid", "institution.bank.clabe".to_string())]);
    let no_use = values(json!({ "institution.bank.clabe": CLABE }));
    assert_eq!(refused(save(&mut c, ListKind::BankAccount, None, no_use).unwrap()), vec![("required", "institution.bank.purpose".to_string())]);

    // written in groups, kept as eighteen digits; the bank is read from it
    let v = values(json!({ "institution.bank.clabe": "002 010 07777777777 1", "institution.bank.purpose": ["donations", "operations"] }));
    let a = saved(save(&mut c, ListKind::BankAccount, None, v).unwrap()).accounts.remove(0);
    assert_eq!((a.masked.as_str(), a.bank.as_deref(), a.in_documents), ("••••••••••••••7771", Some("Banamex"), false));
    let stored: String = c.query_row("SELECT clabe FROM core_bank_account", [], |r| r.get(0)).unwrap();
    assert_eq!(stored, CLABE);

    // the same account twice is refused
    let again = values(json!({ "institution.bank.clabe": CLABE, "institution.bank.purpose": ["operations"] }));
    assert_eq!(refused(save(&mut c, ListKind::BankAccount, None, again).unwrap()), vec![("clabe_repeated", "institution.bank.clabe".to_string())]);

    // a saved account does not ask its CLABE again, and an edit cannot change it
    let f = form(&c, ListKind::BankAccount, Some(&a.id)).unwrap();
    assert!(f.spec.field("institution.bank.clabe").is_none() && !f.values.contains_key("institution.bank.clabe"));
    assert_eq!(f.masked.as_deref(), Some("••••••••••••••7771"));
    let edit = values(json!({ "institution.bank.clabe": OTHER_CLABE, "institution.bank.bank": "Banco Ficticio", "institution.bank.purpose": ["operations"], "institution.bank.in_documents": true }));
    let a = saved(save(&mut c, ListKind::BankAccount, Some(&a.id), edit).unwrap()).accounts.remove(0);
    assert_eq!((a.masked.as_str(), a.bank.as_deref(), a.purpose.clone(), a.in_documents), ("••••••••••••••7771", Some("Banco Ficticio"), vec!["operations".to_string()], true));
}

/// ADR-033 §3: showing the CLABE in full is written in the audit log, with the field and never the value.
#[test]
fn showing_a_clabe_leaves_its_mark_without_the_number() {
    let (_d, mut c) = conn();
    let id = account(&mut c, CLABE);
    assert_eq!(reveal_clabe(&c, &id).unwrap(), CLABE);
    let (event, details): (String, String) = c.query_row("SELECT event, details_json FROM audit_log ORDER BY rowid DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(event, "institution.sensitive_viewed");
    assert!(details.contains("clabe") && !details.contains(CLABE), "{details}");
    assert!(matches!(reveal_clabe(&c, "bnk_nope"), Err(ServiceError::NotFound)));
}

/// ADR-033 §4: each change of a list leaves a line that says only that it changed; never a name nor a CLABE.
#[test]
fn every_change_leaves_a_line_with_no_value() {
    let (_d, mut c) = conn();
    let id = member(&mut c, "Presidenta Ficticia", "president");
    set_active(&mut c, ListKind::BoardMember, &id, false).unwrap();
    let acc = account(&mut c, CLABE);
    delete(&mut c, ListKind::BankAccount, &acc).unwrap();
    let lines = history(&c);
    assert_eq!(
        lines,
        vec![
            ("institution.board_member".into(), None, 1),
            ("institution.board_member".into(), None, 1),
            ("institution.bank_account".into(), None, 1),
            ("institution.bank_account".into(), None, 1),
        ]
    );
    // a refused save leaves nothing
    save(&mut c, ListKind::BoardMember, None, Values::new()).unwrap();
    assert_eq!(history(&c).len(), 4);
}

/// The row and its line go together: without the line, the row is not saved.
#[test]
fn without_its_line_in_the_history_nothing_is_saved() {
    let (_d, mut c) = conn();
    c.execute_batch("CREATE TEMP TRIGGER fail_history BEFORE INSERT ON core_change BEGIN SELECT RAISE(ABORT, 'no'); END;").unwrap();
    assert!(save(&mut c, ListKind::BoardMember, None, values(json!({ "institution.board.name": "X", "institution.board.role": "member" }))).is_err());
    assert!(lists(&c).unwrap().board.is_empty());
}

/// Of the board the AI learns how many and which roles, of those in it today; of the accounts, nothing.
#[test]
fn the_ai_learns_the_roles_and_never_a_name_nor_an_account() {
    let (_d, mut c) = conn();
    with_profile(&mut c);
    let sheet = crate::core::ai_sheet::profile_context(&c).unwrap();
    assert!(sheet.contains("su patronato u órgano de gobierno"), "said as missing:\n{sheet}");

    member(&mut c, "Presidenta Ficticia", "president");
    member(&mut c, "Vocal Uno Ficticio", "member");
    member(&mut c, "Vocal Dos Ficticia", "member");
    let gone = member(&mut c, "Tesorera Ida", "treasurer");
    set_active(&mut c, ListKind::BoardMember, &gone, false).unwrap();
    account(&mut c, CLABE);
    assert_eq!(ai_board(&c).unwrap(), BoardSheet { total: 3, roles: vec![("president", 1), ("member", 2)] });

    let sheet = crate::core::ai_sheet::profile_context(&c).unwrap();
    assert!(sheet.contains("Patronato u órgano de gobierno: 3 personas — presidencia, vocal (2 personas). Sus nombres no se comparten."), "{sheet}");
    let part = crate::core::ai_sheet::section_context(&c, "institution").unwrap().unwrap();
    assert!(part.contains("Patronato u órgano de gobierno: 3 personas"), "{part}");
    for secret in ["Presidenta", "Vocal Uno", "Tesorera", CLABE, "7771", "Banamex"] {
        assert!(!sheet.contains(secret) && !part.contains(secret), "«{secret}» reached the AI:\n{sheet}");
    }
}

/// The scanner takes the accounts and the names of the board as the institution's own; any other CLABE stays blocked.
#[test]
fn the_scanner_knows_the_accounts_and_the_board_as_its_own() {
    use crate::scanner::{RegexScanner, SensitiveScanner};
    let (_d, mut c) = conn();
    with_profile(&mut c);
    member(&mut c, "Rosa Hernández López", "president");
    account(&mut c, CLABE);
    let cfg = crate::core::profile::storage::scanner_config(&c).unwrap();
    assert_eq!(cfg.institutional_clabes, vec![CLABE.to_string()]);
    assert!(cfg.safe_phrases.contains(&"Rosa Hernández López".to_string()));
    let s = RegexScanner::new(cfg);
    assert!(s.scan(&format!("Donativos a la cuenta {CLABE}; preside Rosa Hernández López.")).is_clean());
    assert!(s.scan(&format!("La cuenta de una empleada: {OTHER_CLABE}.")).has_blocking());
}

/// Who leaves is marked, not deleted; with nobody in the board, «lo que falta» asks for it.
#[test]
fn the_board_is_missing_until_someone_is_in_it() {
    let (_d, mut c) = conn();
    with_profile(&mut c);
    let gaps = |c: &Connection| crate::core::overview::institution_overview(c).unwrap().completion.gaps;
    let board = |c: &Connection| gaps(c).into_iter().find(|g| g.code == "board");
    assert_eq!(board(&c).map(|g| g.place), Some(crate::core::overview::Place::Board));
    let id = member(&mut c, "Presidenta Ficticia", "president");
    assert!(board(&c).is_none() && has_board(&c).unwrap());
    set_active(&mut c, ListKind::BoardMember, &id, false).unwrap();
    assert!(board(&c).is_some(), "someone who left does not count");
    assert_eq!(lists(&c).unwrap().board.len(), 1, "and is still there, marked");
    // the accounts are optional
    assert!(gaps(&c).iter().all(|g| !g.code.contains("bank")));
}

/// While a deletion waits for the administrator the row is hidden for everyone; it comes back if rejected.
#[test]
fn a_hidden_row_is_out_of_the_lists_and_comes_back() {
    let (_d, mut c) = conn();
    let id = member(&mut c, "Vocal Ficticia", "member");
    let acc = account(&mut c, CLABE);
    assert_eq!(set_hidden(&c, ListKind::BoardMember, &id, true).unwrap().as_deref(), Some("Vocal Ficticia"));
    assert_eq!(set_hidden(&c, ListKind::BankAccount, &acc, true).unwrap().as_deref(), Some("Cuenta ••••••••••••••7771"));
    let l = lists(&c).unwrap();
    assert!(l.board.is_empty() && l.accounts.is_empty() && !has_board(&c).unwrap());
    assert!(matches!(form(&c, ListKind::BoardMember, Some(&id)), Err(ServiceError::NotFound)));
    set_hidden(&c, ListKind::BoardMember, &id, false).unwrap();
    assert_eq!(lists(&c).unwrap().board.len(), 1);
    assert_eq!(set_hidden(&c, ListKind::BoardMember, "brd_nope", true).unwrap(), None);
}
