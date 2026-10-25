//! The local detector and the vault of SociAI (ADR-004): detection and redaction of sensitive personal data, 100 %
//! local, no network, and the marks that stand for a value while it goes to a model. The detector finds data; it
//! never decides what goes where, which is the policy of the platform. The app and the platform both use it.
//!
//! The scanner never stores or returns the value it found: a `Finding` only has
//! the kind, severity, position and confidence.

mod checks;
mod public;
mod names;
pub mod normalize;
pub mod vault;

use normalize::Normalized;
pub use public::PublicDocScanner;
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::OnceLock;

/// Above this many blocking findings a file is treated as a list of people.
const ROSTER_BLOCK_LIMIT: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    Curp,
    RfcPerson,
    RfcCompany,
    VoterKey,
    Phone,
    Email,
    Clabe,
    Card,
    Nss,
    PersonName,
    HealthContext,
}

impl FindingKind {
    /// Text that replaces the value when redacting.
    pub fn label(self) -> &'static str {
        match self {
            FindingKind::Curp => "[CURP OCULTA]",
            FindingKind::RfcPerson | FindingKind::RfcCompany => "[RFC OCULTO]",
            FindingKind::VoterKey => "[CLAVE DE ELECTOR OCULTA]",
            FindingKind::Phone => "[TELÉFONO OCULTO]",
            FindingKind::Email => "[CORREO OCULTO]",
            FindingKind::Clabe => "[CLABE OCULTA]",
            FindingKind::Card => "[TARJETA OCULTA]",
            FindingKind::Nss => "[NSS OCULTO]",
            FindingKind::PersonName => "[NOMBRE OCULTO]",
            FindingKind::HealthContext => "[DATO DE SALUD OCULTO]",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            FindingKind::Curp => "curp",
            FindingKind::RfcPerson => "rfc_person",
            FindingKind::RfcCompany => "rfc_company",
            FindingKind::VoterKey => "voter_key",
            FindingKind::Phone => "phone",
            FindingKind::Email => "email",
            FindingKind::Clabe => "clabe",
            FindingKind::Card => "card",
            FindingKind::Nss => "nss",
            FindingKind::PersonName => "person_name",
            FindingKind::HealthContext => "health_context",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Cannot be ignored: it is redacted or the text is cancelled.
    Block,
    /// The user may confirm it is not personal data.
    Warn,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub kind: FindingKind,
    pub severity: Severity,
    /// Byte range in the scanned text.
    pub span: Range<usize>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ScanReport {
    pub findings: Vec<Finding>,
}

impl ScanReport {
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    pub fn has_blocking(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Block)
    }

    pub fn count_blocking(&self) -> usize {
        self.findings.iter().filter(|f| f.severity == Severity::Block).count()
    }

    /// Counts per kind, safe to write in the audit log (never the content).
    pub fn counts(&self) -> BTreeMap<&'static str, usize> {
        let mut m = BTreeMap::new();
        for f in &self.findings {
            *m.entry(f.kind.key()).or_insert(0) += 1;
        }
        m
    }
}

/// Institutional data the scanner must not flag.
#[derive(Debug, Clone, Default)]
pub struct ScannerConfig {
    pub institutional_phones: Vec<String>,
    pub institutional_emails: Vec<String>,
    /// Names of the institution, places, foundations and public representatives.
    pub safe_phrases: Vec<String>,
    /// The CLABE of each bank account of the institution (ADR-033 §3): a CLABE in a text is let through only when it
    /// is exactly one of these.
    pub institutional_clabes: Vec<String>,
    /// What the person said is not personal in this errand («No es dato personal»): a warning on exactly one of these
    /// texts is not raised again before the AI (ADR-041). A blocking finding (a CURP, the RFC of a person) never is.
    pub cleared: Vec<String>,
}

pub trait SensitiveScanner: Send + Sync {
    fn scan(&self, text: &str) -> ScanReport;
    fn redact(&self, text: &str, report: &ScanReport) -> String;
}

struct Patterns {
    curp: Regex,
    rfc_person: Regex,
    rfc_company: Regex,
    voter: Regex,
    phone: Regex,
    email: Regex,
    clabe: Regex,
    card: Regex,
    nss: Regex,
    /// An address ending in a known domain, glued to the word before it: an email whose «@» was misread.
    misread_email: Regex,
    /// A CURP and an RFC of a person read from a picture, with letters where digits go («O» for «0»).
    misread_curp: Regex,
    misread_rfc_person: Regex,
}

