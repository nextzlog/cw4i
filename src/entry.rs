//! Picking decoded words into a log entry: a callsign and a received number.
//!
//! A host such as zLog receives each [`Pick`] and puts it into its own entry row;
//! the standalone application copies it to the clipboard.

use crate::station::{Word, WordKind};

/// A word the operator picked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    Callsign(String),
    Number(String),
}

impl Pick {
    /// The pick for a word, or `None` for words that do not go into an entry.
    pub fn of(word: &Word) -> Option<Pick> {
        match word.kind {
            WordKind::Callsign => Some(Pick::Callsign(word.text.clone())),
            WordKind::Exchange if !is_report(&word.text) => {
                Some(Pick::Number(normalize_number(&word.text)))
            }
            _ => None,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Pick::Callsign(s) | Pick::Number(s) => s,
        }
    }
}

/// A signal report such as 599 or 5NN, which the log records by itself.
pub fn is_report(text: &str) -> bool {
    let t: String = text.chars().map(cut_digit).collect();
    t.len() == 3 && t.starts_with('5') && t.chars().all(|c| c.is_ascii_digit())
}

/// Cut digits sent as letters (T or O for 0, A for 1, U for 2, V for 3, E for 5, N for 9).
fn cut_digit(c: char) -> char {
    match c {
        'T' | 'O' => '0',
        'A' => '1',
        'U' => '2',
        'V' => '3',
        'E' => '5',
        'N' => '9',
        _ => c,
    }
}

/// A received number with cut digits written as digits; a final power letter
/// (P, L, M or H) is kept, and a word that is not a plain number is kept as read.
pub fn normalize_number(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let power = matches!(chars.last(), Some('P' | 'L' | 'M' | 'H'));
    let body = if power {
        &chars[..chars.len() - 1]
    } else {
        &chars[..]
    };
    let mut out: String = body.iter().copied().map(cut_digit).collect();
    if out.is_empty() || !out.chars().all(|c| c.is_ascii_digit()) {
        // not a plain number (a serial with letters, or a misread): keep it as read
        return text.into();
    }
    if power {
        out.push(chars[chars.len() - 1]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, kind: WordKind) -> Word {
        Word {
            text: text.into(),
            kind,
            sure: true,
            p: 1.0,
        }
    }

    #[test]
    fn reports_are_not_picked() {
        assert!(is_report("5NN"));
        assert!(is_report("599"));
        assert!(is_report("ENN"));
        assert!(!is_report("1807M"));
        assert_eq!(Pick::of(&word("5NN", WordKind::Exchange)), None);
    }

    #[test]
    fn numbers_lose_cut_digits_but_keep_the_power_letter() {
        assert_eq!(normalize_number("1807M"), "1807M");
        assert_eq!(normalize_number("1OT3H"), "1003H");
        assert_eq!(normalize_number("NA"), "91");
        assert_eq!(normalize_number("82A2BECQ"), "82A2BECQ");
        assert_eq!(
            Pick::of(&word("2T5P", WordKind::Exchange)),
            Some(Pick::Number("205P".into()))
        );
    }

    #[test]
    fn procedure_words_are_not_picked() {
        assert_eq!(Pick::of(&word("TU", WordKind::Procedure)), None);
        assert_eq!(
            Pick::of(&word("JA1ZLO", WordKind::Callsign)),
            Some(Pick::Callsign("JA1ZLO".into()))
        );
    }
}
