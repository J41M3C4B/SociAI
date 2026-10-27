//! The Reader of the Expediente from a file to «Mi institución» (IA6, ADR-036): the data of a document as suggestions
//! with their citation, the protected ones read before the scanner covers them and forgotten once the person decides.
//! A file comes in only through its document of the Expediente: the kind is the one the person attached it to.

use super::*;
use crate::core::archive::search::tests::fixture_pages;
use crate::core::archive::storage as docs;
use crate::core::records::{attach_read, AttachOutcome, FileText};
use serde_json::{json, Value};

const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

fn conn() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let c = crate::storage::open_encrypted(&dir.path().join("t.db"), KEY).unwrap();
    (dir, c)
}

fn pages_of(name: &str) -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/expediente").join(name);
    fixture_pages(&std::fs::read_to_string(path).unwrap())
}

/// Puts a fictitious file on its document of the Expediente; the scanner covers what it finds.
fn attach(c: &mut Connection, kind: &str, file: &str) -> docs::DocumentSummary {
    let read = FileText::of_pages(file, pages_of(file));
    let out = attach_read(c, kind, None, std::path::Path::new("C:/expediente/archivo.pdf"), read).unwrap();
    assert!(matches!(out, AttachOutcome::Attached { readable: true, .. }), "{file}");
    docs::list_drawer(c, docs::Drawer::Record).unwrap().into_iter().find(|d| d.display_name == file).unwrap()
}

fn suggestions(c: &Connection, form: &str) -> Vec<proposals::Proposal> {
    crate::core::profile::forms::get(c, form).unwrap().proposals
}

/// The constancia del SAT on its document of the Expediente: its RFC covered in what is kept, and yet suggested in its
/// field with the document and the page, read by a rule and never by the AI.
#[test]
fn the_constancia_suggests_the_rfc_it_says_although_the_saved_text_covers_it() {
    let (_d, mut c) = conn();
    let doc = attach(&mut c, "tax_status_certificate", "constancia-situacion-fiscal.txt");
    assert_eq!((doc.drawer.as_deref(), doc.record_type.as_deref()), (Some("record"), Some("tax_status_certificate")));
    assert_eq!((doc.category.as_deref(), doc.doc_type.as_deref(), doc.doc_type_origin.as_deref()), (Some("legal"), Some("tax_status_certificate"), Some("user")));
    let kept: String = c.query_row("SELECT extracted_text FROM document WHERE id = ?1", [&doc.id], |r| r.get(0)).unwrap();
    assert!(kept.contains("[RFC OCULTO]") && !kept.contains("CRF150310KX4"), "what is kept stays covered");

    let s = suggestions(&c, "institution.legal");
    let rfc = s.iter().find(|p| p.field_id == "institution.legal_rfc").expect("the RFC is suggested");
    assert_eq!(rfc.value, json!("CRF150310KX4"));
    assert_eq!((rfc.origin, rfc.agent.as_str()), (Origin::Document, AGENT));
    assert_eq!((rfc.document.as_deref(), rfc.page), (Some("constancia-situacion-fiscal.txt"), Some(1)));
    assert!(rfc.said.is_none(), "a document is not «Usted dijo»");
    assert!(!rfc.source_ref.as_deref().unwrap().contains("CRF150310KX4"), "a protected datum keeps no line with it");
    let regime = s.iter().find(|p| p.field_id == "institution.tax_regime").unwrap();
    assert_eq!((regime.value.clone(), regime.page), (json!("non_profit"), Some(2)));
    assert!(regime.source_ref.as_deref().unwrap().contains("Fines no Lucrativos"), "a datum that is not protected keeps its line");
    let fields: Vec<&str> = s.iter().map(|p| p.field_id.as_str()).collect();
    for f in ["institution.legal_name", "institution.legal_form", "institution.fiscal_postal_code"] {
        assert!(fields.contains(&f), "{f}: {fields:?}");
    }
}