/// What a picture puts in place of a digit: O→0, I and L→1, Z→2, S→5, G→6, B→8 (the text is in capitals here).
fn as_digit(c: char) -> char {
    match c {
        'O' => '0',
        'I' | 'L' => '1',
        'Z' => '2',
        'S' => '5',
        'G' => '6',
        'B' => '8',
        _ => c,
    }
}

/// An id read from a picture with the places that hold digits fixed, when it holds enough true digits there to be one
/// (four of the six of its date): `None` otherwise, or when nothing needed fixing.
fn with_digits_fixed(id: &str, places: &[std::ops::Range<usize>]) -> Option<String> {
    let mut chars: Vec<char> = id.chars().collect();
    let date = places.first()?;
    if chars.get(date.clone())?.iter().filter(|c| c.is_ascii_digit()).count() < 4 {
        return None;
    }
    let mut changed = false;
    for range in places {
        for c in chars.get_mut(range.clone())? {
            let d = as_digit(*c);
            changed |= d != *c;
            *c = d;
        }
    }
    changed.then(|| chars.into_iter().collect())
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| {
        let r = |s: &str| Regex::new(s).unwrap();
        Patterns {
            curp: r(r"\b[A-Z][AEIOUX][A-Z]{2}\d{2}(?:0[1-9]|1[0-2])(?:0[1-9]|[12]\d|3[01])[HMX](?:AS|BC|BS|CC|CL|CM|CS|CH|DF|DG|GT|GR|HG|JC|MC|MN|MS|NT|NL|OC|PL|QT|QR|SP|SL|SR|TC|TS|TL|VZ|YN|ZS|NE)[B-DF-HJ-NP-TV-Z]{3}[A-Z\d]\d\b"),
            rfc_person: r(r"\b[A-Z&]{4}\d{6}[A-Z\d]{3}\b"),
            rfc_company: r(r"\b[A-Z&]{3}\d{6}[A-Z\d]{3}\b"),
            voter: r(r"\b[A-Z]{6}\d{8}[HMX]\d{3}\b"),
            phone: r(r"(?:\+?52[ -]?)?(?:\(\d{2,3}\)|\d{2,3})[ -]?\d{3,4}[ -]?\d{4}"),
            email: r(r"[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}"),
            clabe: r(r"\b\d{18}\b"),
            card: r(r"\b\d(?:[ -]?\d){12,18}\b"),
            nss: r(r"\b\d{11}\b"),
            misread_email: r(r"[A-Z0-9][A-Z0-9._%+()-]*\.(?:COM|ORG|NET|MX|ES|GOB|EDU|INFO|IO|CO|US)(?:\.[A-Z]{2})?\b"),
            misread_curp: r(r"\b[A-Z][AEIOUX][A-Z]{2}[0-9OILZSGB]{6}[HMX][A-Z]{2}[B-DF-HJ-NP-TV-Z]{3}[A-Z0-9][0-9OILZSGB]\b"),
            misread_rfc_person: r(r"\b[A-Z&]{4}[0-9OILZSGB]{6}[A-Z0-9]{3}\b"),
        }
    })
}

