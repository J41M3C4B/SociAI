//! The Reader (Lector, IA6 of ADR-034): what a document of the institution is, and the data of the institution it
//! says. Rules first, and in this part only rules: the kind by its words (`kinds`), the data by where each kind writes
//! them (`rules`), the dates as notaries write them (`dates`).
//!
//! - **It reads before the scanner covers.** The rules read the text of the file in memory, before it is saved; what
//!   is kept is still the clean text. That is how a protected datum (the RFC, a folio, the representative) can reach
//!   its field: only when a rule read it, never the AI (decision of the institution, 2026-10-12).
//! - **It proposes, the person decides.** Each datum is a suggestion of its window of «Mi institución», with the
//!   document and page it comes from (`ai_proposal`, origin `document`, agent `reader`). Nothing is saved without
//!   «Usar» and «Guardar».
//! - **Invoices are not read here:** they are uploaded only in Finanzas.

pub mod dates;
pub mod kinds;
pub mod quote;
pub mod rules;

use crate::core::proposals::{self, NewProposal, Origin, ProposeOutcome};
use crate::core::error::ServiceError;
use rules::Found;
use rusqlite::Connection;
use serde_json::json;

/// The agent that signs its proposals.
pub const AGENT: &str = "reader";

/// What the Reader understood of a document: its kind, if a rule recognized it or it was known, and the data found.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub kind: Option<&'static str>,
    pub found: Vec<Found>,
}

/// Reads the text of a file before it is saved. `known`: the kind the person already said (a document of the record).
pub fn read(pages: &[String], known: Option<&str>) -> Reading {
    let kind = known.and_then(|k| kinds::kind(k).map(|k| k.code)).or_else(|| kinds::recognize(pages));
    Reading { kind, found: kind.map(|k| rules::read(k, pages)).unwrap_or_default() }
}

/// The data of a document's own window (ADR-038): written in it as soon as the file is read, when empty.
pub const OWN_PREFIX: &str = "institution.record.";

/// Whether a datum found is one of the document's own window, not of «Mi institución».
pub fn is_own(f: &Found) -> bool {
    f.field.starts_with(OWN_PREFIX)
}

/// Which documents have more say on a datum of «Mi institución», from most to least (ADR-038). The SAT's constancia
/// says the legal name and form as they are today; a reform of the statutes, the purpose as it is today; the deed,
/// how it was born. A kind not listed for a field has the least say.
const AUTHORITY: &[(&str, &[&str])] = &[
    ("institution.legal_name", &["tax_status_certificate", "junta_registration", "bylaws_amendments", "articles_of_incorporation"]),
    ("institution.legal_form", &["tax_status_certificate", "junta_registration", "bylaws_amendments", "articles_of_incorporation"]),
    ("institution.legal_rfc", &["tax_status_certificate", "tax_compliance_opinion"]),
    ("institution.purpose", &["bylaws_amendments", "articles_of_incorporation", "junta_registration"]),
    ("institution.founded_year", &["articles_of_incorporation", "junta_registration"]),
    ("institution.legal_rep_name", &["power_of_attorney"]),
    ("institution.legal_rep_valid_until", &["power_of_attorney"]),
    // what it does today: the report of its year and its rules, before how it was born
    ("institution.services", &["annual_report", "internal_rules", "articles_of_incorporation"]),
];

/// How much say a document of this kind has on a field: more is more; 1 for a kind not listed for it.
pub fn authority(field: &str, kind: Option<&str>) -> i64 {
    let Some(order) = AUTHORITY.iter().find(|(f, _)| *f == field).map(|(_, o)| *o) else { return 1 };
    kind.and_then(|k| order.iter().position(|c| *c == k)).map_or(1, |i| (order.len() - i + 1) as i64)
}

/// The window of «Mi institución» a field is in.
pub(crate) fn form_of(field: &str) -> Option<&'static str> {
    crate::core::institution::forms::forms().iter().find(|f| f.field(field).is_some()).map(|f| f.id)
}

/// Leaves the data of «Mi institución» found in a document as suggestions of their windows, citing the document and
/// the page, and the line it was read from unless the datum is protected. What it suggests follows the one policy of
/// `proposals::propose` (ADR-038): nothing for a field already written, nothing the person rejected, and a suggestion of
/// a document with more say on the field is not replaced by one with less. `kind`: what the document is. Says how
/// many were kept.
pub fn propose(conn: &Connection, document_id: &str, kind: Option<&str>, found: &[Found]) -> Result<usize, ServiceError> {
    let mut kept = 0;
    for f in found.iter().filter(|f| !is_own(f)) {
        let Some(form_id) = form_of(f.field) else { continue };
        let protected = crate::core::institution::forms::field(f.field).is_some_and(|s| s.protected());
        let mut source = json!({ "document_id": document_id, "page": f.page, "rank": authority(f.field, kind), "sha": proposals::fingerprint(&f.value) });
        if !protected {
            source["quote"] = json!(f.line);
        }
        let outcome = proposals::propose(
            conn,
            NewProposal {
                form_id: form_id.to_string(),
                record_id: None,
                field_id: f.field.to_string(),
                value: f.value.clone(),
                origin: Origin::Document,
                source_ref: Some(source.to_string()),
                agent: AGENT,
            },
        )?;
        kept += usize::from(matches!(outcome, ProposeOutcome::Kept { .. }));
    }
    Ok(kept)
}

#[cfg(test)]
mod tests;
