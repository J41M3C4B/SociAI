//! The manual of the program (ADR-034 §3): what each screen and each field is, what to put and what it is for. It is
//! written in `docs/manual/` and comes inside the program, so the «?» next to a field answers at once, with no AI,
//! no internet and no cost. Its search uses FTS5 over the encrypted base; the index is rebuilt only when the text of
//! the manual changes. «What uses it» is not written in the manual: it comes from `used_by` of the catalog of fields.

use crate::common::forms::FieldSpec;
use crate::core::error::ServiceError;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::sync::LazyLock;

/// The files of the manual, in the order their entries are listed.
const FILES: &[(&str, &str)] = &[
    ("mi-institucion.md", include_str!("../../../../docs/manual/mi-institucion.md")),
    ("programa.md", include_str!("../../../../docs/manual/programa.md")),
];

/// The whole text of the manual, file by file.
pub(crate) fn texts() -> impl Iterator<Item = &'static str> {
    FILES.iter().map(|(_, text)| *text)
}

/// One entry: a screen (`screen.home`), a form (`institution.identity`), a field (`institution.name`) or a doubt
/// of use (`howto.saving`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub id: String,
    pub title: String,
    /// One paragraph per line of the manual.
    pub paragraphs: Vec<String>,
    /// The entry of the place where this is done, when the heading names it (`{#howto.backup @screen.security}`):
    /// the map of the program (`core::places`) takes the place from there.
    #[serde(skip)]
    pub at: Option<String>,
}

/// An entry as the screen shows it: with what uses the field, when it is one (codes of `used_by`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntryView {
    #[serde(flatten)]
    pub entry: Entry,
    pub used_by: Vec<&'static str>,
}

/// A result of the search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hit {
    pub id: String,
    pub title: String,
    /// The beginning of the entry, to recognize it.
    pub snippet: String,
}

/// `Título {#clave}` → (title, id, None); `Título {#clave @otra.clave}` → (title, id, the entry of its place).
fn heading(line: &str) -> Option<(String, String, Option<String>)> {
    let text = line.trim_start_matches('#').trim();
    if !line.starts_with('#') || !text.ends_with('}') {
        return None;
    }
    let open = text.rfind("{#")?;
    let mut key = text[open + 2..text.len() - 1].split_whitespace();
    let id = key.next()?;
    let at = key.next().and_then(|a| a.strip_prefix('@')).map(str::to_string);
    Some((text[..open].trim().to_string(), id.to_string(), at))
}

/// The entries of one file of the manual. A heading without a key closes the entry before it and starts nothing.
pub fn parse(text: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut open = false;
    for line in text.lines() {
        if line.starts_with('#') {
            open = match heading(line) {
                Some((title, id, at)) => {
                    out.push(Entry { id, title, paragraphs: Vec::new(), at });
                    true
                }
                None => false,
            };
            continue;
        }
        let line = line.trim();
        if open && !line.is_empty() {
            if let Some(e) = out.last_mut() {
                e.paragraphs.push(line.to_string());
            }
        }
    }
    out
}

/// Every entry of the manual, read once.
pub static MANUAL: LazyLock<Vec<Entry>> = LazyLock::new(|| FILES.iter().flat_map(|(_, text)| parse(text)).collect());

/// A fingerprint of the text of the manual (FNV-1a), to know when the index is out of date.
fn version() -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for (name, text) in FILES {
        for b in name.bytes().chain(text.bytes()) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("{h:016x}")
}

fn field_spec(id: &str) -> Option<&'static FieldSpec> {
    crate::core::institution::forms::field(id)
}

/// The entry of a screen, a form or a field, with what uses it. No AI and no base: it is in the program.
pub fn entry(id: &str) -> Option<EntryView> {
    let entry = MANUAL.iter().find(|e| e.id == id)?.clone();
    let used_by = field_spec(id).map(|f| f.used_by.to_vec()).unwrap_or_default();
    Some(EntryView { entry, used_by })
}

