//! The record of the institution (ADR-033 §5, block N4): the list of what it must have, each document with its
//! state, and the file only when there is one ("we have the deed but did not upload it" counts too).
//!
//! - **Which documents apply** is decided here, from what the institution says of itself: its legal form, whether it
//!   is a donee or has its CLUNI, whom and how it serves (`catalog::RECORD_TYPES`). The institution adds the ones the
//!   catalog does not know (`other`).
//! - **Validity** is reckoned from the date written in each document: valid, expiring (30 days or less) or expired.
//!   Rules of the Junta or the SAT come later (ADR-033 §5).
//! - **A file** (PDF, Word or a photo) is read with what reads the calls; a scanned page or a photo is read as a
//!   picture (OCR, ADR-035). Its text goes through the scanner like any text (quarantine) and only the clean text is
//!   kept, in `document`. Of the original, only where it is on this computer (principle 6). A picture with no text
//!   that can be read still counts as had, and the original is still opened from where it is.
//! - **Nothing of it reaches the AI yet** (N5); the folio and the notes never will.

pub mod background;
pub mod placing;
pub mod storage;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod full_tests;

use crate::common::dates::days_between;
use crate::common::forms::{is_filled, FormSpec, Values};
use crate::core::archive::service::cover_pages;
use crate::core::archive::storage::PageMarks;
use crate::core::error::ServiceError;
use crate::documents::reading::{self, ReadError};
use crate::core::history::{self, Source};
use crate::core::institution::catalog::{Applies, LODGING, MINORS, PREMISES, RECORD_OTHER, RECORD_TYPES, UNDER_A_JUNTA};
use crate::core::institution::forms::{RECORD, RECORD_KNOWN};
use crate::core::profile::domain::ProfileIssue;
use crate::core::reader::kinds::Placement;
use crate::scanner::guard::{Decision, QuarantineReport};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use storage as store;

/// How many days before its date a document is said to be expiring (decision of the institution, 2026-10-10).
pub const EXPIRING_DAYS: i64 = 30;
/// The largest file taken.
pub const MAX_FILE_BYTES: u64 = 40 * 1024 * 1024;
/// Fewer letters than this in every page: there is no text to read (a scan or a photo).
const MIN_LETTERS_PER_PAGE: usize = 20;
/// What the history writes when the record changes: that it changed, never a value.
const HISTORY_FIELD: &str = "institution.record";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Validity {
    Valid,
    Expiring,
    Expired,
}

/// The file of a document, as the screen shows it: its name, whether its text could be read, and whether it was
/// read from a picture (and which pages with doubts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordFile {
    pub name: String,
    pub readable: bool,
    pub from_picture: bool,
    pub doubtful: Vec<usize>,
}

/// A document of the record, as «Mi institución» shows it. Never its folio nor its notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Record {
    /// `None` while nothing was written of it.
    pub id: Option<String>,
    /// A type of the catalog, or `other`.
    #[serde(rename = "type")]
    pub kind: String,
    pub custom_name: Option<String>,
    /// `have`, `in_progress`, `missing` or `not_applicable`; `missing` while nothing was written.
    pub status: String,
    pub issued_on: Option<String>,
    pub valid_until: Option<String>,
    pub validity: Option<Validity>,
    /// Days until its date (negative once it passed), while the institution has it.
    pub days_left: Option<i64>,
    pub file: Option<RecordFile>,
    pub origin: Option<String>,
    pub confirmed_at: Option<String>,
}

/// «Tiene 9 de 12»: of the documents that apply (but the ones marked as not applying), how many it has, which are in
/// process or missing, and how many of the ones it has are expiring or expired.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub expected: usize,
    pub have: usize,
    pub in_progress: usize,
    pub missing: usize,
    pub expiring: usize,
    pub expired: usize,
}