/// Accepting is the usual save: the datum keeps its origin `document`, and the proposal forgets the protected value.
/// The same value is not suggested again; and deleting the document afterwards works and keeps what was accepted
/// (its origin and its history say where it came from, ADR-036).
#[test]
fn accepting_keeps_the_origin_and_forgets_the_protected_value() {
    let (_d, mut c) = conn();
    // the person who accepts it: a datum read from a document is saved with the document and with who took it
    c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS session_actor (user_id TEXT); INSERT INTO session_actor VALUES ('usr_01JB3M7Q8ZK4X2N6V9R5T0W1YD');").unwrap();
    let name = [("institution.name".to_string(), json!("Casa de Reposo Ficticia San Ejemplo"))].into_iter().collect();
    crate::core::profile::forms::save(&mut c, "institution.identity", name, None).unwrap();
    let doc = attach(&mut c, "tax_status_certificate", "constancia-situacion-fiscal.txt");
    let rfc = suggestions(&c, "institution.legal").into_iter().find(|p| p.field_id == "institution.legal_rfc").unwrap();
    let mut values = crate::core::profile::forms::get(&c, "institution.legal").unwrap().values;
    values.insert("institution.legal_rfc".into(), json!("CRF150310KX4"));
    let out = crate::core::profile::forms::save_using(&mut c, "institution.legal", values, None, &[rfc.id.clone()]).unwrap();
    assert!(matches!(out, crate::core::profile::service::SaveProfileOutcome::Saved { .. }), "{out:?}");
    let view = crate::core::profile::forms::get(&c, "institution.legal").unwrap();
    assert_eq!(view.values["institution.legal_rfc"], json!("CRF150310KX4"));
    assert_eq!(view.origins["institution.legal_rfc"].origin, "document");
    let (status, value): (String, String) = c.query_row("SELECT status, value_json FROM ai_proposal WHERE id = ?1", [&rfc.id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((status.as_str(), value.as_str()), ("accepted", "null"));

    // the same file again on its document: the old text leaves the drawer, and the same value is not suggested again
    attach(&mut c, "tax_status_certificate", "constancia-situacion-fiscal.txt");
    assert!(suggestions(&c, "institution.legal").iter().all(|p| p.field_id != "institution.legal_rfc"));
    assert!(docs::list(&c).unwrap().iter().all(|d| d.id != doc.id), "a document of the Expediente has one file");

    // «delete this and everything that came from it» works although its datum was accepted, and the datum stays
    let now = docs::list_drawer(&c, docs::Drawer::Record).unwrap().remove(0);
    docs::emergency_delete_document(&mut c, &now.id).unwrap();
    assert_eq!(crate::core::profile::forms::get(&c, "institution.legal").unwrap().values["institution.legal_rfc"], json!("CRF150310KX4"));
    assert!(docs::list_drawer(&c, docs::Drawer::Record).unwrap().is_empty());
}

/// The oficio of donataria and the acta: their data, each in its field, with no AI; a kind with no rules suggests
/// nothing.
#[test]
fn the_oficio_and_the_acta_suggest_their_data_and_a_kind_with_no_rules_suggests_nothing() {
    let (_d, mut c) = conn();
    attach(&mut c, "donee_authorization", "oficio-donataria.txt");
    attach(&mut c, "articles_of_incorporation", "acta-constitutiva.txt");
    let s = suggestions(&c, "institution.legal");
    let value = |f: &str| s.iter().find(|p| p.field_id == f).map(|p| p.value.clone()).unwrap_or(Value::Null);
    assert_eq!(value("institution.authorized_donee"), json!("yes"));
    assert_eq!(value("institution.donee_letter_number"), json!("600-04-02-2017-11234"));
    assert_eq!(value("institution.donee_letter_date"), json!("2017-03-14"));
    assert_eq!(value("institution.donee_category"), json!("assistance"));
    assert_eq!(value("institution.founded_year"), json!(2015));
    assert!(value("institution.purpose").as_str().unwrap().contains("adultas mayores"));

    let before = c.query_row("SELECT count(*) FROM ai_proposal", [], |r| r.get::<_, i64>(0)).unwrap();
    let rules = attach(&mut c, "internal_rules", "reglamento-interno.txt");
    assert_eq!((rules.category.as_deref(), rules.doc_type.as_deref()), (Some("legal"), Some("internal_rules")));
    assert_eq!(c.query_row("SELECT count(*) FROM ai_proposal", [], |r| r.get::<_, i64>(0)).unwrap(), before);
    assert!(!c.query_row("SELECT count(*) > 0 FROM ai_proposal WHERE agent <> 'reader'", [], |r| r.get::<_, bool>(0)).unwrap(), "no AI was asked");
}

/// A file put on its document, as the person would, keeping what the Reader did.
fn attach_pages(c: &mut Connection, kind: &str, name: &str, pages: Vec<String>) -> AttachOutcome {
    attach_read(c, kind, None, std::path::Path::new("C:/expediente/archivo.pdf"), FileText::of_pages(name, pages)).unwrap()
}

fn pending_of(c: &Connection, form: &str, field: &str) -> Option<Value> {
    suggestions(c, form).into_iter().find(|p| p.field_id == field).map(|p| p.value)
}

/// ADR-038: a field already written gets no suggestion from any file, and saving a field sets aside what documents
/// suggested for it.
#[test]
fn a_written_field_gets_no_suggestion_and_saving_sets_aside_what_waited() {
    let (_d, mut c) = conn();
    let name = [("institution.name".to_string(), json!("Casa de Reposo Ficticia San Ejemplo"))].into_iter().collect();
    crate::core::profile::forms::save(&mut c, "institution.identity", name, None).unwrap();
    attach(&mut c, "tax_status_certificate", "constancia-situacion-fiscal.txt");
    assert!(pending_of(&c, "institution.legal", "institution.legal_name").is_some());
    // the person writes the name by hand: the suggestion for it is set aside, the others still wait
    let mut values = crate::core::profile::forms::get(&c, "institution.legal").unwrap().values;
    values.insert("institution.legal_name".into(), json!("Casa Ficticia, como la escribimos"));
    let out = crate::core::profile::forms::save(&mut c, "institution.legal", values, None).unwrap();
    assert!(matches!(out, crate::core::profile::service::SaveProfileOutcome::Saved { .. }), "{out:?}");
    assert_eq!(pending_of(&c, "institution.legal", "institution.legal_name"), None);
    assert!(pending_of(&c, "institution.legal", "institution.legal_rfc").is_some());
    let set_aside: String = c.query_row("SELECT status FROM ai_proposal WHERE field_id = 'institution.legal_name'", [], |r| r.get(0)).unwrap();
    assert_eq!(set_aside, "rejected");
    // another document that names it suggests nothing for it
    attach(&mut c, "junta_registration", "registro-junta.txt");
    assert_eq!(pending_of(&c, "institution.legal", "institution.legal_name"), None);
    assert!(pending_of(&c, "institution.legal", "institution.junta_folio").is_some(), "an empty field still gets its suggestion");
}

/// ADR-038: what the person rejected is not suggested again, not even when the same file is put on its document
/// again (the old one is deleted with what came from it, and only the fingerprint of the «no» is kept).
#[test]
fn what_the_person_rejected_is_not_suggested_again() {
    let (_d, mut c) = conn();
    attach(&mut c, "donee_authorization", "oficio-donataria.txt");
    let category = suggestions(&c, "institution.legal").into_iter().find(|p| p.field_id == "institution.donee_category").unwrap();
    proposals::reject(&c, &[category.id.clone()], None).unwrap();
    attach(&mut c, "donee_authorization", "oficio-donataria.txt");
    assert_eq!(pending_of(&c, "institution.legal", "institution.donee_category"), None);
    assert!(pending_of(&c, "institution.legal", "institution.donee_letter_number").is_some(), "what was not rejected comes back");
    let kept: (String, String) = c.query_row("SELECT value_json, source_ref FROM ai_proposal WHERE id = ?1", [&category.id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(kept.0, "null");
    assert!(!kept.1.contains("document_id") && kept.1.contains("sha"), "{}", kept.1);
}

/// ADR-038: a suggestion of a document with more say on a field is not replaced by one with less: the constancia of
/// the SAT says the legal name as it is today, the deed as it was born.
#[test]
fn a_document_with_more_say_keeps_its_suggestion() {
    let (_d, mut c) = conn();
    attach(&mut c, "tax_status_certificate", "constancia-situacion-fiscal.txt");
    attach(&mut c, "articles_of_incorporation", "acta-constitutiva.txt");
    assert_eq!(pending_of(&c, "institution.legal", "institution.legal_name"), Some(json!("CASA DE REPOSO FICTICIA SAN EJEMPLO")));
    assert_eq!(pending_of(&c, "institution.legal", "institution.founded_year"), Some(json!(2015)), "what only the deed says is suggested");
    assert!(authority("institution.legal_name", Some("tax_status_certificate")) > authority("institution.legal_name", Some("articles_of_incorporation")));
    assert_eq!(authority("institution.mission", Some("articles_of_incorporation")), 1);
}

/// ADR-038: the document's own window is written from its file at once (issue, validity, number), with the page;
/// a file that replaces it is read again, but never over what the person wrote by hand.
#[test]
fn the_own_window_is_written_from_its_file_and_respects_what_the_person_wrote() {
    let (_d, mut c) = conn();
    let out = attach_pages(&mut c, "power_of_attorney", "poder.pdf", pages_of("poder-notarial.txt"));
    let AttachOutcome::Attached { filled, .. } = out else { panic!() };
    assert_eq!(filled, vec!["institution.record.issued_on", "institution.record.valid_until", "institution.record.reference"]);
    let form = crate::core::records::form(&c, "power_of_attorney", None).unwrap();
    assert_eq!(form.values["institution.record.issued_on"], json!("2024-01-18"));
    assert_eq!(form.values["institution.record.valid_until"], json!("2027-01-18"));
    assert_eq!(form.values["institution.record.reference"], json!("51300"));
    assert_eq!(form.read["institution.record.valid_until"].page, Some(2));
    // the folio is protected: its origin keeps no line with it
    let source: String = c.query_row("SELECT source_ref FROM core_record_field WHERE field = 'institution.record.reference'", [], |r| r.get(0)).unwrap();
    assert!(!source.contains("quote"), "{source}");

    // the person corrects the validity by hand
    let mut values = form.values.clone();
    values.insert("institution.record.valid_until".into(), json!("2027-02-01"));
    assert!(matches!(crate::core::records::save(&mut c, "power_of_attorney", None, values, None).unwrap(), crate::core::records::RecordOutcome::Saved { .. }));
    // a new poder replaces the file: its issue date and number are read again, the validity the person wrote stays
    let newer: Vec<String> = pages_of("poder-notarial.txt")
        .into_iter()
        .map(|p| p.replace("dieciocho días del mes de enero de dos mil veinticuatro", "cinco días del mes de mayo de dos mil veinticinco").replace("CINCUENTA Y UN MIL TRESCIENTOS", "SESENTA MIL"))
        .collect();
    let AttachOutcome::Attached { filled, .. } = attach_pages(&mut c, "power_of_attorney", "poder-2025.pdf", newer) else { panic!() };
    assert_eq!(filled, vec!["institution.record.issued_on", "institution.record.reference"]);
    let form = crate::core::records::form(&c, "power_of_attorney", None).unwrap();
    assert_eq!(
        (form.values["institution.record.issued_on"].clone(), form.values["institution.record.valid_until"].clone(), form.values["institution.record.reference"].clone()),
        (json!("2025-05-05"), json!("2027-02-01"), json!("60000"))
    );
    assert!(!form.read.contains_key("institution.record.valid_until"), "what the person wrote is not «read from the file»");

    // taking the file off keeps what was read from it
    let id = form.values.get("institution.record.status").map(|_| crate::core::records::storage::by_type(&c, "power_of_attorney").unwrap().unwrap().id).unwrap();
    crate::core::records::detach(&mut c, &id).unwrap();
    assert_eq!(crate::core::records::form(&c, "power_of_attorney", None).unwrap().values["institution.record.issued_on"], json!("2025-05-05"));
}

/// ADR-042 §4: a file whose words say it is another document is not kept where it was put. Nothing is written
/// until the person answers: on the document its words say, on this one after all (read with its rules, and written
/// in the log), or nowhere. Each test waits on its own document: what waits is shared by the whole program.
#[test]
fn a_file_put_on_the_wrong_document_waits_for_the_person() {
    use crate::core::records::placing::{keep_as, take, Answer};
    let (_d, mut c) = conn();
    let kept = |c: &Connection| docs::list_drawer(c, docs::Drawer::Record).unwrap().len();

    // a constancia put on the poder: asked, nothing kept, nothing suggested
    let out = attach_pages(&mut c, "power_of_attorney", "constancia.pdf", pages_of("constancia-situacion-fiscal.txt"));
    let AttachOutcome::Ask { looks_like, file_name, there_has_file } = out else { panic!("{out:?}") };
    assert_eq!((looks_like.as_deref(), file_name.as_str(), there_has_file), (Some("tax_status_certificate"), "constancia.pdf", false));
    assert_eq!(kept(&c), 0);
    assert!(crate::core::records::storage::by_type(&c, "power_of_attorney").unwrap().is_none(), "the poder is not marked as had");
    assert!(suggestions(&c, "institution.legal").is_empty());

    // «Cancelar»: nothing, and nothing waits any more
    let w = take("power_of_attorney").expect("it waits");
    assert!(keep_as(&mut c, w, Answer::Cancel).unwrap().is_none());
    assert!(take("power_of_attorney").is_none());
    assert_eq!(kept(&c), 0);

    // «Guardarlo en Constancia…»: kept on the constancia, read with its rules
    attach_pages(&mut c, "power_of_attorney", "constancia.pdf", pages_of("constancia-situacion-fiscal.txt"));
    let out = keep_as(&mut c, take("power_of_attorney").unwrap(), Answer::There).unwrap().unwrap();
    let AttachOutcome::Attached { suggested, .. } = out else { panic!() };
    assert!(suggested > 0);
    assert!(crate::core::records::storage::by_type(&c, "tax_status_certificate").unwrap().is_some_and(|r| r.document_id.is_some()));
    assert!(crate::core::records::storage::by_type(&c, "power_of_attorney").unwrap().is_none());

    // another constancia on the poder now says the constancia already has a file
    let AttachOutcome::Ask { there_has_file, .. } = attach_pages(&mut c, "power_of_attorney", "otra.pdf", pages_of("constancia-situacion-fiscal.txt")) else { panic!() };
    assert!(there_has_file);
    // «Sí es este documento»: kept on the poder, and the log says the person said so (never the file)
    let out = keep_as(&mut c, take("power_of_attorney").unwrap(), Answer::Here).unwrap().unwrap();
    assert!(matches!(out, AttachOutcome::Attached { readable: true, .. }));
    assert!(crate::core::records::storage::by_type(&c, "power_of_attorney").unwrap().is_some_and(|r| r.document_id.is_some()));
    let details: String = c.query_row("SELECT details_json FROM audit_log WHERE event = 'institution.record_kind_confirmed'", [], |r| r.get(0)).unwrap();
    assert!(details.contains("power_of_attorney") && details.contains("tax_status_certificate") && !details.contains("otra.pdf"), "{details}");

    // a deed is kept on its own document without asking
    assert!(matches!(attach_pages(&mut c, "articles_of_incorporation", "acta.pdf", pages_of("acta-constitutiva.txt")), AttachOutcome::Attached { .. }));
}

/// ADR-042 §4: a file nothing recognizes (a contract on the Junta's registry) is asked about, and kept there only
/// when the person says so; its name raises the confidence when it says the document.
#[test]
fn a_file_nothing_recognizes_is_asked_about_unless_its_name_says_it() {
    use crate::core::records::placing::{keep_as, take, Answer};
    let (_d, mut c) = conn();
    let out = attach_pages(&mut c, "junta_registration", "escaneo.pdf", pages_of("contrato-comodato.txt"));
    assert!(matches!(out, AttachOutcome::Ask { looks_like: None, there_has_file: false, .. }), "{out:?}");
    assert!(matches!(keep_as(&mut c, take("junta_registration").unwrap(), Answer::Here).unwrap(), Some(AttachOutcome::Attached { .. })));
    // the same words, in a file whose name says the document
    assert!(matches!(attach_pages(&mut c, "junta_registration", "Registro JAP 2015.pdf", pages_of("contrato-comodato.txt")), AttachOutcome::Attached { .. }));
}

/// ADR-036: no new document is left without a drawer, whatever way it came in.
#[test]
fn no_new_document_is_left_without_a_drawer() {
    let (_d, mut c) = conn();
    attach(&mut c, "articles_of_incorporation", "acta-constitutiva.txt");
    docs::add_call_document(&c, "bases.pdf", "application/pdf", &["Bases de la convocatoria.".into()], 0, &docs::PageMarks::default()).unwrap();
    docs::add_text_document(&mut c, docs::Drawer::Record, "Texto", "Un texto del expediente.", 0).unwrap();
    let none: i64 = c.query_row("SELECT count(*) FROM document WHERE drawer IS NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(none, 0);
    let calls: i64 = c.query_row("SELECT count(*) FROM document WHERE drawer = 'projects' AND kind = 'call'", [], |r| r.get(0)).unwrap();
    assert_eq!(calls, 1);
}

/// What the Reader understands of a real file, without saving anything nor printing its text: per page, how many
/// letters were read and whether from a picture; the kind; and per datum found, its page and its length. For a real
/// document of the institution: its text never leaves this computer, and here not even the screen.
///   $env:SOCIAI_READER_FILE="D:\...\acta.pdf"; cargo test reader_on_a_file -- --ignored --nocapture > salida.txt
///   (optional: $env:SOCIAI_READER_KIND="articles_of_incorporation", the kind of the document of the record)
#[test]
#[ignore]
fn reader_on_a_file() {
    let path = std::path::PathBuf::from(std::env::var("SOCIAI_READER_FILE").expect("SOCIAI_READER_FILE"));
    let known = std::env::var("SOCIAI_READER_KIND").ok();
    let bytes = std::fs::read(&path).unwrap();
    let started = std::time::Instant::now();
    let read = crate::documents::reading::read_upload(&path.file_name().unwrap().to_string_lossy(), &bytes).unwrap();
    println!("Páginas: {}  leídas de imagen: {}  con dudas: {:?}  ({:.0} s)", read.pages.len(), read.from_picture.len(), read.doubtful, started.elapsed().as_secs_f64());
    for (i, p) in read.pages.iter().enumerate() {
        let letters = p.chars().filter(|c| c.is_alphabetic()).count();
        let folded = crate::storage::fts::fold(p);
        let marks: Vec<&str> = ["por objeto", "objeto", "articulo", "clausula", "denominacion", "escritura", "vigencia"].into_iter().filter(|w| folded.contains(w)).collect();
        println!("  p{:>2}: {:>5} letras  {}  {:?}", i + 1, letters, if read.from_picture.contains(&(i + 1)) { "imagen" } else { "texto " }, marks);
    }
    let pages: Vec<String> = read.pages.iter().map(|p| crate::documents::text::clean_text(p)).collect();
    // what the scanner finds, only by kind and page (never the values), and whether the file is refused as a list
    use crate::scanner::SensitiveScanner;
    let scanner = crate::scanner::RegexScanner::new(Default::default());
    let scanner = if read.from_picture.is_empty() { scanner } else { scanner.reading_pictures() };
    let whole = scanner.scan(&pages.join("\n\n"));
    println!("Escáner: {:?}  bloqueantes: {}  ¿lista de personas?: {}", whole.counts(), whole.count_blocking(), scanner.looks_like_roster(&pages.join("\n\n"), &whole));
    for (i, p) in pages.iter().enumerate() {
        // the lines that read as the header of a list of people: only which of its words are there
        for line in crate::storage::fts::fold(p).lines() {
            let seps = line.matches(['\t', '|', ';', ',']).count() + line.matches("  ").count();
            let keys: Vec<&str> = ["edad", "diagnostico", "curp", "padecimiento", "fecha de nacimiento"].into_iter().filter(|k| line.contains(k)).collect();
            if seps >= 2 && line.contains("nombre") && !keys.is_empty() {
                println!("  p{:>2}: renglón como encabezado de lista: «nombre» + {:?}, {} separadores, {} letras", i + 1, keys, seps, line.chars().count());
            }
        }
        let r = scanner.scan(p);
        if r.count_blocking() > 0 {
            println!("  p{:>2}: {:?}", i + 1, r.findings.iter().filter(|f| f.severity == crate::scanner::Severity::Block).map(|f| f.kind).collect::<Vec<_>>());
        }
    }
    // only when asked: a few words around the first place each of these appears, to see how the document writes it
    //   $env:SOCIAI_READER_SHOW="objeto,denominacion"
    if let Ok(show) = std::env::var("SOCIAI_READER_SHOW") {
        for w in show.split(',').map(str::trim).filter(|w| !w.is_empty()) {
            let hit = pages.iter().enumerate().find_map(|(i, p)| {
                let folded = crate::storage::fts::fold(p);
                folded.find(w).map(|at| (i + 1, folded[..at].chars().count(), p))
            });
            if let Some((page, at, p)) = hit {
                let around: String = p.chars().skip(at.saturating_sub(20)).take(160).collect();
                println!("«{w}» p{page}: …{}…", around.split_whitespace().collect::<Vec<_>>().join(" "));
            }
        }
    }
    // only the labels of a form-like document (the words before each «:»), never what follows them
    //   $env:SOCIAI_READER_LABELS="1"
    if std::env::var("SOCIAI_READER_LABELS").is_ok() {
        for (i, p) in pages.iter().enumerate() {
            let labels: Vec<String> = p
                .split(':')
                .take(40)
                .filter_map(|chunk| {
                    let words: Vec<&str> = chunk.split_whitespace().collect();
                    let tail: Vec<&str> = words.iter().rev().take_while(|w| w.chars().next().is_some_and(|c| c.is_alphabetic())).take(4).copied().collect();
                    (!tail.is_empty()).then(|| tail.into_iter().rev().collect::<Vec<_>>().join(" "))
                })
                .collect();
            println!("  rótulos p{}: {:?}", i + 1, labels);
        }
    }
    println!("Tipo por sus palabras: {:?}", kinds::recognize(&pages));
    let reading = super::read(&pages, known.as_deref());
    println!("Tipo usado: {:?}", reading.kind);
    for f in &reading.found {
        let len = f.value.as_str().map_or(f.value.to_string().len(), |s| s.chars().count());
        println!("  {} p{}  {} letras", f.field, f.page, len);
        // where a long text ends, to see that it stops at the next article or label
        if len > 200 {
            let s = f.value.as_str().unwrap_or_default();
            println!("     …{}", s.chars().skip(len - 60).collect::<String>());
        }
    }
}
