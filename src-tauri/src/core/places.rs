//! The map of the program: every place a button may take the person to (a page, or a window of «Mi institución»), its
//! name and who may open it. It is built from what already exists, not from a list written by hand:
//!
//! - **A page is a screen of the manual** (`screen.<page>`): its name is the title of that entry. A new page is born
//!   when its entry is written in `docs/manual/`.
//! - **A window of «Mi institución» is the forms it holds** (`WINDOWS`): its name is the title of its first form in
//!   the manual. A new form of the catalog must say its window, or a test fails.
//! - **Every entry of the manual knows its place:** a field goes to the window of its form, a document of the
//!   Expediente (`record.*`) to the Expediente, and a doubt of use to the place its heading names
//!   (`{#howto.backup @screen.security}`). So the button of an answer follows the entry it was based on.
//!
//! Tests check the map against the screen: the pages of the bar (`Shell.tsx`), the ones it hides by permission, and
//! the windows «Mi institución» opens.

use crate::core::access::domain::{Permission, Role};
use crate::core::institution::forms::{forms, LIST_FORMS};
use crate::core::manual::MANUAL;
use serde::Serialize;
use std::sync::LazyLock;

/// The windows of «Mi institución» and the forms each one holds (`profile.<window>`, as the screen opens them).
pub const WINDOWS: &[(&str, &[&str])] = &[
    ("profile.institution", &["institution.identity"]),
    ("profile.contact", &["institution.contact"]),
    ("profile.legal", &["institution.legal"]),
    ("profile.capacity", &["institution.capacity"]),
    ("profile.board", &["institution.board_member", "institution.bank_account"]),
    ("profile.records", &["institution.record"]),
];

/// The pages only some may open (ADR-028), as the bar of the program hides them.
const NEEDS: &[(&str, Permission)] = &[("ai", Permission::Settings), ("admin", Permission::Administer)];

/// The form whose window holds the documents of the Expediente (`record.*` of the manual).
const RECORD_FORM: &str = "institution.record";

/// A place of the program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Place {
    /// `security`, or `profile.legal` for a window.
    pub id: String,
    /// Its name, the title of its entry in the manual.
    pub label: String,
    /// Its entry in the manual.
    #[serde(skip)]
    pub entry: String,
    /// What it says, in a line (the first paragraph of its entry).
    #[serde(skip)]
    pub about: String,
    #[serde(skip)]
    pub needs: Option<Permission>,
}

impl Place {
    /// The page it is on: `profile` for `profile.legal`.
    pub fn page(&self) -> &str {
        self.id.split('.').next().unwrap_or(&self.id)
    }
    pub fn is_window(&self) -> bool {
        self.id.contains('.')
    }
}

fn of_manual(id: &str, entry: &str) -> Option<Place> {
    let e = MANUAL.iter().find(|e| e.id == entry)?;
    let needs = NEEDS.iter().find(|(p, _)| *p == id.split('.').next().unwrap_or(id)).map(|(_, n)| *n);
    Some(Place { id: id.to_string(), label: e.title.clone(), entry: e.id.clone(), about: e.paragraphs.first().cloned().unwrap_or_default(), needs })
}

/// Every place: the pages in the order of the manual, then the windows.
pub static PLACES: LazyLock<Vec<Place>> = LazyLock::new(|| {
    // a frozen module is not a place: the bar, the Assistant and its buttons do not know it (ADR-043)
    let pages = MANUAL.iter().filter_map(|e| e.id.strip_prefix("screen.").filter(|page| crate::core::modules::shown(page)).and_then(|page| of_manual(page, &e.id)));
    let windows = WINDOWS.iter().filter_map(|(id, forms)| of_manual(id, forms.first()?));
    pages.chain(windows).collect()
});

pub fn place(id: &str) -> Option<&'static Place> {
    PLACES.iter().find(|p| p.id == id)
}

/// The place an entry of the manual itself names: a screen, a form or a field, or a document of the Expediente.
fn direct(id: &str) -> Option<&'static str> {
    if let Some(page) = id.strip_prefix("screen.") {
        return place(page).map(|p| p.id.as_str());
    }
    let form = if id.starts_with("record.") { RECORD_FORM } else { forms().iter().chain(LIST_FORMS).find(|f| f.id == id || f.field(id).is_some())?.id };
    WINDOWS.iter().find(|(_, forms)| forms.contains(&form)).map(|(w, _)| *w)
}

/// Where what an entry of the manual explains is done: its own place, or the one its heading names with `@`.
pub fn of_entry(id: &str) -> Option<&'static str> {
    direct(id).or_else(|| direct(MANUAL.iter().find(|e| e.id == id)?.at.as_deref()?))
}

/// Whether the person may open a place: the bar shows them only what their role allows.
pub fn may_open(p: &Place, role: Option<Role>) -> bool {
    p.needs.is_none_or(|n| role.is_some_and(|r| r.can(n)))
}

