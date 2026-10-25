//! The Mexican identifiers of a person (CURP, RFC, NSS) and of a bank account (CLABE): their shape and their check
//! digit, checked by code. And how they are shown covered on screen.

fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace() && *c != '-').collect::<String>().to_uppercase()
}

/// The value of each character for the CURP check digit (RENAPO).
const CURP_CHARS: &str = "0123456789ABCDEFGHIJKLMNÑOPQRSTUVWXYZ";
/// States of birth in a CURP (`NE`: born abroad).
const CURP_STATES: &[&str] = &[
    "AS", "BC", "BS", "CC", "CL", "CM", "CS", "CH", "DF", "DG", "GT", "GR", "HG", "JC", "MC", "MN", "MS", "NT", "NL", "OC", "PL", "QT",
    "QR", "SP", "SL", "SR", "TC", "TS", "TL", "VZ", "YN", "ZS", "NE",
];

/// A CURP written as it should be stored: no spaces, capitals.
pub fn normalize(s: &str) -> String {
    clean(s)
}

pub fn curp_is_valid(s: &str) -> bool {
    let c: Vec<char> = clean(s).chars().collect();
    if c.len() != 18 {
        return false;
    }
    let letter = |x: char| x.is_ascii_uppercase() || x == 'Ñ';
    let shape = c[..4].iter().all(|&x| letter(x))
        && c[4..10].iter().all(char::is_ascii_digit)
        && matches!(c[10], 'H' | 'M' | 'X')
        && CURP_STATES.contains(&c[11..13].iter().collect::<String>().as_str())
        && c[13..16].iter().all(|&x| letter(x))
        && (c[16].is_ascii_digit() || c[16].is_ascii_uppercase())
        && c[17].is_ascii_digit();
    if !shape || !date_ok(&c[4..10]) {
        return false;
    }
    let sum: u32 = c[..17].iter().enumerate().map(|(i, ch)| CURP_CHARS.chars().position(|x| x == *ch).unwrap_or(0) as u32 * (18 - i as u32)).sum();
    (10 - sum % 10) % 10 == c[17].to_digit(10).unwrap_or(99)
}

/// `YYMMDD` with a month and a day that exist.
fn date_ok(d: &[char]) -> bool {
    let n = |a: usize| d[a].to_digit(10).unwrap_or(0) * 10 + d[a + 1].to_digit(10).unwrap_or(0);
    (1..=12).contains(&n(2)) && (1..=31).contains(&n(4))
}

/// The birth date a CURP says, as `MMDD` and the two digits of the year (the century is not in it for sure).
pub fn curp_birth(s: &str) -> Option<(u32, u32, u32)> {
    let c: Vec<char> = clean(s).chars().collect();
    if c.len() < 10 {
        return None;
    }
    let n = |a: usize| Some(c[a].to_digit(10)? * 10 + c[a + 1].to_digit(10)?);
    Some((n(4)?, n(6)?, n(8)?))
}

/// The value of each character for the RFC check digit (SAT).
const RFC_CHARS: &str = "0123456789ABCDEFGHIJKLMN&OPQRSTUVWXYZ Ñ";

/// The RFC of a person: four letters, six digits of the date and a three-character key, the last one the check digit.
pub fn rfc_person_is_valid(s: &str) -> bool {
    let c: Vec<char> = clean(s).chars().collect();
    if c.len() != 13 {
        return false;
    }
    let shape = c[..4].iter().all(|&x| x.is_ascii_uppercase() || x == 'Ñ' || x == '&')
        && c[4..10].iter().all(char::is_ascii_digit)
        && c[10..].iter().all(|x| x.is_ascii_alphanumeric());
    if !shape || !date_ok(&c[4..10]) {
        return false;
    }
    let sum: u32 = c[..12].iter().enumerate().map(|(i, ch)| RFC_CHARS.chars().position(|x| x == *ch).unwrap_or(0) as u32 * (13 - i as u32)).sum();
    let expected = match 11 - sum % 11 {
        11 => '0',
        10 => 'A',
        n => char::from_digit(n, 10).unwrap_or('?'),
    };
    c[12] == expected
}