impl Summary {
    /// What it has and is not expired: «expediente vigente» of the monthly photo.
    pub fn current(&self) -> usize {
        self.have - self.expired
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Expediente {
    pub records: Vec<Record>,
    pub summary: Summary,
}

/// A document ready to draw in its window.
#[derive(Debug, Serialize)]
pub struct RecordForm {
    pub spec: &'static FormSpec,
    pub values: Values,
    pub missing: Vec<&'static str>,
    pub file: Option<RecordFile>,
    /// The data of its window that came from its file, by field: the page, and whether a rule worked it out (ADR-038).
    pub read: std::collections::BTreeMap<String, ReadFrom>,
}

/// Where a datum of a document's window came from, when it was read from its file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadFrom {
    pub page: Option<i64>,
    /// Worked out by a rule from what the file says (the validity of a 32-D), not written in it.
    pub computed: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RecordOutcome {
    Saved { id: String, expediente: Expediente },
    /// A value that cannot be kept: the field and why. Nothing was saved.
    Invalid { issues: Vec<ProfileIssue> },
    /// The name or the notes have data of a person: nothing was saved until the person decides.
    Quarantine { report: QuarantineReport },
}

/// Why a file was not taken; the screen says each one in plain words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unreadable {
    /// Not a PDF, a Word file (.docx) or a photo.
    UnsupportedType,
    TooLarge,
    /// It is not where the person said, or it cannot be opened.
    NotFound,
    Damaged,
    /// More scanned pages than can be read in one file (`reading::MAX_PICTURE_PAGES`).
    TooManyPages,
    /// It asks for a password to be opened.
    Protected,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AttachOutcome {
    /// The file is on the document. `readable = false`: a picture with no text that could be read;
    /// `from_picture`: its text was read from a picture, and `doubtful` are the pages read with doubts;
    /// `covered`: how many data of people the scanner covered in its text; `filled`: the fields of its own window
    /// written from the file; `suggested`: how many suggestions it left in «Mi institución».
    Attached {
        id: String,
        readable: bool,
        from_picture: bool,
        doubtful: Vec<usize>,
        covered: i64,
        filled: Vec<String>,
        suggested: usize,
        expediente: Expediente,
    },
    /// Not kept: the code is not sure the file is the document it was put on (ADR-042 §4), and it waits for the
    /// person (`placing`). `looks_like`: the document its words say, if any, and whether that one already has a file
    /// (`there_has_file`), which it would replace.
    Ask { looks_like: Option<String>, file_name: String, there_has_file: bool },
    Unreadable { reason: Unreadable },
}

fn text(v: &Values, field: &str) -> Option<String> {
    v.get(field).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn list(v: &Values, field: &str) -> Vec<String> {
    v.get(field).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

/// Whether a type applies to the institution, from what it says of itself now.
fn applies(a: Applies, identity: &Values, legal: &Values) -> bool {
    let one_of = |field: &str, codes: &[&str]| text(legal, field).is_some_and(|v| codes.contains(&v.as_str()));
    let serves_by = |ways: &[&str]| list(identity, "institution.modalities").iter().any(|m| ways.contains(&m.as_str()));
    match a {
        Applies::Always => true,
        Applies::Donee => one_of("institution.authorized_donee", &["yes", "in_progress"]),
        Applies::Cluni => one_of("institution.cluni", &["yes", "in_progress"]),
        Applies::Junta => one_of("institution.legal_form", UNDER_A_JUNTA),
        Applies::MinorsLiving => list(identity, "institution.populations").iter().any(|p| MINORS.contains(&p.as_str())) && serves_by(&["residential"]),
        Applies::Housing => serves_by(LODGING),
        Applies::Premises => serves_by(PREMISES),
    }
}

fn known(kind: &str) -> bool {
    kind == RECORD_OTHER || RECORD_TYPES.iter().any(|(code, _)| *code == kind)
}

/// Today on this computer (`YYYY-MM-DD`).
fn today(conn: &Connection) -> Result<String, ServiceError> {
    Ok(conn.query_row("SELECT date('now', 'localtime')", [], |r| r.get(0))?)
}

fn file_of(row: &store::Row) -> Option<RecordFile> {
    row.original_name.clone().map(|name| RecordFile {
        name,
        readable: row.document_shown,
        from_picture: row.document_shown && row.picture_pages > 0,
        doubtful: if row.document_shown { row.doubtful_pages.clone() } else { vec![] },
    })
}

fn record(kind: &str, row: Option<&store::Row>, today: &str) -> Record {
    let status = row.map_or("missing".to_string(), |r| r.status.clone());
    let valid_until = row.and_then(|r| r.valid_until.clone());
    let days_left = valid_until.as_deref().filter(|_| status == "have").and_then(|d| days_between(today, d));
    let validity = days_left.map(|d| match d {
        d if d < 0 => Validity::Expired,
        d if d <= EXPIRING_DAYS => Validity::Expiring,
        _ => Validity::Valid,
    });
    Record {
        id: row.map(|r| r.id.clone()),
        kind: kind.to_string(),
        custom_name: row.and_then(|r| r.custom_name.clone()),
        status,
        issued_on: row.and_then(|r| r.issued_on.clone()),
        valid_until,
        validity,
        days_left,
        file: row.and_then(file_of),
        origin: row.map(|r| r.origin.clone()),
        confirmed_at: row.and_then(|r| r.confirmed_at.clone()),
    }
}

/// The record of the institution as of a day: the types that apply, in the order of the catalog, then the ones the
/// institution named. A type that no longer applies still shows while the institution has it or has it in process.
pub fn expediente_on(conn: &Connection, today: &str) -> Result<Expediente, ServiceError> {
    let rows = store::rows(conn)?;
    let identity = crate::core::profile::forms::saved_values(conn, "institution.identity")?;
    let legal = crate::core::profile::forms::saved_values(conn, "institution.legal")?;
    let mut records = Vec::new();
    for (code, rule) in RECORD_TYPES {
        let row = rows.iter().find(|r| r.r#type == *code);
        let held = row.is_some_and(|r| r.status == "have" || r.status == "in_progress");
        if applies(*rule, &identity, &legal) || held {
            records.push(record(code, row, today));
        }
    }
    records.extend(rows.iter().filter(|r| r.r#type == RECORD_OTHER).map(|r| record(RECORD_OTHER, Some(r), today)));

    let mut s = Summary::default();
    for r in records.iter().filter(|r| r.status != "not_applicable") {
        s.expected += 1;
        match r.status.as_str() {
            "have" => s.have += 1,
            "in_progress" => s.in_progress += 1,
            _ => s.missing += 1,
        }
        match r.validity {
            Some(Validity::Expiring) => s.expiring += 1,
            Some(Validity::Expired) => s.expired += 1,
            _ => {}
        }
    }
    Ok(Expediente { records, summary: s })
}

pub fn expediente(conn: &Connection) -> Result<Expediente, ServiceError> {
    expediente_on(conn, &today(conn)?)
}

/// The row a request speaks of: by its id, or the one of a type of the catalog. `Err(NotFound)` for a type the code
/// does not know or an id of another type.
fn row_of(conn: &Connection, kind: &str, id: Option<&str>) -> Result<Option<store::Row>, ServiceError> {
    if !known(kind) {
        return Err(ServiceError::NotFound);
    }
    match id {
        Some(id) => match store::by_id(conn, id)? {
            Some(r) if r.r#type == kind => Ok(Some(r)),
            _ => Err(ServiceError::NotFound),
        },
        None if kind == RECORD_OTHER => Ok(None),
        None => Ok(store::by_type(conn, kind)?),
    }
}

fn spec_of(kind: &str) -> &'static FormSpec {
    if kind == RECORD_OTHER {
        &RECORD
    } else {
        &RECORD_KNOWN
    }
}

/// The window of a document: what is written of it, or empty.
pub fn form(conn: &Connection, kind: &str, id: Option<&str>) -> Result<RecordForm, ServiceError> {
    let row = row_of(conn, kind, id)?;
    let spec = spec_of(kind);
    let mut values = Values::new();
    if let Some(r) = &row {
        values.insert("institution.record.custom_name".into(), json!(r.custom_name));
        values.insert("institution.record.status".into(), json!(r.status));
        values.insert("institution.record.issued_on".into(), json!(r.issued_on));
        values.insert("institution.record.valid_until".into(), json!(r.valid_until));
        values.insert("institution.record.reference".into(), json!(r.reference));
        values.insert("institution.record.notes".into(), json!(r.notes));
    }
    values.retain(|k, v| is_filled(Some(v)) && spec.field(k).is_some());
    let mut read = std::collections::BTreeMap::new();
    if let Some(r) = &row {
        for (field, (origin, source_ref)) in store::field_origins(conn, &r.id)? {
            if origin == "user" || !values.contains_key(&field) {
                continue;
            }
            let page = source_ref.as_deref().and_then(|s| serde_json::from_str::<Value>(s).ok()).and_then(|v| v["page"].as_i64());
            read.insert(field, ReadFrom { page, computed: origin == "computed" });
        }
    }
    Ok(RecordForm { spec, missing: spec.missing(&values), file: row.as_ref().and_then(file_of), values, read })
}

/// Saves what the person writes of a document. A status out of its list, a date that does not exist or a validity
/// before the issue date is refused with its field; the name of an `other` is needed. The name and the notes go
/// through the scanner. The document and its line in the history go together.
pub fn save(conn: &mut Connection, kind: &str, id: Option<&str>, mut values: Values, decision: Option<Decision>) -> Result<RecordOutcome, ServiceError> {
    let row = row_of(conn, kind, id)?;
    let spec = spec_of(kind);
    spec.clear_hidden(&mut values);
    let mut issues: Vec<ProfileIssue> = spec
        .validate(&values)
        .into_iter()
        // a document is not filled little by little: what it needs is needed now
        .map(|i| ProfileIssue { code: if i.code == "missing" { "required" } else { i.code }, field: i.field.to_string(), blocking: true })
        .collect();
    if let (Some(from), Some(to)) = (text(&values, "institution.record.issued_on"), text(&values, "institution.record.valid_until")) {
        if days_between(&from, &to).is_some_and(|d| d < 0) && issues.is_empty() {
            issues.push(ProfileIssue { code: "valid_before_issued", field: "institution.record.valid_until".into(), blocking: true });
        }
    }
    if !issues.is_empty() {
        issues.sort_by_key(|i| spec.fields().position(|f| f.id == i.field));
        return Ok(RecordOutcome::Invalid { issues });
    }
    // the free texts of the person, through the scanner
    let free: Vec<(String, String)> = ["institution.record.custom_name", "institution.record.notes"]
        .iter()
        .filter_map(|f| text(&values, f).map(|t| (f.to_string(), t)))
        .collect();
    let mut cleaned = match crate::core::screen::guard_texts(conn, "core_record", &free, decision)? {
        Ok(texts) => free.iter().map(|(f, _)| f.clone()).zip(texts).collect::<std::collections::BTreeMap<_, _>>(),
        Err(report) => return Ok(RecordOutcome::Quarantine { report }),
    };
    let (custom_name, notes) = (cleaned.remove("institution.record.custom_name"), cleaned.remove("institution.record.notes"));
    let (status, issued_on, valid_until, reference) = (
        text(&values, "institution.record.status").unwrap_or_default(),
        text(&values, "institution.record.issued_on"),
        text(&values, "institution.record.valid_until"),
        text(&values, "institution.record.reference"),
    );
    let input = store::Input {
        custom_name: custom_name.as_deref().filter(|_| kind == RECORD_OTHER),
        status: &status,
        issued_on: issued_on.as_deref(),
        valid_until: valid_until.as_deref(),
        reference: reference.as_deref(),
        notes: notes.as_deref(),
    };
    let tx = conn.transaction()?;
    let id = store::save(&tx, row.as_ref().map(|r| r.id.as_str()), kind, &input)?;
    // a datum the person changed is the person's: no file writes over it again; one emptied may be filled again
    for (field, before, after) in [
        ("institution.record.issued_on", row.as_ref().and_then(|r| r.issued_on.clone()), issued_on.clone()),
        ("institution.record.valid_until", row.as_ref().and_then(|r| r.valid_until.clone()), valid_until.clone()),
        ("institution.record.reference", row.as_ref().and_then(|r| r.reference.clone()), reference.clone()),
    ] {
        if before == after {
            continue;
        }
        match after {
            Some(_) => store::set_field_origin(&tx, &id, field, "user", None)?,
            None => store::clear_field_origin(&tx, &id, field)?,
        }
    }
    history::record_list_change(&tx, HISTORY_FIELD, &Source::user())?;
    tx.commit()?;
    Ok(RecordOutcome::Saved { id, expediente: expediente(conn)? })
}

/// The text of a file, page by page, or `None` when it has none to read (a picture with no text that could be
/// read), with the pages read from a picture.
pub struct FileText {
    name: String,
    mime: &'static str,
    pages: Option<Vec<String>>,
    marks: PageMarks,
}

impl FileText {
    /// A file already read, with its text page by page: for the tests of what reads the Expediente.
    #[cfg(test)]
    pub(crate) fn of_pages(name: &str, pages: Vec<String>) -> FileText {
        FileText { name: name.to_string(), mime: "application/pdf", pages: Some(pages), marks: PageMarks::default() }
    }

    /// Its text page by page: for the measurement of how sure the code is of a file's kind (ADR-042 §4).
    #[cfg(test)]
    pub(crate) fn pages_for_test(&self) -> Option<Vec<String>> {
        self.pages.clone()
    }
}

/// Reads a file from where it is on this computer. A scanned page or a photo is read as a picture, a few seconds a
/// page: the caller does it without holding the database.
pub fn read_original(path: &Path) -> Result<FileText, Unreadable> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).ok_or(Unreadable::NotFound)?;
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mime = match ext.as_str() {
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        _ => return Err(Unreadable::UnsupportedType),
    };
    let size = std::fs::metadata(path).map_err(|_| Unreadable::NotFound)?.len();
    if size > MAX_FILE_BYTES {
        return Err(Unreadable::TooLarge);
    }
    let raw = reading::read_file(path).map_err(|e| match e {
        ReadError::TooManyPictures => Unreadable::TooManyPages,
        ReadError::Unsupported => Unreadable::UnsupportedType,
        ReadError::Damaged => Unreadable::Damaged,
        ReadError::Protected => Unreadable::Protected,
    })?;
    let pages: Vec<String> = raw.pages.iter().map(|p| crate::documents::text::clean_text(p)).collect();
    let letters = |t: &str| t.chars().filter(|c| c.is_alphabetic()).count();
    if pages.iter().all(|p| letters(p) < MIN_LETTERS_PER_PAGE) {
        return Ok(FileText { name, mime, pages: None, marks: PageMarks::default() });
    }
    Ok(FileText { name, mime, pages: Some(pages), marks: PageMarks { from_picture: raw.from_picture, doubtful: raw.doubtful } })
}

/// `read_original` and `attach_read` in one go. The command reads first without holding the database.
#[cfg(test)]
pub fn attach(conn: &mut Connection, kind: &str, id: Option<&str>, path: &Path) -> Result<AttachOutcome, ServiceError> {
    let row = row_of(conn, kind, id)?;
    if kind == RECORD_OTHER && row.is_none() {
        return Err(ServiceError::NotFound);
    }
    match read_original(path) {
        Ok(file) => attach_read(conn, kind, id, path, file),
        Err(reason) => Ok(AttachOutcome::Unreadable { reason }),
    }
}

/// Puts a file already read (`read_original`) on a document: a PDF, a Word file or a photo, from where it is on this
/// computer. Its text goes through the scanner, which covers the data of people it finds (a deed names its partners),
/// without refusing it nor asking (decision of the institution, 2026-10-13); only the clean text is kept,
/// and of the original only where it is. A document of the catalog with nothing written is created as had; an
/// `other` must exist.
///
/// The file is kept only if the code is sure it is the document it was put on (ADR-042 §4, `kinds::placement`);
/// otherwise nothing is written, it waits in `placing`, and the person is asked.
pub fn attach_read(conn: &mut Connection, kind: &str, id: Option<&str>, path: &Path, file: FileText) -> Result<AttachOutcome, ServiceError> {
    if kind == RECORD_OTHER && row_of(conn, kind, id)?.is_none() {
        return Err(ServiceError::NotFound);
    }
    let looks_like = match crate::core::reader::kinds::placement(file.pages.as_deref(), &file.name, kind) {
        Placement::Sure => return keep(conn, kind, id, path, file),
        Placement::Other(other) => Some(other),
        Placement::Unsure => None,
    };
    let there_has_file = match looks_like {
        Some(other) => store::by_type(conn, other)?.is_some_and(|r| r.original_path.is_some() || r.document_id.is_some()),
        None => false,
    };
    let file_name = file.name.clone();
    placing::wait(kind, id, path.to_path_buf(), file, looks_like);
    Ok(AttachOutcome::Ask { looks_like: looks_like.map(str::to_string), file_name, there_has_file })
}

/// Keeps a file on its document: read by the rules of that document, its text through the scanner, its own window
/// written and its suggestions left (ADR-038).
fn keep(conn: &mut Connection, kind: &str, id: Option<&str>, path: &Path, file: FileText) -> Result<AttachOutcome, ServiceError> {
    let row = row_of(conn, kind, id)?;
    if kind == RECORD_OTHER && row.is_none() {
        return Err(ServiceError::NotFound);
    }
    // the Reader reads before the scanner covers (IA6): the kind is the one the person attached the file to
    let reading = file.pages.as_deref().map(|pages| crate::core::reader::read(pages, Some(kind)));
    let clean = match &file.pages {
        Some(pages) => Some(cover_pages(conn, "core_record", pages, !file.marks.from_picture.is_empty())?),
        None => None,
    };
    // the text of the file it replaces leaves the drawer: a document of the Expediente has one file (ADR-036)
    let replaced = row.as_ref().and_then(|r| r.document_id.clone());
    let tx = conn.transaction()?;
    let id = match row {
        Some(r) => r.id,
        None => store::save(&tx, None, kind, &store::Input { custom_name: None, status: "have", issued_on: None, valid_until: None, reference: None, notes: None })?,
    };
    let doc = match &clean {
        Some(c) => Some(crate::core::archive::storage::add_paged_document(&tx, crate::core::archive::storage::Drawer::Record, &file.name, file.mime, &c.pages, c.covered, &file.marks)?),
        None => None,
    };
    let (mut filled, mut suggested) = (vec![], 0);
    if let (Some(d), Some(reading)) = (&doc, &reading) {
        let class = reading.kind.and_then(crate::core::reader::kinds::kind).map(|k| k.class);
        // the kind is the document the person put it on; for an «Otro documento», what a rule recognized
        let said = crate::core::reader::kinds::kind(kind).is_some();
        crate::core::archive::storage::set_class(&tx, d, class, reading.kind, if said { "user" } else { "computed" })?;
        filled = write_own(&tx, &id, d, &reading.found)?;
        suggested = crate::core::reader::propose(&tx, d, reading.kind, &reading.found)?;
    }
    store::set_file(&tx, &id, doc.as_deref(), &path.to_string_lossy(), &file.name)?;
    history::record_list_change(&tx, HISTORY_FIELD, &Source::user())?;
    tx.commit()?;
    if let Some(old) = replaced.filter(|old| doc.as_deref() != Some(old.as_str())) {
        crate::core::archive::storage::emergency_delete_document(conn, &old)?;
    }
    let readable = doc.is_some();
    let (from_picture, doubtful) = if readable { (!file.marks.from_picture.is_empty(), file.marks.doubtful) } else { (false, vec![]) };
    let covered = clean.map_or(0, |c| c.covered);
    Ok(AttachOutcome::Attached {
        id,
        readable,
        from_picture,
        doubtful,
        covered,
        filled,
        suggested,
        expediente: expediente(conn)?,
    })
}

/// Writes in a document's own window what its file says of it (its date of issue, its validity, its number): a
/// datum still empty, or one read from the file it replaces; never one the person wrote (ADR-038). A date the window
/// would refuse (a validity before the issue) is left out. Says which fields were written.
fn write_own(tx: &Connection, id: &str, document_id: &str, found: &[crate::core::reader::rules::Found]) -> Result<Vec<String>, ServiceError> {
    let row = store::by_id(tx, id)?.ok_or(ServiceError::NotFound)?;
    let origins = store::field_origins(tx, id)?;
    let now = |field: &str| match field {
        "institution.record.issued_on" => row.issued_on.clone(),
        "institution.record.valid_until" => row.valid_until.clone(),
        "institution.record.reference" => row.reference.clone(),
        _ => None,
    };
    let mut written: Vec<String> = vec![];
    let mut issued = row.issued_on.clone();
    // the date of issue first: the validity is checked against it
    let mut own: Vec<_> = found.iter().filter(|f| crate::core::reader::is_own(f)).collect();
    own.sort_by_key(|f| f.field != "institution.record.issued_on");
    for f in own {
        let Some(value) = f.value.as_str().map(str::trim).filter(|v| !v.is_empty()) else { continue };
        if written.iter().any(|w| w == f.field) {
            continue;
        }
        let from_a_file = origins.get(f.field).is_some_and(|(o, _)| o != "user");
        let empty = now(f.field).is_none_or(|v| v.trim().is_empty());
        // a datum written before origins were kept has no line: it is the person's
        if !(empty || from_a_file) {
            continue;
        }
        if f.field == "institution.record.valid_until" && issued.as_deref().is_some_and(|i| days_between(i, value).is_some_and(|d| d < 0)) {
            continue;
        }
        let protected = crate::core::institution::forms::field(f.field).is_some_and(|s| s.protected());
        let mut source = json!({ "document_id": document_id, "page": f.page });
        if !protected {
            source["quote"] = json!(f.line);
        }
        store::write_read(tx, id, f.field, value)?;
        store::set_field_origin(tx, id, f.field, if f.computed { "computed" } else { "document" }, Some(&source.to_string()))?;
        if f.field == "institution.record.issued_on" {
            issued = Some(value.to_string());
        }
        written.push(f.field.to_string());
    }
    Ok(written)
}

/// The document in the drawer that holds the text of a document's file, if it has one.
pub fn document_of(conn: &Connection, id: &str) -> Result<Option<String>, ServiceError> {
    Ok(store::by_id(conn, id)?.and_then(|r| r.document_id))
}

/// Development only: an example of `fixtures/` brings its own record, so the one there is goes first.
pub fn clear_for_example(conn: &Connection) -> Result<(), ServiceError> {
    Ok(store::clear_all(conn)?)
}

/// Takes the file off a document, and its clean text out of the drawer (ADR-036): there is no document without a
/// drawer, nor a text of the Expediente without its document. What the person already accepted from it stays.
pub fn detach(conn: &mut Connection, id: &str) -> Result<Expediente, ServiceError> {
    let old = store::by_id(conn, id)?.and_then(|r| r.document_id);
    let tx = conn.transaction()?;
    if !store::clear_file(&tx, id)? {
        return Err(ServiceError::NotFound);
    }
    history::record_list_change(&tx, HISTORY_FIELD, &Source::user())?;
    tx.commit()?;
    if let Some(old) = old {
        crate::core::archive::storage::emergency_delete_document(conn, &old)?;
    }
    expediente(conn)
}

/// Where the original of a document is, if it is still there.
pub fn original(conn: &Connection, id: &str) -> Result<Option<std::path::PathBuf>, ServiceError> {
    let row = store::by_id(conn, id)?.ok_or(ServiceError::NotFound)?;
    Ok(row.original_path.map(std::path::PathBuf::from).filter(|p| p.is_file()))
}

/// How a document is named when nobody says what it is.
const UNNAMED: &str = "Documento del Expediente";

/// How the AI knows a document of the Expediente: by its type, with the name the screen gives it; one the institution
/// named itself («Otro documento»), by that name, which went through the quarantine. Never by the name of its file
/// (decision of the institution, 2026-10-13): that is only the reference of a quote, on the screen.
fn type_name(kind: &str, custom_name: Option<&str>) -> String {
    if kind == RECORD_OTHER {
        custom_name.map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Otro documento").to_string()
    } else {
        crate::core::institution::forms::record_type_label(kind).unwrap_or(UNNAMED).to_string()
    }
}

/// The name of a document of the store for the AI (see `type_name`).
pub fn ai_name(conn: &Connection, document_id: &str) -> Result<String, ServiceError> {
    Ok(store::by_document(conn, document_id)?.map_or(UNNAMED.to_string(), |r| type_name(&r.r#type, r.custom_name.as_deref())))
}

/// A document of the Expediente as the map of an agent tells it (ADR-036 §6): what it is and how it is, never what it
/// says. `file_name` stays with the code: it tells which document a person names, and never reaches the AI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapDocument {
    /// `D1`, `D2`…: how an agent asks for this document, in the order of the Expediente.
    pub key: String,
    pub name: String,
    /// `have`, `in_progress`, `missing` or `not_applicable`.
    pub status: String,
    pub validity: Option<Validity>,
    pub valid_until: Option<String>,
    /// Its clean text in the drawer, if its file could be read.
    pub document_id: Option<String>,
    pub from_picture: bool,
    pub file_name: Option<String>,
}

/// The documents of the Expediente for the map of an agent, in the order of the screen.
pub fn map(conn: &Connection) -> Result<Vec<MapDocument>, ServiceError> {
    let rows = store::rows(conn)?;
    Ok(expediente(conn)?
        .records
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            let row = r.id.as_deref().and_then(|id| rows.iter().find(|x| x.id == id));
            MapDocument {
                key: format!("D{}", i + 1),
                name: type_name(&r.kind, r.custom_name.as_deref()),
                status: r.status,
                validity: r.validity,
                valid_until: r.valid_until,
                document_id: row.filter(|x| x.document_shown).and_then(|x| x.document_id.clone()),
                from_picture: r.file.as_ref().is_some_and(|f| f.from_picture),
                file_name: r.file.map(|f| f.name),
            }
        })
        .collect())
}

/// The document of the map whose file the person names in a question («¿qué número tiene registro_JAP.png?»), with
/// or without its extension, in any case and accents. Found by the code: the AI never sees the names of the files.
pub fn named_in<'a>(map: &'a [MapDocument], question: &str) -> Option<&'a MapDocument> {
    // «registro_JAP.png», «registro-jap» and «Registro JAP» are the same name
    let words = |s: &str| crate::storage::fts::fold(s).split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    let said = format!(" {} ", words(question));
    map.iter().filter(|d| d.document_id.is_some()).find(|d| {
        let Some(name) = d.file_name.as_deref() else { return false };
        let stem = words(name.rsplit_once('.').map_or(name, |(s, _)| s));
        stem.chars().count() >= 3 && said.contains(&format!(" {stem} "))
    })
}
