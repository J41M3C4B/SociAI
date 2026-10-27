//! Heuristics for person names and individual health context (version 1, no ML).

use super::normalize::{fold_str, Normalized};
use super::{Finding, FindingKind, ScannerConfig, Severity};
use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

fn load(raw: &str) -> HashSet<String> {
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(fold_str)
        .collect()
}

fn given_names() -> &'static HashSet<String> {
    static S: OnceLock<HashSet<String>> = OnceLock::new();
    S.get_or_init(|| load(include_str!("data/given_names.txt")))
}

fn surnames() -> &'static HashSet<String> {
    static S: OnceLock<HashSet<String>> = OnceLock::new();
    S.get_or_init(|| load(include_str!("data/surnames.txt")))
}

/// Words right before a name that make it very likely to be a person.
const TRIGGERS: &[&str] = &[
    "NINA", "NINO", "SENORA", "SENOR", "RESIDENTE", "DONA", "DON", "PACIENTE", "MAMA", "PAPA",
    "ABUELITA", "ABUELITO", "SOR",
];
/// Titles that are not part of the name itself.
const TITLES: &[&str] = &["DONA", "DON", "SENORA", "SENOR", "SOR"];
/// Saints in place names ("Casa Hogar San José") are not people.
const SAINTS: &[&str] = &["SAN", "SANTA", "SANTO"];
/// Words before a capitalised sequence that show it is a place or organisation.
const PLACE_WORDS: &[&str] = &[
    "HOGAR", "ASILO", "FUNDACION", "COLONIA", "CALLE", "AVENIDA", "AV", "PARROQUIA", "COLEGIO",
    "ESCUELA", "HOSPITAL", "INSTITUTO", "ORGANIZACION", "ASOCIACION", "CASA", "CENTRO", "CLINICA",
    "CAPILLA", "TEMPLO", "PLAZA", "BARRIO", "MUNICIPIO", "ESTADO", "CONGREGACION", "ORDEN",
    // the municipalities of Mexico City ("alcaldía Benito Juárez") are named after people
    "ALCALDIA", "DELEGACION", "DEMARCACION",
];
const CONNECTORS: &[&str] = &["de", "del", "la", "las", "los", "y"];

struct Tok {
    start: usize,
    end: usize,
    cap: bool,
    norm: String,
    lower: String,
}

fn tokens(text: &str) -> Vec<Tok> {
    static W: OnceLock<Regex> = OnceLock::new();
    let re = W.get_or_init(|| Regex::new(r"\p{L}+").unwrap());
    re.find_iter(text)
        .map(|m| {
            let w = m.as_str();
            let mut ch = w.chars();
            let first = ch.next().unwrap();
            let cap = first.is_uppercase() && w.chars().count() > 1 && ch.any(|c| c.is_lowercase());
            Tok {
                start: m.start(),
                end: m.end(),
                cap,
                norm: fold_str(w),
                lower: w.to_lowercase(),
            }
        })
        .collect()
}