/// `a` is `b` or inside it (`profile.legal` is inside `profile`).
fn within(a: &str, b: &str) -> bool {
    a == b || a.strip_prefix(b).is_some_and(|rest| rest.starts_with('.'))
}

/// Where the button of an answer takes the person. `said` is the place the AI chose; `used`, the entries of the manual
/// the answer was based on; `here`, the page where the person is.
///
/// - A place they cannot open, or the page they are already on, is no button: it would lead nowhere.
/// - The place of the entries the answer used wins over a guess: if the AI chose that place, or one inside it, it
///   stays; if it chose the page that holds it, the button goes to the window; if it chose another place, the button
///   follows the entry.
/// - With no entry that has a place, the place the AI chose.
pub fn choose(said: Option<&str>, used: &[&str], here: &str, role: Option<Role>) -> Option<&'static str> {
    let fits = |id: &str| place(id).filter(|p| p.id != here && may_open(p, role)).map(|p| p.id.as_str());
    let mut linked: Vec<&'static str> = Vec::new();
    for p in used.iter().filter_map(|e| of_entry(e)).filter_map(fits) {
        if !linked.contains(&p) {
            linked.push(p);
        }
    }
    let Some(said) = said.and_then(fits) else { return linked.first().copied() };
    if linked.is_empty() || linked.iter().any(|l| within(said, l)) {
        return Some(said);
    }
    linked.iter().find(|l| within(l, said)).or(linked.first()).copied()
}

