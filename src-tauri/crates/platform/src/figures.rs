//! Figure check: the AI must not invent numbers. Every number written in an AI
//! text has to appear in what the person said (or in the profile). Done by code.

use std::collections::BTreeSet;

const WORDS: &[(&str, u64)] = &[
    ("cero", 0), ("uno", 1), ("una", 1), ("un", 1), ("dos", 2), ("tres", 3), ("cuatro", 4),
    ("cinco", 5), ("seis", 6), ("siete", 7), ("ocho", 8), ("nueve", 9), ("diez", 10),
    ("once", 11), ("doce", 12), ("trece", 13), ("catorce", 14), ("quince", 15),
    ("dieciseis", 16), ("diecisiete", 17), ("dieciocho", 18), ("diecinueve", 19),
    ("veinte", 20), ("veintiuno", 21), ("veintidos", 22), ("veintitres", 23),
    ("veinticuatro", 24), ("veinticinco", 25), ("treinta", 30), ("cuarenta", 40),
    ("cincuenta", 50), ("sesenta", 60), ("setenta", 70), ("ochenta", 80), ("noventa", 90),
    ("cien", 100), ("ciento", 100), ("mil", 1000),
];

fn strip_accents(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'Á' => 'a',
            'é' | 'É' => 'e',
            'í' | 'Í' => 'i',
            'ó' | 'Ó' => 'o',
            'ú' | 'Ú' | 'ü' => 'u',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

/// Numbers written with digits: "150,000" and "150 000" -> 150000, "12.5" -> 12.5.
/// A separator followed by exactly three digits is a thousands separator.
pub fn digit_numbers(text: &str) -> BTreeSet<String> {
    let c: Vec<char> = text.chars().collect();
    let n = c.len();
    let digit = |k: usize| k < n && c[k].is_ascii_digit();
    let mut out = BTreeSet::new();
    let mut i = 0;
    while i < n {
        if !c[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut num = String::new();
        let mut j = i;
        loop {
            while digit(j) {
                num.push(c[j]);
                j += 1;
            }
            let sep = j < n && matches!(c[j], ',' | '.' | ' ');
            if sep && digit(j + 1) && digit(j + 2) && digit(j + 3) && !digit(j + 4) {
                j += 1; // thousands separator
                continue;
            }
            if sep && c[j] != ' ' && digit(j + 1) {
                num.push('.'); // decimal part
                j += 1;
                while digit(j) {
                    num.push(c[j]);
                    j += 1;
                }
            }
            break;
        }
        out.insert(normalize(&num));
        i = j;
    }
    out
}

fn normalize(n: &str) -> String {
    if let Some((int, frac)) = n.split_once('.') {
        let int = int.trim_start_matches('0');
        let frac = frac.trim_end_matches('0');
        let int = if int.is_empty() { "0" } else { int };
        return if frac.is_empty() { int.to_string() } else { format!("{int}.{frac}") };
    }
    let t = n.trim_start_matches('0');
    if t.is_empty() { "0".into() } else { t.to_string() }
}

/// Numbers in a source text: digits plus Spanish number words ("seis" -> 6).
pub fn source_numbers(text: &str) -> BTreeSet<String> {
    let mut out = digit_numbers(text);
    for w in strip_accents(text).split(|c: char| !c.is_alphabetic()) {
        if let Some((_, n)) = WORDS.iter().find(|(word, _)| *word == w) {
            out.insert(n.to_string());
        }
    }
    out
}

/// Numbers (written with digits) in `outputs` that do not appear in any of `sources`.
pub fn unsupported_figures(outputs: &[&str], sources: &[&str]) -> Vec<String> {
    let allowed: BTreeSet<String> = sources.iter().flat_map(|s| source_numbers(s)).collect();
    let mut bad = BTreeSet::new();
    for o in outputs {
        for n in digit_numbers(o) {
            if !allowed.contains(&n) {
                bad.insert(n);
            }
        }
    }
    bad.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reads_digits_with_separators() {
        assert_eq!(digit_numbers("Hubo 3 caídas y 14 personas"), set(&["3", "14"]));
        assert_eq!(digit_numbers("Cuesta $150,000 pesos"), set(&["150000"]));
        assert_eq!(digit_numbers("El 12.5 % y 150 000"), set(&["12.5", "150000"]));
        assert_eq!(digit_numbers("sin cifras"), BTreeSet::new());
        assert_eq!(digit_numbers("3 4 y 5"), set(&["3", "4", "5"])); // not merged
        assert_eq!(digit_numbers("$1,234,567"), set(&["1234567"]));
        assert_eq!(digit_numbers("2026"), set(&["2026"]));
    }

    #[test]
    fn reads_spanish_number_words_in_sources() {
        assert!(source_numbers("Seis van en silla de ruedas").contains("6"));
        assert!(source_numbers("hubo veintidós quejas").contains("22") || source_numbers("hubo veintidos quejas").contains("22"));
        assert!(source_numbers("catorce abuelitos").contains("14"));
    }

    #[test]
    fn detects_invented_numbers() {
        let said = ["Lo usan los 14 abuelitos; 6 van en silla de ruedas.", "Hubo tres caídas, una con fractura."];
        let ok = unsupported_figures(&["14 personas, 6 en silla, 3 caídas (1 con fractura)"], &said);
        assert!(ok.is_empty(), "{ok:?}");
        let bad = unsupported_figures(&["14 personas y 8 caídas, cuesta $90,000"], &said);
        assert_eq!(bad, vec!["8".to_string(), "90000".to_string()]);
    }

    #[test]
    fn profile_numbers_count_as_sources() {
        assert!(unsupported_figures(&["25 camas"], &["Capacidad total: 25"]).is_empty());
    }
}
