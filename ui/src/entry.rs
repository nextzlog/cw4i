use std::collections::BTreeMap;

use crate::station::{Word, WordKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    Callsign(String),
    Number(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CutTable {
    digits: BTreeMap<char, char>,
}

impl Default for CutTable {
    fn default() -> Self {
        CutTable::new([
            ('T', '0'),
            ('O', '0'),
            ('A', '1'),
            ('U', '2'),
            ('V', '3'),
            ('E', '5'),
            ('N', '9'),
        ])
    }
}

impl CutTable {
    pub fn new(pairs: impl IntoIterator<Item = (char, char)>) -> CutTable {
        let digits = pairs
            .into_iter()
            .map(|(l, d)| (l.to_ascii_uppercase(), d))
            .filter(|(l, d)| l.is_ascii_uppercase() && d.is_ascii_digit())
            .collect();
        CutTable { digits }
    }

    pub fn pairs(&self) -> Vec<(char, char)> {
        self.digits.iter().map(|(l, d)| (*l, *d)).collect()
    }

    pub fn digit(&self, c: char) -> char {
        self.digits.get(&c).copied().unwrap_or(c)
    }

    pub fn cut(&self, word: &str) -> String {
        word.chars().map(|c| self.digit(c)).collect()
    }

    pub fn is_report(&self, text: &str) -> bool {
        let t = self.cut(text);
        t.len() == 3 && t.starts_with('5') && t.chars().all(|c| c.is_ascii_digit())
    }

    pub fn normalize_number(&self, text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let power = matches!(chars.last(), Some('P' | 'L' | 'M' | 'H'));
        let body = if power {
            &chars[..chars.len() - 1]
        } else {
            &chars[..]
        };
        let mut out: String = body.iter().map(|c| self.digit(*c)).collect();
        if out.is_empty() || !out.chars().all(|c| c.is_ascii_digit()) {
            return text.into();
        }
        if power {
            out.push(chars[chars.len() - 1]);
        }
        out
    }

    pub fn pick(&self, word: &Word) -> Option<Pick> {
        match word.kind {
            WordKind::Callsign => Some(Pick::Callsign(word.text.clone())),
            WordKind::Exchange if !self.is_report(&word.text) => {
                Some(Pick::Number(self.normalize_number(&word.text)))
            }
            _ => None,
        }
    }
}

impl Pick {
    pub fn of(word: &Word) -> Option<Pick> {
        CutTable::default().pick(word)
    }

    pub fn text(&self) -> &str {
        match self {
            Pick::Callsign(s) | Pick::Number(s) => s,
        }
    }
}

pub fn is_report(text: &str) -> bool {
    CutTable::default().is_report(text)
}

pub fn normalize_number(text: &str) -> String {
    CutTable::default().normalize_number(text)
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
    fn the_default_cut_table() {
        let t = CutTable::default();
        assert_eq!(t.cut("TOAUVEN"), "0012359");
        assert_eq!(
            t.pairs(),
            [
                ('A', '1'),
                ('E', '5'),
                ('N', '9'),
                ('O', '0'),
                ('T', '0'),
                ('U', '2'),
                ('V', '3')
            ]
        );
    }

    #[test]
    fn a_custom_cut_table_reads_numbers_and_reports() {
        let t = CutTable::new([('o', '0'), ('A', '1'), ('N', '9'), ('X', 'Y'), ('1', '2')]);
        assert_eq!(t.pairs(), [('A', '1'), ('N', '9'), ('O', '0')]);
        assert_eq!(t.normalize_number("1OA3H"), "1013H");
        assert_eq!(t.normalize_number("1T3H"), "1T3H");
        assert!(t.is_report("5NN") && !t.is_report("ENN"));
        assert_eq!(
            t.pick(&word("2T5P", WordKind::Exchange)),
            Some(Pick::Number("2T5P".into()))
        );
        assert_eq!(CutTable::new([]).cut("5NN"), "5NN");
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
