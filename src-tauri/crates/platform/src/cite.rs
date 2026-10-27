//! The words of an answer and its data (E1 of IA-4): the model gives the context and the order, and cites each datum
//! as `[[path]]` instead of writing it; the screen puts there the datum as it is, with the name of its field. What is
//! cited has to exist, and a value the words copy instead of citing makes them not hold: the data are always exact.

/// The paths the text cites, once each, in the order they first appear. A path may hold brackets of its own
/// (`[[missing[0].field]]`, `[[missing[0]]]`).
pub fn paths(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (path, _) in spans(text) {
        if !found.contains(&path) {
            found.push(path);
        }
    }
    found
}

/// The text without its citations: what the figure check reads, so the index of a path is not taken for a figure.
pub fn without_citations(text: &str) -> String {
    let mut out = String::new();
    let mut last = 0;
    for (_, (start, end)) in spans(text) {
        out.push_str(&text[last..start]);
        last = end;
    }
    out.push_str(&text[last..]);
    out
}

/// Whether the words write one of these data instead of citing it: as a whole word, ignoring case. Only data of the
/// institution count: a code of the program (`legal`, `legal_rfc`) is a word the screen names in its own way, and the
/// words may well say «legal» in Spanish. Short values (under four letters) are left out, since they are common words.
pub fn copies<'a>(text: &str, values: impl IntoIterator<Item = &'a str>) -> bool {
    let text = without_citations(text).to_lowercase();
    values.into_iter().filter(|v| !is_code(v)).map(str::to_lowercase).filter(|v| v.chars().count() >= 4).any(|v| whole(&text, &v))
}

/// A code of the program: lowercase letters, digits and `_`, like the ids of fields and the options of a list.
fn is_code(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn whole(text: &str, word: &str) -> bool {
    let edge = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
    text.match_indices(word).any(|(i, _)| edge(text[..i].chars().next_back()) && edge(text[i + word.len()..].chars().next()))
}

/// Each citation, with where it starts and ends in the text.
fn spans(text: &str) -> Vec<(String, (usize, usize))> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find("[[").map(|i| from + i) {
        let inner = open + 2;
        let mut depth = 0usize;
        let mut close = None;
        let chars: Vec<(usize, char)> = text[inner..].char_indices().map(|(i, c)| (inner + i, c)).collect();
        for (k, &(i, c)) in chars.iter().enumerate() {
            match c {
                '[' => depth += 1,
                ']' if depth > 0 => depth -= 1,
                ']' if chars.get(k + 1).is_some_and(|&(_, n)| n == ']') => {
                    close = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let Some(close) = close else { break };
        let path = text[inner..close].trim().to_string();
        if !path.is_empty() {
            out.push((path, (open, close + 2)));
        }
        from = close + 2;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_may_hold_brackets_of_its_own() {
        let text = "Empiece por [[missing[0].field]] y [[missing[1]]]; luego [[missing[0].field]] otra vez.";
        assert_eq!(paths(text), vec!["missing[0].field".to_string(), "missing[1]".to_string()]);
        assert_eq!(without_citations(text), "Empiece por  y ; luego  otra vez.");
        assert_eq!(paths("Sin citas [y con corchetes] sueltos [["), Vec::<String>::new());
    }

    #[test]
    fn a_datum_written_instead_of_cited_is_found_as_a_whole_word() {
        assert!(copies("Se llama Casa Hogar.", ["Casa Hogar"]));
        assert!(copies("La misión es ofrecer un espacio seguro", ["Ofrecer un espacio seguro"]));
        // «contacto» is not «Contacto Uno», a citation is not a copy, and a short value is a common word
        assert!(!copies("En contacto faltan datos", ["Contacto Uno"]));
        assert!(!copies("Falta [[missing[0].field]]", ["missing[0].field"]));
        assert!(!copies("Es de casa", ["sí", "De"]));
    }

    /// A code of the program is not a datum: «en la sección legal» does not copy the window `legal`.
    #[test]
    fn a_code_of_the_program_may_be_said_in_words() {
        assert!(!copies("Faltan datos en la sección legal y en identidad", ["legal", "identity", "legal_rfc"]));
    }
}