/// Rebuilds the index when the manual changed (after installing or updating the program).
pub fn ensure_index(conn: &Connection) -> Result<(), ServiceError> {
    let v = version();
    let indexed: Option<String> = conn.query_row("SELECT version FROM manual_index LIMIT 1", [], |r| r.get(0)).optional()?;
    if indexed.as_deref() == Some(v.as_str()) {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM manual_fts", [])?;
    for e in MANUAL.iter() {
        tx.execute("INSERT INTO manual_fts (id, title, body) VALUES (?1, ?2, ?3)", params![e.id, e.title, e.paragraphs.join("\n")])?;
    }
    tx.execute("DELETE FROM manual_index", [])?;
    tx.execute("INSERT INTO manual_index (version) VALUES (?1)", [&v])?;
    tx.commit()?;
    Ok(())
}

fn snippet(e: &Entry) -> String {
    let first = e.paragraphs.first().cloned().unwrap_or_default();
    if first.chars().count() <= 160 {
        return first;
    }
    let cut: String = first.chars().take(157).collect();
    format!("{}…", cut.trim_end())
}

/// The entries that best answer a question, best first (the title weighs more than the text). Works offline.
pub fn search(conn: &Connection, query: &str, limit: usize) -> Result<Vec<Hit>, ServiceError> {
    let Some(terms) = crate::storage::fts::match_terms(query) else { return Ok(Vec::new()) };
    ensure_index(conn)?;
    let mut stmt = conn.prepare("SELECT id FROM manual_fts WHERE manual_fts MATCH ?1 ORDER BY bm25(manual_fts, 0.0, 5.0, 1.0) LIMIT ?2")?;
    let ask = limit + crate::core::modules::HIDDEN.len();
    let ids = stmt.query_map(params![terms, ask as i64], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(ids
        .into_iter()
        .filter_map(|id| MANUAL.iter().find(|e| e.id == id))
        .filter(|e| !about_a_hidden_module(e))
        .take(limit)
        .map(|e| Hit { id: e.id.clone(), title: e.title.clone(), snippet: snippet(e) })
        .collect())
}

/// The screen of a frozen module, or a doubt done there (ADR-043): the program does not show it, so the search does
/// not answer with it. Its entry stays in the manual for when the module is turned back on.
fn about_a_hidden_module(e: &Entry) -> bool {
    [Some(e.id.as_str()), e.at.as_deref()].into_iter().flatten().filter_map(|id| id.strip_prefix("screen.")).any(|page| !crate::core::modules::shown(page))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::institution::forms::{forms, LIST_FORMS};

    fn conn() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let c = crate::storage::open_encrypted(&dir.path().join("t.db"), "k").unwrap();
        (dir, c)
    }

    /// ADR-033 §1 and ADR-034: the manual is part of the product. Every field of the catalog has its entry, saying
    /// what to put and what it is for, and every form has one too.
    #[test]
    fn every_field_of_the_catalog_has_its_entry_in_the_manual() {
        for form in forms().iter().chain(LIST_FORMS) {
            assert!(entry(form.id).is_some(), "the form {} has no entry in docs/manual", form.id);
            for f in form.fields() {
                let e = entry(f.id).unwrap_or_else(|| panic!("the field {} has no entry in docs/manual", f.id));
                assert!(e.entry.paragraphs.iter().any(|p| p.starts_with("Qué poner: ")), "{}: no «Qué poner:»", f.id);
                assert!(e.entry.paragraphs.iter().any(|p| p.starts_with("Para qué sirve: ")), "{}: no «Para qué sirve:»", f.id);
                assert_eq!(e.used_by, f.used_by, "what uses it comes from the catalog");
            }
        }
    }

    /// Every document of the record says what it is (ADR-033 §5): the «?» of the expediente answers it.
    #[test]
    fn every_document_of_the_record_has_its_entry() {
        for (code, _) in crate::core::institution::catalog::RECORD_TYPES {
            assert!(entry(&format!("record.{code}")).is_some(), "the document {code} has no entry in docs/manual");
        }
    }

    /// An entry about a field that no longer exists would explain a screen that is not there.
    #[test]
    fn the_manual_has_unique_keys_and_no_entry_for_a_field_that_does_not_exist() {
        let ids: Vec<&str> = MANUAL.iter().map(|e| e.id.as_str()).collect();
        assert!((1..ids.len()).all(|i| !ids[..i].contains(&ids[i])), "repeated key in {ids:?}");
        for id in &ids {
            let kind = id.split('.').next().unwrap();
            if kind == "record" {
                let code = &id["record.".len()..];
                assert!(crate::core::institution::catalog::RECORD_TYPES.iter().any(|(c, _)| *c == code), "the manual speaks of the document «{id}», which is not in the catalog");
                continue;
            }
            if !matches!(kind, "screen" | "howto") && forms().iter().chain(LIST_FORMS).all(|f| f.id != *id) {
                assert!(field_spec(id).is_some(), "the manual speaks of «{id}», which is not in the catalog");
            }
        }
        assert!(MANUAL.iter().all(|e| !e.paragraphs.is_empty() && !e.title.is_empty()));
    }

    #[test]
    fn the_manual_is_written_in_plain_words() {
        for e in MANUAL.iter() {
            let text = format!("{} {}", e.title, e.paragraphs.join(" ")).to_lowercase();
            for jargon in ["json", "prompt", "token", "input", "output", "dashboard", "campo obligatorio", "validación", " pii", "null"] {
                assert!(!text.contains(jargon), "«{jargon}» in {}", e.id);
            }
            assert!(!text.contains("{#") && !text.contains("**"), "{}: marks of the source", e.id);
        }
    }

    #[test]
    fn a_heading_carries_its_key_and_lines_are_paragraphs() {
        let m = parse("# Pantalla {#screen.x}\n\nUno.\nDos.\n\n# Sin clave\nNo cuenta.\n## Campo {#a.b}\nQué poner: algo.\n# Cómo {#howto.y @screen.x}\nAsí.\n");
        assert_eq!(m.len(), 3);
        assert_eq!((m[0].id.as_str(), m[0].title.as_str(), m[0].paragraphs.len(), m[0].at.as_deref()), ("screen.x", "Pantalla", 2, None));
        assert_eq!(m[1].paragraphs, vec!["Qué poner: algo."]);
        assert_eq!((m[2].id.as_str(), m[2].title.as_str(), m[2].at.as_deref()), ("howto.y", "Cómo", Some("screen.x")), "the place it names");
    }

    #[test]
    fn the_search_works_without_the_ai_and_without_accents() {
        let (_d, c) = conn();
        let hits = search(&c, "¿qué pongo en a qué se dedica?", 5).unwrap();
        assert_eq!(hits[0].id, "institution.mission", "{hits:?}");
        // without accents, and a plural finds the singular
        assert_eq!(search(&c, "respaldo computadora", 3).unwrap()[0].id, "howto.backup");
        assert!(search(&c, "Convocatorias: ¿a quién atienden?", 3).unwrap().iter().any(|h| h.id == "institution.populations"));
        // Proyectos is frozen: the manual does not answer with its screen (ADR-043)
        assert!(search(&c, "proyecto convocatoria", 3).unwrap().iter().all(|h| h.id != "screen.projects"));
        assert!(search(&c, "que como", 5).unwrap().is_empty(), "only empty words: nothing");
        // what the person writes never reaches FTS5 as syntax
        assert!(search(&c, "\" OR * NEAR( -", 5).unwrap().is_empty());
        assert!(search(&c, "zzzz", 5).unwrap().is_empty());
    }

    #[test]
    fn the_index_is_built_once_and_rebuilt_only_when_the_manual_changes() {
        let (_d, c) = conn();
        ensure_index(&c).unwrap();
        let n = |c: &Connection| c.query_row("SELECT count(*) FROM manual_fts", [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(n(&c) as usize, MANUAL.len());
        ensure_index(&c).unwrap();
        assert_eq!(n(&c) as usize, MANUAL.len(), "not indexed twice");
        c.execute("UPDATE manual_index SET version = 'old'", []).unwrap();
        c.execute("DELETE FROM manual_fts WHERE id = 'howto.ai'", []).unwrap();
        ensure_index(&c).unwrap();
        assert_eq!(n(&c) as usize, MANUAL.len(), "rebuilt after an update");
    }
}