/// The places as the Assistant's trade lists them: the code, the name and, for a window, what it holds.
pub fn listed() -> String {
    PLACES
        .iter()
        .map(|p| if p.is_window() { format!("  - `{}`: {}\n", p.id, p.about) } else { format!("  - `{}`: {}.\n", p.id, p.label) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen_file(path: &str) -> String {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(path)).unwrap()
    }

    /// The strings between quotes of the line that starts with `start`.
    fn quoted(text: &str, start: &str) -> Vec<String> {
        let line = text.lines().find(|l| l.trim_start().starts_with(start)).unwrap_or_else(|| panic!("no line «{start}»"));
        line.split('"').skip(1).step_by(2).map(str::to_string).collect()
    }

    /// Every page of the bar is a place with its screen in the manual, and every screen of the manual is a page of the
    /// bar: a new page cannot be offered by the Assistant before it exists, nor exist without its entry. A frozen
    /// module (ADR-043) keeps its page and its screen, but it is no place.
    #[test]
    fn every_page_of_the_bar_has_its_screen_in_the_manual() {
        let mut bar = quoted(&screen_file("src/components/Shell.tsx"), "export type Page =");
        for page in crate::core::modules::HIDDEN {
            assert!(bar.iter().any(|b| b == page) && MANUAL.iter().any(|e| e.id == format!("screen.{page}")), "{page} must keep its page and its screen, to be turned back on");
        }
        bar.retain(|b| crate::core::modules::shown(b));
        let mut pages: Vec<String> = PLACES.iter().filter(|p| !p.is_window()).map(|p| p.id.clone()).collect();
        bar.sort();
        pages.sort();
        assert_eq!(bar, pages, "the pages of Shell.tsx and the screens (`screen.*`) of docs/manual");
        assert_eq!(place("security").map(|p| p.label.as_str()), Some("Seguridad"));
        assert_eq!(place("profile").map(|p| p.label.as_str()), Some("Mi institución"));
    }

    /// The pages that need a permission are the ones the bar hides from whoever lacks it.
    #[test]
    fn the_pages_only_some_may_open_are_the_ones_the_bar_hides() {
        let shell = screen_file("src/components/Shell.tsx");
        for line in shell.lines().filter(|l| l.contains("{ page: \"") && l.contains("needs:")) {
            let page = line.split("page: \"").nth(1).and_then(|r| r.split('"').next()).unwrap();
            let needs = line.split("needs: ").nth(1).unwrap();
            let expected = if needs.starts_with("null") { None } else { needs.split('"').nth(1).map(str::to_string) };
            let got = place(page).unwrap().needs.map(|n| serde_json::to_value(n).unwrap().as_str().unwrap().to_string());
            assert_eq!(got, expected, "{page}");
        }
        assert!(!may_open(place("ai").unwrap(), Some(Role::Manager)) && may_open(place("ai").unwrap(), Some(Role::Admin)));
        assert!(may_open(place("security").unwrap(), Some(Role::Manager)));
        assert!(!may_open(place("admin").unwrap(), None));
    }

    /// Every form of the catalog is in exactly one window, every window has its entry, and «Mi institución» opens it.
    #[test]
    fn every_form_is_in_one_window_that_the_screen_opens() {
        for f in forms().iter().chain(LIST_FORMS) {
            assert_eq!(WINDOWS.iter().filter(|(_, forms)| forms.contains(&f.id)).count(), 1, "the form {} must say its window in WINDOWS", f.id);
        }
        let mut opened = quoted(&screen_file("src/lib/types.ts"), "export type Place =");
        opened.push("records".into());
        for (id, _) in WINDOWS {
            assert!(place(id).is_some(), "{id} has no entry in the manual");
            assert!(opened.iter().any(|w| format!("profile.{w}") == *id), "«Mi institución» does not open {id}");
        }
        assert_eq!(place("profile.legal").map(|p| p.label.as_str()), Some("Datos legales y fiscales"));
        assert!(place("profile.institution").unwrap().about.contains("«Su institución»"));
    }

    /// Every entry of the manual that names its place with `@` leads to one.
    #[test]
    fn every_place_the_manual_names_exists() {
        for e in MANUAL.iter() {
            if let Some(at) = &e.at {
                let hidden = at.strip_prefix("screen.").is_some_and(|page| !crate::core::modules::shown(page));
                assert!(direct(at).is_some() || hidden, "{}: «@{at}» is not a place", e.id);
            }
        }
    }

    #[test]
    fn each_entry_of_the_manual_leads_to_where_it_is_done() {
        assert_eq!(of_entry("institution.legal_rfc"), Some("profile.legal"), "a field, to the window of its form");
        assert_eq!(of_entry("institution.bank.clabe"), Some("profile.board"));
        assert_eq!(of_entry("institution.identity"), Some("profile.institution"), "a form, to its window");
        assert_eq!(of_entry("record.power_of_attorney"), Some("profile.records"), "a document, to the Expediente");
        assert_eq!(of_entry("screen.profile"), Some("profile"));
        assert_eq!(of_entry("howto.backup"), Some("security"), "a doubt of use, to the place its heading names");
        assert_eq!(of_entry("howto.saving"), None);
        assert_eq!(of_entry("nope"), None);
        // every field of the catalog has its window
        for f in forms().iter().chain(LIST_FORMS).flat_map(|f| f.fields()) {
            assert!(of_entry(f.id).is_some_and(|p| p.starts_with("profile.")), "{}", f.id);
        }
    }

    /// The button of an answer never leads nowhere, and it follows the entry the answer was based on.
    #[test]
    fn the_button_goes_where_the_answer_says_and_never_where_the_person_is() {
        let m = Some(Role::Manager);
        // «¿Cómo hago un respaldo?» from «Mi institución»: the AI said `profile`, the entry says Seguridad
        assert_eq!(choose(Some("profile"), &["howto.backup"], "profile", m), Some("security"));
        // already in Seguridad: no button
        assert_eq!(choose(Some("security"), &["howto.backup"], "security", m), None);
        // the page the person is on, with nothing else: no button; a window of that page opens
        assert_eq!(choose(Some("profile"), &[], "profile", m), None);
        assert_eq!(choose(Some("profile.legal"), &[], "profile", m), Some("profile.legal"));
        // the AI chose the page that holds the field: the button opens its window
        assert_eq!(choose(Some("profile"), &["institution.legal_rfc"], "home", m), Some("profile.legal"));
        // the AI chose that window, or the entry has no place: it stays
        assert_eq!(choose(Some("profile.legal"), &["institution.legal_rfc", "howto.saving"], "home", m), Some("profile.legal"));
        assert_eq!(choose(Some("documents"), &["howto.saving"], "home", m), Some("documents"));
        // a frozen module is no place: no button to it (ADR-043)
        assert_eq!(choose(Some("finance"), &["howto.saving"], "home", m), None);
        // no place from the AI: the one of the entry
        assert_eq!(choose(None, &["institution.populations"], "home", m), Some("profile.institution"));
        // a place it does not know, or one the person may not open, is no button
        assert_eq!(choose(Some("somewhere"), &[], "home", m), None);
        assert_eq!(choose(Some("ai"), &["howto.ai"], "home", m), None);
        assert_eq!(choose(Some("ai"), &["howto.ai"], "home", Some(Role::Admin)), Some("ai"));
    }

    /// Each window of «Mi institución» is a window of the map.
    #[test]
    fn the_windows_of_the_institution_are_windows_of_the_map() {
        for form in forms() {
            let window = direct(form.id).unwrap_or_else(|| panic!("{} has no window in the map", form.id));
            assert!(window.starts_with("profile.") && place(window).is_some(), "{window}");
        }
    }

    #[test]
    fn the_list_of_the_trade_has_every_place() {
        let l = listed();
        for p in PLACES.iter() {
            assert!(l.contains(&format!("`{}`:", p.id)), "{}", p.id);
        }
        assert!(l.contains("`security`: Seguridad.") && l.contains("`profile.records`: En «Mi institución», el expediente"), "{l}");
    }
}