fn digits(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

pub struct RegexScanner {
    cfg: ScannerConfig,
    phones: Vec<String>,
    emails: Vec<String>,
    /// Each account of the institution, with or without spaces or dashes between its digits.
    own_accounts: Vec<Regex>,
    /// The text was read from a picture (OCR, ADR-035): an email with its «@» misread is still looked for.
    pictures: bool,
}

impl RegexScanner {
    pub fn new(cfg: ScannerConfig) -> Self {
        let phones = cfg
            .institutional_phones
            .iter()
            .map(|p| {
                let d = digits(p);
                d[d.len().saturating_sub(10)..].to_string()
            })
            .collect();
        let emails = cfg
            .institutional_emails
            .iter()
            .map(|e| e.trim().to_uppercase())
            .collect();
        let own_accounts = cfg
            .institutional_clabes
            .iter()
            .map(|c| digits(c))
            .filter(|d| d.len() == 18)
            .filter_map(|d| Regex::new(&format!(r"\b{}\b", d.chars().map(String::from).collect::<Vec<_>>().join("[ -]?"))).ok())
            .collect();
        RegexScanner { cfg, phones, emails, own_accounts, pictures: false }
    }

    /// For a text read from a picture (OCR, ADR-035): Tesseract may read the «@» of an email as «G», «(» or «©»
    /// («infoGorlandojuan.es»), which the email pattern no longer sees. Then an address ending in a known domain,
    /// glued to the word before it, is taken as an email (a warning: the person may say it is not personal) when its
    /// line speaks of mail or when it has a sign in place of the «@». A web address (`www.`, `http`) is not.
    pub fn reading_pictures(mut self) -> Self {
        self.pictures = true;
        self
    }

    /// The emails whose «@» was misread, as ranges of the normalized text.
    fn misread_emails(&self, t: &str) -> Vec<Range<usize>> {
        patterns()
            .misread_email
            .find_iter(t)
            .filter(|m| {
                let token = m.as_str();
                let line = &t[t[..m.start()].rfind('\n').map_or(0, |i| i + 1)..m.start()];
                let web = token.starts_with("WWW.") || line.ends_with("//") || line.ends_with("WWW.");
                let speaks_of_mail = ["MAIL", "CORREO"].iter().any(|w| line.contains(w));
                let sign = token.contains(['(', ')']);
                // the address of the institution, misread too, is still its own
                let own = self.emails.iter().any(|e| e.split_once('@').is_some_and(|(local, domain)| token.starts_with(local) && token.ends_with(domain)));
                !web && !own && (speaks_of_mail || sign)
            })
            .map(|m| m.range())
            .collect()
    }

    /// True when the text looks like a list of people (a record file) and must be rejected whole.
    pub fn looks_like_roster(&self, text: &str, report: &ScanReport) -> bool {
        if report.count_blocking() > ROSTER_BLOCK_LIMIT {
            return true;
        }
        let norm = Normalized::new(text);
        norm.text.lines().any(|line| {
            let seps = line.matches(['\t', '|', ';', ',']).count() + line.matches("  ").count();
            seps >= 2
                && line.contains("NOMBRE")
                && ["EDAD", "DIAGNOSTICO", "CURP", "PADECIMIENTO", "FECHA DE NACIMIENTO"]
                    .iter()
                    .any(|k| line.contains(k))
        })
    }
}

impl SensitiveScanner for RegexScanner {
    fn scan(&self, text: &str) -> ScanReport {
        let norm = Normalized::new(text);
        let t = norm.text.as_str();
        let p = patterns();
        let mut found: Vec<Finding> = Vec::new();
        let mut push = |kind, severity, r: Range<usize>, confidence: f32| {
            found.push(Finding { kind, severity, span: norm.orig_span(r), confidence });
        };
        // `\b` is not enough for ids that sit next to hyphens or slashes (folios, dates).
        let clean_edges = |r: &Range<usize>| {
            let before = t[..r.start].chars().next_back();
            let after = t[r.end..].chars().next();
            let bad = |c: Option<char>| c.map_or(false, |c| c.is_alphanumeric() || c == '-' || c == '_' || c == '/');
            !bad(before) && !bad(after)
        };

        for m in p.curp.find_iter(t) {
            if checks::curp_ok(m.as_str()) {
                push(FindingKind::Curp, Severity::Block, m.range(), 1.0);
            } else {
                // Right shape but wrong check digit: probably a typo, still flagged.
                push(FindingKind::Curp, Severity::Warn, m.range(), 0.6);
            }
        }
        for m in p.rfc_person.find_iter(t) {
            if checks::yymmdd_ok(&m.as_str()[4..10]) {
                push(FindingKind::RfcPerson, Severity::Block, m.range(), 0.9);
            }
        }
        for m in p.rfc_company.find_iter(t) {
            if checks::yymmdd_ok(&m.as_str()[3..9]) {
                push(FindingKind::RfcCompany, Severity::Warn, m.range(), 0.8);
            }
        }
        for m in p.voter.find_iter(t) {
            push(FindingKind::VoterKey, Severity::Block, m.range(), 0.9);
        }
        for m in p.clabe.find_iter(t) {
            if checks::clabe_ok(m.as_str()) {
                push(FindingKind::Clabe, Severity::Block, m.range(), 1.0);
            }
        }
        for m in p.card.find_iter(t) {
            let d = digits(m.as_str());
            if (13..=19).contains(&d.len()) && checks::luhn_ok(&d) {
                push(FindingKind::Card, Severity::Block, m.range(), 0.9);
            }
        }
        for m in p.nss.find_iter(t) {
            if checks::luhn_ok(m.as_str()) {
                push(FindingKind::Nss, Severity::Block, m.range(), 0.7);
            }
        }
        for m in p.phone.find_iter(t) {
            let mut d = digits(m.as_str());
            if d.len() == 12 && d.starts_with("52") {
                d.drain(..2);
            }
            if d.len() == 10 && clean_edges(&m.range()) && !self.phones.contains(&d) {
                push(FindingKind::Phone, Severity::Warn, m.range(), 0.8);
            }
        }
        for m in p.email.find_iter(t) {
            if !self.emails.iter().any(|e| e == m.as_str()) {
                push(FindingKind::Email, Severity::Warn, m.range(), 1.0);
            }
        }
        if self.pictures {
            for r in self.misread_emails(t) {
                push(FindingKind::Email, Severity::Warn, r, 0.5);
            }
            // a CURP or an RFC of a person read with a letter where a digit goes: fixed, it must still be one (the
            // check digit of a CURP, a real date in an RFC) to be covered
            for m in p.misread_curp.find_iter(t) {
                if let Some(fixed) = with_digits_fixed(m.as_str(), &[4..10, 17..18]) {
                    if p.curp.is_match(&fixed) && checks::curp_ok(&fixed) {
                        push(FindingKind::Curp, Severity::Block, m.range(), 0.8);
                    }
                }
            }
            for m in p.misread_rfc_person.find_iter(t) {
                if let Some(fixed) = with_digits_fixed(m.as_str(), &[4..10]) {
                    if checks::yymmdd_ok(&fixed[4..10]) {
                        push(FindingKind::RfcPerson, Severity::Block, m.range(), 0.7);
                    }
                }
            }
        }

        // an account of the institution is its own, not a person's: nothing found inside exactly one of them counts
        let own: Vec<Range<usize>> = self.own_accounts.iter().flat_map(|r| r.find_iter(t).map(|m| norm.orig_span(m.range())).collect::<Vec<_>>()).collect();
        found.retain(|f| !own.iter().any(|o| o.start <= f.span.start && f.span.end <= o.end));

        let name_findings = names::find_names(text, &self.cfg);
        let health = names::find_health(&norm, &name_findings);
        found.extend(name_findings);
        found.extend(health);

        // what the person cleared in this errand: a warning on the same text, and only a warning
        if !self.cfg.cleared.is_empty() {
            let cleared: Vec<String> = self.cfg.cleared.iter().map(|c| normalize::fold_str(c.trim())).collect();
            found.retain(|f| f.severity != Severity::Warn || !text.get(f.span.clone()).is_some_and(|s| cleared.contains(&normalize::fold_str(s.trim()))));
        }

        ScanReport { findings: resolve_overlaps(found) }
    }

    fn redact(&self, text: &str, report: &ScanReport) -> String {
        let mut spans: Vec<&Finding> = report.findings.iter().collect();
        spans.sort_by_key(|f| f.span.start);
        let mut out = String::with_capacity(text.len());
        let mut cursor = 0;
        for f in spans {
            if f.span.start < cursor || f.span.end > text.len() {
                continue; // overlapping or out of range: already covered
            }
            out.push_str(&text[cursor..f.span.start]);
            out.push_str(f.kind.label());
            cursor = f.span.end;
        }
        out.push_str(&text[cursor..]);
        out
    }
}

/// Keeps the strongest finding where several overlap (Block first, then the longest).
fn resolve_overlaps(mut found: Vec<Finding>) -> Vec<Finding> {
    found.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then((b.span.end - b.span.start).cmp(&(a.span.end - a.span.start)))
            .then(a.kind.cmp(&b.kind))
    });
    let mut kept: Vec<Finding> = Vec::new();
    for f in found {
        if !kept.iter().any(|k| f.span.start < k.span.end && k.span.start < f.span.end) {
            kept.push(f);
        }
    }
    kept.sort_by_key(|f| f.span.start);
    kept
}

#[cfg(test)]
mod tests;
