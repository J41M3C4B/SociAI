//! Text normalisation for matching: uppercase, no accents, Ñ folded to N,
//! with a map from normalised bytes back to the original byte positions.

use std::ops::Range;

pub struct Normalized {
    pub text: String,
    /// For every byte of `text`, the (start, end) byte range of the original char it came from.
    map: Vec<(usize, usize)>,
}

fn fold(ch: char, out: &mut Vec<char>) {
    match ch {
        'á' | 'à' | 'ä' | 'â' | 'Á' | 'À' | 'Ä' | 'Â' => out.push('A'),
        'é' | 'è' | 'ë' | 'ê' | 'É' | 'È' | 'Ë' | 'Ê' => out.push('E'),
        'í' | 'ì' | 'ï' | 'î' | 'Í' | 'Ì' | 'Ï' | 'Î' => out.push('I'),
        'ó' | 'ò' | 'ö' | 'ô' | 'Ó' | 'Ò' | 'Ö' | 'Ô' => out.push('O'),
        'ú' | 'ù' | 'ü' | 'û' | 'Ú' | 'Ù' | 'Ü' | 'Û' => out.push('U'),
        'ñ' | 'Ñ' => out.push('N'),
        // combining accent marks disappear
        '\u{0300}'..='\u{036F}' => {}
        _ => out.extend(ch.to_uppercase()),
    }
}

/// Normalises a single word the same way (used for the name lists).
pub fn fold_str(s: &str) -> String {
    Normalized::new(s).text
}

impl Normalized {
    pub fn new(orig: &str) -> Self {
        let mut text = String::with_capacity(orig.len());
        let mut map = Vec::with_capacity(orig.len());
        let mut buf = Vec::with_capacity(2);
        for (i, ch) in orig.char_indices() {
            let end = i + ch.len_utf8();
            buf.clear();
            fold(ch, &mut buf);
            for c in &buf {
                let before = text.len();
                text.push(*c);
                map.extend(std::iter::repeat((i, end)).take(text.len() - before));
            }
        }
        Normalized { text, map }
    }

    /// Converts a byte range of `text` into a byte range of the original string.
    pub fn orig_span(&self, r: Range<usize>) -> Range<usize> {
        self.map[r.start].0..self.map[r.end - 1].1
    }
}