pub fn find_names(text: &str, cfg: &ScannerConfig) -> Vec<Finding> {
    let toks = tokens(text);
    let safe: Vec<String> = cfg.safe_phrases.iter().map(|p| fold_str(p)).collect();
    let gap_ok = |a: &Tok, b: &Tok| &text[a.end..b.start] == " ";
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if !toks[i].cap {
            i += 1;
            continue;
        }
        // Build a run of capitalised words (allowing "de", "del"... between them).
        let mut idx = vec![i];
        let mut caps = 1;
        let mut k = i + 1;
        while k < toks.len() && caps < 4 {
            if toks[k].cap && gap_ok(&toks[k - 1], &toks[k]) {
                idx.push(k);
                caps += 1;
                k += 1;
            } else if CONNECTORS.contains(&toks[k].lower.as_str())
                && !toks[k].cap
                && k + 1 < toks.len()
                && toks[k + 1].cap
                && gap_ok(&toks[k - 1], &toks[k])
                && gap_ok(&toks[k], &toks[k + 1])
            {
                idx.push(k);
                idx.push(k + 1);
                caps += 1;
                k += 2;
            } else {
                break;
            }
        }
        let next_i = k.max(i + 1);

        // A leading title ("Doña", "Don") is a trigger, not part of the name.
        let mut trigger = i > 0 && gap_ok(&toks[i - 1], &toks[i]) && TRIGGERS.contains(&toks[i - 1].norm.as_str());
        let mut name_idx: Vec<usize> = idx.clone();
        while let Some(&f) = name_idx.first() {
            if TITLES.contains(&toks[f].norm.as_str()) {
                trigger = true;
                name_idx.remove(0);
            } else {
                break;
            }
        }
        // Drop saints at the start ("San José" is a place).
        let words: Vec<usize> = name_idx
            .iter()
            .copied()
            .filter(|&t| toks[t].cap && !SAINTS.contains(&toks[t].norm.as_str()))
            .collect();
        if words.is_empty() {
            i = next_i;
            continue;
        }
        let looks_like_place = (1..=2).any(|d| {
            i.checked_sub(d)
                .map_or(false, |p| PLACE_WORDS.contains(&toks[p].norm.as_str()))
        });
        let has_given = |skip: Option<usize>| {
            words.iter().enumerate().any(|(n, &t)| Some(n) != skip && given_names().contains(&toks[t].norm))
        };
        let pair = words.iter().enumerate().any(|(n, &t)| {
            surnames().contains(&toks[t].norm) && has_given(Some(n))
        });
        let any_known = words
            .iter()
            .any(|&t| given_names().contains(&toks[t].norm) || surnames().contains(&toks[t].norm));
        let qualifies = pair || (trigger && any_known);

        let first = *name_idx.iter().find(|&&t| toks[t].cap && !SAINTS.contains(&toks[t].norm.as_str())).unwrap();
        let last = *words.last().unwrap();
        let span = toks[first].start..toks[last].end;
        let folded = fold_str(&text[span.clone()]);
        let is_safe = safe.iter().any(|s| !s.is_empty() && (s.contains(&folded) || folded.contains(s.as_str())));

        if qualifies && !looks_like_place && !is_safe {
            out.push(Finding {
                kind: FindingKind::PersonName,
                severity: Severity::Warn,
                span,
                confidence: if trigger { 0.9 } else { 0.6 },
            });
        }
        i = next_i;
    }
    out
}

/// Health words flagged only when the sentence is about one individual
/// (a detected name or a singular subject), never for aggregated phrases.
pub fn find_health(norm: &Normalized, names: &[Finding]) -> Vec<Finding> {
    static KW: OnceLock<Regex> = OnceLock::new();
    static SUBJECT: OnceLock<Regex> = OnceLock::new();
    let kw = KW.get_or_init(|| {
        Regex::new(
            r"\b(?:DIAGNOSTIC\w*|PADECE\w*|MEDICAMENT\w*|DOSIS|ALZHEIMER|DEMENCIA|DIABET\w*|HIPERTENSI\w*|VIH|EPILEPSIA|DISCAPACIDAD|PSIQUIATRIC\w*|EXPEDIENTE|ALERGIA\w*|TRATAMIENTO)\b",
        )
        .unwrap()
    });
    let subject = SUBJECT.get_or_init(|| {
        Regex::new(
            r"\b(?:(?:LA|EL|UNA|UN) (?:SENORA|SENOR|NINA|NINO|ABUELITA|ABUELITO|RESIDENTE|PACIENTE|JOVEN|ADULTA|ADULTO|ANCIANA|ANCIANO)|UNA DE LAS|UNO DE LOS|SU (?:MAMA|PAPA|HIJO|HIJA|ABUELA|ABUELO)|CAMA \d+|HABITACION \d+|ELLA)\b",
        )
        .unwrap()
    });

    let text = &norm.text;
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i <= bytes.len() {
        let at_end = i == bytes.len();
        if at_end || matches!(bytes[i], b'.' | b'!' | b'?' | b'\n' | b';') {
            if i > start {
                let sent = &text[start..i];
                let hits: Vec<_> = kw.find_iter(sent).collect();
                if !hits.is_empty() {
                    let sent_orig = norm.orig_span(start..i);
                    let has_name = names
                        .iter()
                        .any(|n| n.span.start >= sent_orig.start && n.span.end <= sent_orig.end);
                    if has_name || subject.is_match(sent) {
                        for h in hits {
                            out.push(Finding {
                                kind: FindingKind::HealthContext,
                                severity: Severity::Warn,
                                span: norm.orig_span(start + h.start()..start + h.end()),
                                confidence: if has_name { 0.9 } else { 0.7 },
                            });
                        }
                    }
                }
            }
            start = i + 1;
        }
        i += 1;
    }
    out
}
