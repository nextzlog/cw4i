use std::time::SystemTime;

use crate::entry::CutTable;
use crate::station::{Station, WordKind};

const WORDS_READ: usize = 24;

const PROCEDURES: &[&str] = &[
    "CQ", "DE", "TU", "R", "K", "KN", "BK", "AR", "SK", "QSL", "QRZ", "TEST", "NR", "UR", "GL",
    "EE", "73", "TNX", "TKS", "FB", "AGN", "PSE", "CFM", "RR", "QSO",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Contact {
    pub station: u32,
    pub callsign: Option<String>,
    pub report: Option<String>,
    pub exchange: Vec<String>,
    pub raw_exchange: Vec<String>,
    pub freq: f64,
    pub wpm: f64,
    pub p: f64,
    pub time: SystemTime,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Heard {
    pub callsign: Option<String>,
    pub report: Option<String>,
    pub exchange: Vec<String>,
    pub raw_exchange: Vec<String>,
}

impl Contact {
    pub fn of(s: &Station, my_call: &str) -> Contact {
        Contact::with_cut(s, my_call, &CutTable::default())
    }

    pub fn with_cut(s: &Station, my_call: &str, table: &CutTable) -> Contact {
        let text = s.text();
        let words: Vec<&str> = text.split_whitespace().collect();
        let from = words.len().saturating_sub(WORDS_READ);
        let mut heard = parse_with(&words[from..], my_call, table);
        if heard.callsign.is_none() {
            heard.callsign = s
                .callsign()
                .filter(|c| !same_call(c, my_call))
                .map(str::to_string);
        }
        let word_p = heard.callsign.as_ref().and_then(|c| {
            s.words
                .iter()
                .find(|w| w.kind == WordKind::Callsign && &w.text == c)
                .map(|w| w.p)
        });
        let sure = if s.letters.is_empty() {
            0.0
        } else {
            s.letters
                .iter()
                .filter(|l| !l.doubtful && !l.suspect)
                .count() as f64
                / s.letters.len() as f64
        };
        Contact {
            station: s.id,
            callsign: heard.callsign,
            report: heard.report,
            exchange: heard.exchange,
            raw_exchange: heard.raw_exchange,
            freq: s.freq,
            wpm: s.wpm,
            p: word_p.unwrap_or(sure),
            time: SystemTime::now(),
        }
    }
}

fn clean(word: &str) -> Option<String> {
    let w = word.trim().to_uppercase();
    (!w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '/')).then_some(w)
}

fn base_call(part: &str) -> bool {
    let chars: Vec<char> = part.chars().collect();
    let Some(i) = chars.iter().rposition(|c| c.is_ascii_digit()) else {
        return false;
    };
    let prefix = &chars[..i];
    let suffix = &chars[i + 1..];
    (1..=3).contains(&prefix.len())
        && prefix.iter().any(|c| c.is_ascii_alphabetic())
        && prefix.iter().all(|c| c.is_ascii_alphanumeric())
        && (1..=4).contains(&suffix.len())
        && suffix.iter().all(|c| c.is_ascii_alphabetic())
}

pub fn base_of(call: &str) -> Option<&str> {
    let parts: Vec<&str> = call.split('/').collect();
    let base: Vec<&&str> = parts.iter().filter(|p| base_call(p)).collect();
    let ok = base.len() == 1
        && parts.len() <= 3
        && parts
            .iter()
            .all(|p| (1..=4).contains(&p.len()) || base_call(p));
    ok.then(|| *base[0])
}

pub fn is_callsign(word: &str) -> bool {
    is_callsign_with(word, &CutTable::default())
}

fn is_callsign_with(word: &str, table: &CutTable) -> bool {
    base_of(word).is_some() && table.normalize_number(word) == word
}

fn same_call(a: &str, b: &str) -> bool {
    !b.is_empty()
        && (a.eq_ignore_ascii_case(b)
            || matches!((base_of(a), base_of(&b.to_uppercase())), (Some(x), Some(y)) if x == y))
}

pub fn cut(word: &str) -> String {
    CutTable::default().cut(word)
}

pub fn report_of(word: &str) -> Option<String> {
    report_with(word, &CutTable::default())
}

pub fn report_with(word: &str, table: &CutTable) -> Option<String> {
    let t = table.cut(word);
    let digits = t.chars().all(|c| c.is_ascii_digit());
    let b = t.as_bytes();
    let ok = digits
        && match b.len() {
            2 => t == "59",
            3 => b[0] == b'5' && b[1] != b'0' && b[2] != b'0',
            _ => false,
        };
    ok.then_some(t)
}

pub fn parse(words: &[&str], my_call: &str) -> Heard {
    parse_with(words, my_call, &CutTable::default())
}

pub fn parse_with(words: &[&str], my_call: &str, table: &CutTable) -> Heard {
    let is_callsign = |w: &str| is_callsign_with(w, table);
    let report_of = |w: &str| report_with(w, table);
    let words: Vec<String> = words.iter().filter_map(|w| clean(w)).collect();
    let mut best: Option<(i32, usize, &str)> = None;
    for (i, w) in words.iter().enumerate() {
        if !is_callsign(w) || same_call(w, my_call) {
            continue;
        }
        let after_de = i > 0 && words[i - 1] == "DE";
        let repeats = words.iter().filter(|x| *x == w).count() as i32;
        let score = 2 * i32::from(after_de) + repeats;
        if best.is_none_or(|(s, _, _)| score >= s) {
            best = Some((score, i, w));
        }
    }
    let callsign = best.map(|(_, _, c)| c.to_string());
    let last_call = callsign
        .as_ref()
        .and_then(|c| words.iter().rposition(|w| w == c));
    let last_report = words.iter().rposition(|w| report_of(w).is_some());
    let report = last_report.and_then(|i| report_of(&words[i]));
    let start = match (last_report, last_call) {
        (Some(r), _) => r + 1,
        (None, Some(c)) => c + 1,
        (None, None) => 0,
    };
    let mut exchange: Vec<String> = Vec::new();
    let mut raw_exchange: Vec<String> = Vec::new();
    for w in &words[start.min(words.len())..] {
        if PROCEDURES.contains(&w.as_str()) || is_callsign(w) || report_of(w).is_some() {
            continue;
        }
        let n = table.normalize_number(w);
        if !exchange.contains(&n) {
            exchange.push(n);
            raw_exchange.push(w.clone());
        }
    }
    Heard {
        callsign,
        report,
        exchange,
        raw_exchange,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard(text: &str, my_call: &str) -> Heard {
        Heard {
            raw_exchange: Vec::new(),
            ..parse(&text.split_whitespace().collect::<Vec<_>>(), my_call)
        }
    }

    fn h(call: Option<&str>, report: Option<&str>, exchange: &[&str]) -> Heard {
        Heard {
            callsign: call.map(str::to_string),
            report: report.map(str::to_string),
            exchange: exchange.iter().map(|s| s.to_string()).collect(),
            raw_exchange: Vec::new(),
        }
    }

    #[test]
    fn the_raw_exchange_keeps_the_cut_letters() {
        let x = parse(&["5NN", "1OT3H", "TOKYO"], "");
        assert_eq!(x.exchange, ["1003H", "TOKYO"]);
        assert_eq!(x.raw_exchange, ["1OT3H", "TOKYO"]);
    }

    #[test]
    fn callsigns() {
        for c in [
            "JA1ZLO",
            "JA1ZLO/1",
            "W1AW",
            "3D2AB",
            "2E0ABC",
            "VK2/JA1ZLO",
            "JA1ZLO/P",
            "K1ABC/QRP",
            "JH1ABC/MM",
        ] {
            assert!(is_callsign(c), "{c}");
        }
        for w in [
            "5NN",
            "599",
            "1317M",
            "1OT3H",
            "TU",
            "R",
            "15",
            "13M",
            "CQ",
            "JA1",
            "ABCD1EFGHI",
        ] {
            assert!(!is_callsign(w), "{w}");
        }
        assert_eq!(base_of("VK2/JA1ZLO"), Some("JA1ZLO"));
    }

    #[test]
    fn reports_and_cut_numbers() {
        assert_eq!(report_of("5NN").as_deref(), Some("599"));
        assert_eq!(report_of("ENN").as_deref(), Some("599"));
        assert_eq!(report_of("579").as_deref(), Some("579"));
        assert_eq!(report_of("59").as_deref(), Some("59"));
        assert_eq!(report_of("55"), None);
        assert_eq!(report_of("1317M"), None);
        assert_eq!(report_of("500"), None);
    }

    #[test]
    fn contest_exchanges() {
        assert_eq!(
            heard("TU JA1ZLO 5NN 1317M", ""),
            h(Some("JA1ZLO"), Some("599"), &["1317M"])
        );
        assert_eq!(heard("JA1YWX JA1YWX", ""), h(Some("JA1YWX"), None, &[]));
        assert_eq!(heard("R 599 15", ""), h(None, Some("599"), &["15"]));
        assert_eq!(heard("DE JA1ZLO/1 K", ""), h(Some("JA1ZLO/1"), None, &[]));
        assert_eq!(
            heard("JA1YWX 5NN 1OT3H", ""),
            h(Some("JA1YWX"), Some("599"), &["1003H"])
        );
        assert_eq!(
            heard("JA1ZLO 5NN NA 5NN NA TU", ""),
            h(Some("JA1ZLO"), Some("599"), &["91"])
        );
    }

    #[test]
    fn the_call_after_de_or_repeated_wins_and_mine_is_skipped() {
        assert_eq!(
            heard("JA1YWX DE JA1ZLO JA1ZLO 5NN 13M", "JA1YWX")
                .callsign
                .as_deref(),
            Some("JA1ZLO")
        );
        assert_eq!(
            heard("JA1YWX DE JA1ZLO", "").callsign.as_deref(),
            Some("JA1ZLO")
        );
        assert_eq!(
            heard("JA1ABC JA1ZLO JA1ZLO", "").callsign.as_deref(),
            Some("JA1ZLO")
        );
        assert_eq!(heard("ja1ywx 5nn 13m", "JA1YWX").callsign, None);
        assert_eq!(
            heard("JA1YWX/1 DE JH1ABC", "ja1ywx").callsign.as_deref(),
            Some("JH1ABC")
        );
    }

    #[test]
    fn a_custom_cut_table_changes_reports_and_exchanges() {
        let words = ["JA1ZLO", "ENN", "1OT3H"];
        let table = CutTable::new([('O', '0'), ('T', '0'), ('N', '9')]);
        let x = parse_with(&words, "", &table);
        assert_eq!(x.report, None);
        assert_eq!(x.exchange, ["ENN", "1003H"]);
        let x = parse_with(&words, "", &CutTable::default());
        assert_eq!(
            (x.report.as_deref(), x.exchange.as_slice()),
            (Some("599"), &["1003H".to_string()][..])
        );
    }

    #[test]
    fn garbled_fragments_are_skipped() {
        assert_eq!(
            heard("J?1ZLO JA1ZLO 5NN 13M 13M", ""),
            h(Some("JA1ZLO"), Some("599"), &["13M"])
        );
        assert_eq!(heard("", ""), Heard::default());
        assert_eq!(heard("E E T", "").callsign, None);
    }
}
