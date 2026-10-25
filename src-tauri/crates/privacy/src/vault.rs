//! The vault (ADR-044): what goes to the AI carries marks, never the value of a personal datum. Each finding of the
//! scanner becomes a mark with its kind and number (`[NOMBRE_1]`, `[CORREO_1]`), the same value the same mark in the
//! whole errand, and the code puts the value back on this computer (`reveal`). The table of values lives only in
//! memory while the errand lasts: nothing of it reaches the disk, the audit log nor `ai_usage`.
//!
//! Health of one person keeps its label of today and is never revealed (ADR-044 §3). Nothing here knows the
//! institution: whoever runs the errand may tell where a value is saved (`know`), and the legend says it by its name.

use crate::normalize::fold_str;
use crate::{FindingKind, ScanReport};
use regex::Regex;
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

/// The word of the mark of each kind; health has none.
fn word(kind: FindingKind) -> Option<&'static str> {
    Some(match kind {
        FindingKind::PersonName => "NOMBRE",
        FindingKind::Email => "CORREO",
        FindingKind::Phone => "TELEFONO",
        FindingKind::RfcPerson | FindingKind::RfcCompany => "RFC",
        FindingKind::Curp => "CURP",
        FindingKind::Clabe => "CLABE",
        FindingKind::Card => "TARJETA",
        FindingKind::Nss => "NSS",
        FindingKind::VoterKey => "CLAVE_ELECTOR",
        FindingKind::HealthContext => return None,
    })
}

/// What a kind is, in the words of the legend.
fn what(word: &str) -> &'static str {
    match word {
        // the scanner does not know a person from a place named after one: the context says it
        "NOMBRE" => "un nombre propio, de una persona o de un lugar (el contexto dice cuál)",
        "CORREO" => "un correo",
        "TELEFONO" => "un teléfono",
        "RFC" => "un RFC",
        "CURP" => "una CURP",
        "CLABE" => "una CLABE",
        "TARJETA" => "un número de tarjeta",
        "NSS" => "un número de seguro social",
        _ => "una clave de elector",
    }
}

/// A mark as the model may write it: `[NOMBRE_1]`, and also `[NOMBRE 1]`, `[Nombre-1]`, `[TELÉFONO_2]` or `NOMBRE_1`.
fn mark_pattern() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        let words = r"NOMBRE|CORREO|TEL[EÉ]FONO|RFC|CURP|CLABE|TARJETA|NSS|CLAVE[ _]ELECTOR";
        Regex::new(&format!(r"(?i)\[\s*({words})[ _-]?(\d{{1,3}})\s*\]|\b({words})_(\d{{1,3}})\b")).unwrap()
    })
}

/// The mark a match of `mark_pattern` names, written as the vault writes it.
fn canonical(c: &regex::Captures) -> String {
    let (w, n) = match (c.get(1), c.get(2)) {
        (Some(w), Some(n)) => (w.as_str(), n.as_str()),
        _ => (c.get(3).unwrap().as_str(), c.get(4).unwrap().as_str()),
    };
    let w = fold_str(w).replace(' ', "_");
    format!("[{w}_{}]", n.trim_start_matches('0'))
}

/// A value as a pattern that also finds it written with or without accents («Ana Maria» for «Ana María»).
fn any_accent(value: &str) -> String {
    value
        .chars()
        .map(|c| match fold_str(&c.to_string()).as_str() {
            "A" => "[aáàäâ]".to_string(),
            "E" => "[eéèëê]".to_string(),
            "I" => "[iíìïî]".to_string(),
            "O" => "[oóòöô]".to_string(),
            "U" => "[uúùüû]".to_string(),
            "N" => "[nñ]".to_string(),
            _ => regex::escape(&c.to_string()),
        })
        .collect()
}

/// How two values are the same: their letters and digits, without capitals nor accents; a phone by its last ten digits.
fn key(word: &str, value: &str) -> String {
    let k: String = fold_str(value).chars().filter(|c| c.is_alphanumeric()).collect();
    if word == "TELEFONO" && k.len() > 10 {
        return k[k.len() - 10..].to_string();
    }
    k
}

