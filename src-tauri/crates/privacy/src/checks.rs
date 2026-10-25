//! Check-digit validations that cut false positives.

const CURP_DICT: &str = "0123456789ABCDEFGHIJKLMNÑOPQRSTUVWXYZ";

/// CURP: 18 chars, last one is a check digit over the first 17.
pub fn curp_ok(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() != 18 {
        return false;
    }
    let mut sum = 0u32;
    for (i, c) in chars[..17].iter().enumerate() {
        match CURP_DICT.chars().position(|d| d == *c) {
            Some(v) => sum += v as u32 * (18 - i as u32),
            None => return false,
        }
    }
    let expected = (10 - sum % 10) % 10;
    chars[17].to_digit(10) == Some(expected)
}

/// CLABE: 18 digits, weights 3-7-1 over the first 17.
pub fn clabe_ok(s: &str) -> bool {
    let d: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if d.len() != 18 {
        return false;
    }
    let w = [3, 7, 1];
    let sum: u32 = d[..17].iter().enumerate().map(|(i, x)| (x * w[i % 3]) % 10).sum();
    d[17] == (10 - sum % 10) % 10
}

/// Luhn algorithm over a string of digits (cards, NSS).
pub fn luhn_ok(s: &str) -> bool {
    let d: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if d.len() < 2 {
        return false;
    }
    let sum: u32 = d
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &x)| {
            if i % 2 == 1 {
                let y = x * 2;
                if y > 9 { y - 9 } else { y }
            } else {
                x
            }
        })
        .sum();
    sum % 10 == 0
}

/// `YYMMDD` with a possible month and day.
pub fn yymmdd_ok(s: &str) -> bool {
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let m: u32 = s[2..4].parse().unwrap();
    let d: u32 = s[4..6].parse().unwrap();
    let max = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 29,
        _ => return false,
    };
    (1..=max).contains(&d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values() {
        assert!(luhn_ok("4111111111111111"));
        assert!(!luhn_ok("4111111111111112"));
        assert!(clabe_ok("012180001234567899"));
        assert!(!clabe_ok("012180001234567898"));
        assert!(curp_ok("LOPM800101MDFRZN09"));
        assert!(!curp_ok("LOPM800101MDFRZN08"));
        assert!(yymmdd_ok("800101"));
        assert!(!yymmdd_ok("801301"));
        assert!(!yymmdd_ok("800132"));
    }
}