/// The NSS of the IMSS: eleven digits, the last one by the Luhn rule over the first ten.
pub fn nss_is_valid(s: &str) -> bool {
    let d: Vec<u32> = clean(s).chars().filter_map(|c| c.to_digit(10)).collect();
    if d.len() != 11 || clean(s).len() != 11 {
        return false;
    }
    let sum: u32 = d[..10].iter().enumerate().map(|(i, &x)| if i % 2 == 1 { let v = x * 2; v / 10 + v % 10 } else { x }).sum();
    (10 - sum % 10) % 10 == d[10]
}

/// The CLABE of a bank account: eighteen digits and their check digit. The kernel's, which the dictionary uses.
pub use kernel::clabe_is_valid;

/// The bank of a CLABE, by its first three digits (the most common ones).
pub fn bank_of_clabe(s: &str) -> Option<&'static str> {
    let c = clean(s);
    Some(match c.get(..3)? {
        "002" => "Banamex",
        "012" => "BBVA",
        "014" => "Santander",
        "021" => "HSBC",
        "030" => "BanBajío",
        "036" => "Inbursa",
        "044" => "Scotiabank",
        "058" => "Banregio",
        "062" => "Afirme",
        "072" => "Banorte",
        "127" => "Banco Azteca",
        "137" => "BanCoppel",
        "166" => "Banco del Bienestar",
        "638" => "Nu México",
        "646" => "STP",
        "722" => "Mercado Pago",
        _ => return None,
    })
}

/// How a covered identifier looks: the first characters that do not identify alone and the last ones.
pub fn mask(s: &str, head: usize, tail: usize) -> String {
    let c: Vec<char> = clean(s).chars().collect();
    if c.len() <= head + tail {
        return "•".repeat(c.len());
    }
    let mut out: String = c[..head].iter().collect();
    out.push_str(&"•".repeat(c.len() - head - tail));
    out.extend(&c[c.len() - tail..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curp_shape_and_check_digit() {
        assert!(curp_is_valid("HEGG560427MVZRRL04"), "the RENAPO example");
        assert!(curp_is_valid("hegg 560427 mvzrrl04"), "spaces and lower case are fine");
        assert!(!curp_is_valid("HEGG560427MVZRRL05"), "wrong check digit");
        assert!(!curp_is_valid("HEGG561327MVZRRL04"), "month 13");
        assert!(!curp_is_valid("HEGG560427MXXRRL04"), "no such state");
        assert!(!curp_is_valid("HEGG560427MVZRRL0"), "short");
        assert_eq!(curp_birth("HEGG560427MVZRRL04"), Some((56, 4, 27)));
    }

    #[test]
    fn rfc_shape_and_check_digit() {
        assert!(rfc_person_is_valid("GODE561231GR8"), "the SAT example");
        assert!(rfc_person_is_valid("gode-561231-gr8"));
        assert!(!rfc_person_is_valid("GODE561231GR9"));
        assert!(!rfc_person_is_valid("AHE200101AB1"), "an institution's RFC has 12 characters");
    }

    #[test]
    fn nss_and_clabe_check_digits() {
        assert!(nss_is_valid("12345678903"));
        assert!(!nss_is_valid("12345678904"));
        assert!(!nss_is_valid("1234567890"));
        assert!(clabe_is_valid("002010077777777771"), "the Banamex example");
        assert!(clabe_is_valid("032 180 00011835971 9"));
        assert!(!clabe_is_valid("002010077777777772"));
        assert!(!clabe_is_valid("00201007777777777A"));
        assert_eq!(bank_of_clabe("002010077777777771"), Some("Banamex"));
        assert_eq!(bank_of_clabe("999010077777777771"), None);
    }

    #[test]
    fn covered_identifiers_keep_only_what_does_not_identify() {
        assert_eq!(mask("HEGG560427MVZRRL04", 4, 2), "HEGG••••••••••••04");
        assert_eq!(mask("002010077777777771", 0, 4), "••••••••••••••7771");
        assert_eq!(mask("123", 2, 2), "•••");
    }
}