struct Entry {
    mark: String,
    word: &'static str,
    value: String,
    key: String,
}

/// The marks of one errand. Shared by every step of it (`&Vault`), so the same value keeps its mark.
#[derive(Default)]
pub struct Vault {
    entries: Mutex<Vec<Entry>>,
    /// Values saved by the program, with where (`«Correo de contacto»`): the legend says when a mark is one of them.
    known: Mutex<Vec<(String, String)>>,
}

impl Vault {
    pub fn new() -> Self {
        Self::default()
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, Vec<Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A value saved by the program and where: the legend says «es el mismo guardado en …» without the value.
    pub fn know(&self, value: &str, place: &str) {
        let k: String = fold_str(value).chars().filter(|c| c.is_alphanumeric()).collect();
        if !k.is_empty() {
            self.known.lock().unwrap_or_else(|e| e.into_inner()).push((k, place.to_string()));
        }
    }

    /// The mark of a value, made the first time it is seen.
    fn mark_of(&self, word: &'static str, value: &str) -> String {
        let key = key(word, value);
        let mut entries = self.entries();
        if let Some(e) = entries.iter().find(|e| e.word == word && e.key == key) {
            return e.mark.clone();
        }
        let n = entries.iter().filter(|e| e.word == word).count() + 1;
        let mark = format!("[{word}_{n}]");
        entries.push(Entry { mark: mark.clone(), word, value: value.trim().to_string(), key });
        mark
    }

    /// The text with each finding of the scanner as its mark, health as its label. A value marked before in this errand
    /// is marked wherever it shows up again, even where the scanner did not see it this time.
    pub fn mark(&self, text: &str, report: &ScanReport) -> String {
        let mut findings: Vec<_> = report.findings.iter().collect();
        findings.sort_by_key(|f| f.span.start);
        let mut out = String::with_capacity(text.len());
        let mut cursor = 0;
        for f in findings {
            if f.span.start < cursor || f.span.end > text.len() {
                continue;
            }
            out.push_str(&text[cursor..f.span.start]);
            let found = &text[f.span.clone()];
            match word(f.kind) {
                // a value of the errand inside a longer finding («Firmó Ana López»: a word before the name taken with
                // it) keeps its mark; anything else found is a new one
                Some(w) => match self.mark_known(found) {
                    known if known != found => out.push_str(&known),
                    _ => out.push_str(&self.mark_of(w, found)),
                },
                None => out.push_str(f.kind.label()),
            }
            cursor = f.span.end;
        }
        out.push_str(&text[cursor..]);
        self.mark_known(&out)
    }

    /// Every value of this errand still written in a text, as its mark (the longest first).
    fn mark_known(&self, text: &str) -> String {
        let entries = self.entries();
        let mut values: Vec<(&str, &str)> = entries.iter().map(|e| (e.value.as_str(), e.mark.as_str())).filter(|(v, _)| v.chars().count() >= 3).collect();
        values.sort_by_key(|(v, _)| std::cmp::Reverse(v.len()));
        let mut out = text.to_string();
        for (value, mark) in values {
            let edge = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
            let pattern = format!(
                "(?i){}{}{}",
                if edge(value.chars().next()) { r"\b" } else { "" },
                any_accent(value),
                if edge(value.chars().last()) { r"\b" } else { "" }
            );
            if let Ok(r) = Regex::new(&pattern) {
                out = r.replace_all(&out, regex::NoExpand(mark)).into_owned();
            }
        }
        out
    }

    /// The text with each mark of this errand as its value, also a mark written a little differently. A mark that this
    /// errand never made stays as it is: the code never guesses a value.
    pub fn reveal(&self, text: &str) -> String {
        let entries = self.entries();
        mark_pattern()
            .replace_all(text, |c: &regex::Captures| {
                let mark = canonical(c);
                entries.iter().find(|e| e.mark == mark).map_or_else(|| c[0].to_string(), |e| e.value.clone())
            })
            .into_owned()
    }

    /// Every text inside a value revealed (the arguments of a tool, a suggestion).
    pub fn reveal_json(&self, v: &Value) -> Value {
        match v {
            Value::String(s) => Value::String(self.reveal(s)),
            Value::Array(a) => Value::Array(a.iter().map(|x| self.reveal_json(x)).collect()),
            Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), self.reveal_json(x))).collect()),
            other => other.clone(),
        }
    }

    /// The marks written in a text that this errand never made: the model made them up.
    pub fn unknown(&self, text: &str) -> Vec<String> {
        let entries = self.entries();
        mark_pattern().captures_iter(text).map(|c| canonical(&c)).filter(|m| !entries.iter().any(|e| &e.mark == m)).collect()
    }

    /// Whether a value (any text inside it) holds a mark this errand never made.
    pub fn has_unknown(&self, v: &Value) -> bool {
        match v {
            Value::String(s) => !self.unknown(s).is_empty(),
            Value::Array(a) => a.iter().any(|x| self.has_unknown(x)),
            Value::Object(o) => o.values().any(|x| self.has_unknown(x)),
            _ => false,
        }
    }

    /// What the AI is told of the marks of this errand, written by the code (ADR-044 §4): what each one is and, when it
    /// is a value saved by the program, where. Never the value. `None` with no marks.
    pub fn legend(&self) -> Option<String> {
        let entries = self.entries();
        if entries.is_empty() {
            return None;
        }
        let known = self.known.lock().unwrap_or_else(|e| e.into_inner());
        let mut s = String::from(
            "Marcas: cada una es un dato que se queda en este equipo. Escribir una marca no es escribir el dato: úsela tal cual donde iría ese dato, en su respuesta o en el valor de cualquier campo, y el programa pone el dato. No la cambie, no la explique y no invente otras.\n",
        );
        for e in entries.iter() {
            let plain: String = fold_str(&e.value).chars().filter(|c| c.is_alphanumeric()).collect();
            let saved: Vec<&str> = known.iter().filter(|(k, _)| *k == plain || (e.word == "TELEFONO" && k.ends_with(&e.key))).map(|(_, p)| p.as_str()).collect();
            s.push_str(&format!("- {}: {}", e.mark, what(e.word)));
            if !saved.is_empty() {
                s.push_str(&format!("; es el mismo que está guardado en {}", saved.join(" y en ")));
            }
            s.push_str(".\n");
        }
        Some(s.trim_end().to_string())
    }

    /// How many marks of each kind this errand made (for the audit log: counts, never values).
    pub fn counts(&self) -> std::collections::BTreeMap<&'static str, usize> {
        let mut m = std::collections::BTreeMap::new();
        for e in self.entries().iter() {
            *m.entry(e.word).or_insert(0) += 1;
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RegexScanner, ScannerConfig, SensitiveScanner};
    use serde_json::json;

    fn marked(v: &Vault, text: &str) -> String {
        let s = RegexScanner::new(ScannerConfig::default());
        v.mark(text, &s.scan(text))
    }

    #[test]
    fn the_same_value_keeps_its_mark_and_two_people_are_two_marks() {
        let v = Vault::new();
        let a = marked(&v, "La directora es Ana María López y la tesorera Rosa Ruiz.");
        assert_eq!(a, "La directora es [NOMBRE_1] y la tesorera [NOMBRE_2].");
        // in the next step of the errand, the same person is the same mark, even written in another way
        let b = marked(&v, "Escríbale a ana maría lópez al correo ana@ejemplo.mx o al 55 1234 5678.");
        assert_eq!(b, "Escríbale a [NOMBRE_1] al correo [CORREO_1] o al [TELEFONO_1].");
        let c = marked(&v, "Su número es +52 (55) 1234-5678.");
        assert!(c.contains("[TELEFONO_1]"), "a phone by its ten digits: {c}");
        // the scanner takes the word before the name with it, and the name keeps its mark; without accents too
        assert_eq!(marked(&v, "Firmó Ana Maria Lopez."), "Firmó [NOMBRE_1].");
        assert_eq!(v.counts(), [("CORREO", 1), ("NOMBRE", 2), ("TELEFONO", 1)].into_iter().collect());
    }

    #[test]
    fn health_of_one_person_keeps_its_label_and_is_never_revealed() {
        let v = Vault::new();
        let t = marked(&v, "La señora Rosa padece diabetes.");
        assert!(t.contains("[DATO DE SALUD OCULTO]") && !t.contains("diabetes") && !t.contains("Rosa"), "{t}");
        assert_eq!(v.reveal(&t), "La señora Rosa [DATO DE SALUD OCULTO] [DATO DE SALUD OCULTO].");
    }

    #[test]
    fn the_code_reveals_the_marks_also_written_a_little_differently() {
        let v = Vault::new();
        marked(&v, "Mi correo es ana@ejemplo.mx y soy Ana María López.");
        assert_eq!(v.reveal("Escriba a [CORREO_1]."), "Escriba a ana@ejemplo.mx.");
        for written in ["[NOMBRE 1]", "[Nombre_1]", "[NOMBRE-1]", "NOMBRE_1", "[ NOMBRE_01 ]"] {
            assert_eq!(v.reveal(&format!("Hola, {written}.")), "Hola, Ana María López.", "{written}");
        }
        assert_eq!(v.reveal_json(&json!({ "q": "¿Quién es [NOMBRE_1]?", "n": 3 })), json!({ "q": "¿Quién es Ana María López?", "n": 3 }));
    }

    #[test]
    fn a_mark_the_errand_never_made_is_never_guessed() {
        let v = Vault::new();
        marked(&v, "Soy Ana María López.");
        assert_eq!(v.reveal("Llame a [NOMBRE_2] o a [TELEFONO_1]."), "Llame a [NOMBRE_2] o a [TELEFONO_1].");
        assert_eq!(v.unknown("[NOMBRE_1], [NOMBRE_2] y [TELEFONO_1]"), vec!["[NOMBRE_2]", "[TELEFONO_1]"]);
        assert!(v.has_unknown(&json!(["[CURP_1]"])) && !v.has_unknown(&json!("[NOMBRE_1] 12")));
        // a word that only looks like a mark is left alone
        assert_eq!(v.reveal("El RFC tiene 13 letras y el NOMBRE va primero."), "El RFC tiene 13 letras y el NOMBRE va primero.");
    }

    #[test]
    fn the_legend_says_what_each_mark_is_and_where_it_is_saved_never_the_value() {
        let v = Vault::new();
        assert_eq!(v.legend(), None);
        v.know("contacto@ejemplo.org", "«Correo de contacto»");
        v.know("33 1234 5678", "«Teléfono»");
        marked(&v, "Soy Ana María López, mi correo es CONTACTO@ejemplo.org y el teléfono +52 33 1234 5678.");
        let legend = v.legend().unwrap();
        assert!(legend.contains("- [NOMBRE_1]: un nombre propio, de una persona o de un lugar (el contexto dice cuál).\n"), "{legend}");
        assert!(legend.contains("- [CORREO_1]: un correo; es el mismo que está guardado en «Correo de contacto»."), "{legend}");
        assert!(legend.contains("- [TELEFONO_1]: un teléfono; es el mismo que está guardado en «Teléfono»."), "{legend}");
        assert!(!legend.contains("López") && !legend.contains("ejemplo") && !legend.contains("1234"), "{legend}");
    }

    #[test]
    fn a_text_without_findings_goes_as_it_is() {
        let v = Vault::new();
        assert_eq!(marked(&v, "Atendemos a 40 personas en la colonia Centro."), "Atendemos a 40 personas en la colonia Centro.");
        assert!(v.counts().is_empty());
    }
}
