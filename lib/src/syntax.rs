//! Callsign and exchange syntax of committed text.
//!
//! Committed characters of a track are cut into tokens at spaces; procedure words and
//! reports glued to a token (DEJA1ZLO, TU5NN1807M) are split off when that makes every
//! part valid. Each token is classified as a callsign, an exchange, a procedure word or
//! unknown, and gets a verdict. Characters are never changed: the verdict only says
//! whether a token can be shown as confirmed or must be shown as doubtful.

/// Class of a token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    /// Callsign, or a token shaped like one.
    Callsign,
    /// Signal report, serial number or contest exchange.
    Exchange,
    /// Procedure word or abbreviation (CQ, DE, TU, 73 and so on).
    Procedure,
    /// Anything else.
    Unknown,
}

/// Verdict on a closed token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Valid and every character was committed with confidence.
    Confirmed,
    /// A valid callsign with doubtful characters that matches an earlier read on the track.
    Repeated,
    /// Valid, but at least one character was a doubtful best guess.
    DoubtfulCharacter,
    /// The token breaks callsign or exchange syntax.
    Invalid,
    /// The token looks like two tokens run together (a callsign and a procedure word or
    /// a second callsign).
    RunTogether,
    /// The token is not a callsign, exchange or known procedure word.
    Unknown,
    /// The first token of a track whose first character was dropped as a possible
    /// cut-off tail.
    Truncated,
    /// A callsign that is the start or the end of a longer callsign read before on the
    /// track (a character lost or split off).
    Partial,
}

impl Verdict {
    /// Whether the token may be shown as confirmed.
    pub fn confirmed(self) -> bool {
        matches!(self, Verdict::Confirmed | Verdict::Repeated)
    }

    /// Short lowercase name.
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Confirmed => "confirmed",
            Verdict::Repeated => "repeated",
            Verdict::DoubtfulCharacter => "doubtful-character",
            Verdict::Invalid => "invalid",
            Verdict::RunTogether => "run-together",
            Verdict::Unknown => "unknown",
            Verdict::Truncated => "truncated",
            Verdict::Partial => "partial",
        }
    }
}

impl TokenKind {
    /// Short lowercase name.
    pub fn name(self) -> &'static str {
        match self {
            TokenKind::Callsign => "callsign",
            TokenKind::Exchange => "exchange",
            TokenKind::Procedure => "procedure",
            TokenKind::Unknown => "unknown",
        }
    }
}

/// One classified token of a track's committed text.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    /// Index of the first character in the track's committed text (spaces included).
    pub start: usize,
    pub text: String,
    pub kind: TokenKind,
    pub verdict: Verdict,
    /// Lowest and summed log probability of its characters.
    pub min_lp: f64,
    pub sum_lp: f64,
    /// Whether the same callsign was read before on the track.
    pub repeat: bool,
}

/// Procedure words and common abbreviations accepted as they are.
const PROCEDURE: [&str; 51] = [
    "CQ", "TEST", "DE", "TU", "R", "UR", "QSL", "K", "KN", "BK", "QRZ", "?", "NR", "73", "88",
    "TNX", "TKS", "GL", "GM", "GA", "GE", "GN", "FB", "OM", "ES", "HR", "RST", "NAME", "QTH",
    "AGN", "PSE", "CFM", "WX", "ANT", "RIG", "PWR", "SK", "AR", "BT", "CL", "QRL", "QRS", "QRQ",
    "QSY", "QRP", "QRO", "QSO", "TEST?", "CQ?", "DX", "UP",
];

/// Procedure words that may be glued to the front of a token.
const GLUED_FRONT: [&str; 6] = ["DE", "TU", "UR", "R", "CQ", "TEST"];

/// Procedure words that may be glued to the end of a token.
const GLUED_BACK: [&str; 4] = ["TU", "BK", "KN", "K"];

/// Portable suffixes after a slash.
const PORTABLE: [&str; 6] = ["P", "M", "MM", "AM", "A", "QRP"];

/// Prefixes longer than two characters that are in use.
const LONG_PREFIXES: [&str; 3] = ["3DA", "VP2E", "VP2M"];

/// Cut-number letters and their digits.
fn cut_digit(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some(c as u8 - b'0'),
        'T' | 'O' => Some(0),
        'A' => Some(1),
        'U' => Some(2),
        'V' => Some(3),
        'E' => Some(5),
        'N' => Some(9),
        _ => None,
    }
}

fn is_procedure(w: &str) -> bool {
    PROCEDURE.contains(&w)
}

/// Signal report such as 5NN or 599 (cut digits allowed).
fn is_report(w: &str) -> bool {
    let d: Vec<Option<u8>> = w.chars().map(cut_digit).collect();
    d.len() == 3
        && w.starts_with(|c: char| c.is_ascii_digit())
        && matches!(d[0], Some(1..=5))
        && matches!(d[1], Some(1..=9))
        && matches!(d[2], Some(1..=9))
}

/// Serial or contest number: digits and cut digits with at least one real digit,
/// optionally ended by exactly one power letter P, L, M or H.
fn is_number(w: &str) -> bool {
    let c: Vec<char> = w.chars().collect();
    if c.len() >= 4 && matches!(c[0], '1'..='5') && is_letter(c[1]) && is_letter(c[2]) {
        // a report glued to a number must be split off and valid on its own
        return false;
    }
    let body = w.strip_suffix(['P', 'L', 'M', 'H']).unwrap_or(w);
    !body.is_empty()
        && body.len() <= 8
        && body.chars().any(|c| c.is_ascii_digit())
        && body.chars().all(|c| cut_digit(c).is_some())
}

fn is_exchange(w: &str) -> bool {
    is_report(w) || is_number(w)
}

fn is_letter(c: char) -> bool {
    c.is_ascii_uppercase()
}

/// Whether p is an allocated-looking prefix (one letter of the one-letter series, two
/// characters with at least one letter and not starting with 0 or 1, or a listed longer one).
fn valid_prefix(p: &str) -> bool {
    let c: Vec<char> = p.chars().collect();
    match c.len() {
        1 => matches!(c[0], 'B' | 'F' | 'G' | 'I' | 'K' | 'M' | 'N' | 'R' | 'W'),
        2 => {
            (is_letter(c[0]) && (is_letter(c[1]) || c[1].is_ascii_digit()))
                || (matches!(c[0], '2'..='9') && is_letter(c[1]))
        }
        _ => LONG_PREFIXES.contains(&p),
    }
}

/// Japanese amateur prefixes: JA-JS, 7K-7N, 8J-8N.
fn japanese(p: &str) -> Option<char> {
    let c: Vec<char> = p.chars().collect();
    if c.len() != 2 {
        return None;
    }
    match (c[0], c[1]) {
        ('J', 'A'..='S') => Some('J'),
        ('7', 'K'..='N') => Some('7'),
        ('8', 'J'..='N') => Some('8'),
        _ => None,
    }
}

/// Base callsign: prefix, one area digit (two for listed long prefixes), suffix of one
/// to four letters with the Japanese rules for JA-JS, 7K-7N and 8J-8N.
fn base_call(w: &str) -> bool {
    let c: Vec<char> = w.chars().collect();
    for plen in 1..=4.min(c.len()) {
        let p: String = c[..plen].iter().collect();
        if !valid_prefix(&p) {
            continue;
        }
        let rest = &c[plen..];
        if rest.is_empty() || !rest[0].is_ascii_digit() {
            continue;
        }
        let area = rest[0];
        let suffix = &rest[1..];
        if suffix.is_empty() || suffix.len() > 4 || !suffix.iter().all(|&x| is_letter(x)) {
            continue;
        }
        let ok = match japanese(&p) {
            Some('J') if p == "JD" => area == '1' && (2..=3).contains(&suffix.len()),
            Some('J') => (2..=3).contains(&suffix.len()),
            Some('7') => matches!(area, '1'..='4') && suffix.len() == 3,
            Some(_) => suffix.len() <= 3,
            None => true,
        };
        if ok {
            return true;
        }
    }
    false
}

/// Full callsign with an optional prefix before a slash (VK9/JA1ABC) and an optional
/// portable part after it (/1, /P, /QRP, /VK9).
pub fn is_callsign(w: &str) -> bool {
    let parts: Vec<&str> = w.split('/').collect();
    match parts.len() {
        1 => base_call(parts[0]),
        2 => {
            let (a, b) = (parts[0], parts[1]);
            (base_call(a) && portable(b)) || (base_call(b) && host_prefix(a))
        }
        3 => host_prefix(parts[0]) && base_call(parts[1]) && portable(parts[2]),
        _ => false,
    }
}

fn portable(s: &str) -> bool {
    (s.len() == 1 && s.chars().all(|c| c.is_ascii_digit()))
        || PORTABLE.contains(&s)
        || host_prefix(s)
}

/// A country prefix used before or after a slash, optionally with an area digit (VK9, F, DL).
fn host_prefix(s: &str) -> bool {
    let t = s.strip_suffix(|c: char| c.is_ascii_digit()).unwrap_or(s);
    !t.is_empty() && t.len() <= 3 && valid_prefix(t)
}

/// Kind and syntax validity of a single token.
fn classify(w: &str) -> (TokenKind, bool) {
    if is_procedure(w) {
        return (TokenKind::Procedure, true);
    }
    if is_exchange(w) {
        return (TokenKind::Exchange, true);
    }
    if is_callsign(w) {
        return (TokenKind::Callsign, true);
    }
    let digits = w.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits >= 2 {
        (TokenKind::Exchange, false)
    } else if w.chars().any(|c| c.is_ascii_digit() || c == '/') {
        (TokenKind::Callsign, false)
    } else {
        (TokenKind::Unknown, false)
    }
}

/// Splits glued procedure words and reports off a word when every part becomes valid
/// (a glued 5NN first); returns (offset, length) of the parts.
fn split(w: &str) -> Vec<(usize, usize)> {
    if classify(w).1 {
        return vec![(0, w.len())];
    }
    if let Some(i) = w.find("5NN") {
        let mut v = Vec::new();
        let mut ok = true;
        if i > 0 {
            let head = &w[..i];
            let p = split(head);
            ok &= all_valid(head, &p);
            v.extend(p);
        }
        v.push((i, 3));
        if i + 3 < w.len() {
            let rest = &w[i + 3..];
            let p = split(rest);
            ok &= all_valid(rest, &p);
            v.extend(p.iter().map(|&(o, l)| (o + i + 3, l)));
        }
        if ok && v.len() > 1 {
            return v;
        }
    }
    for f in GLUED_FRONT {
        if let Some(rest) = w.strip_prefix(f) {
            if !rest.is_empty() {
                let tail = split(rest);
                if all_valid(rest, &tail) {
                    let mut v = vec![(0, f.len())];
                    v.extend(tail.iter().map(|&(o, l)| (o + f.len(), l)));
                    return v;
                }
            }
        }
    }
    for b in GLUED_BACK {
        if let Some(head) = w.strip_suffix(b) {
            if !head.is_empty() {
                let p = split(head);
                if all_valid(head, &p) {
                    let mut v = p;
                    v.push((head.len(), b.len()));
                    return v;
                }
            }
        }
    }
    vec![(0, w.len())]
}

fn all_valid(w: &str, parts: &[(usize, usize)]) -> bool {
    parts.iter().all(|&(o, l)| classify(&w[o..o + l]).1)
}

/// Whether a valid callsign is better read as a callsign followed by a procedure word
/// (a long suffix ending in K, R, BK or TU), or a word is two callsigns run together.
fn run_together(w: &str, valid_call: bool) -> bool {
    if valid_call {
        let base = w.split('/').next().unwrap_or(w);
        let suffix = base.chars().rev().take_while(|c| is_letter(*c)).count();
        return suffix == 4
            && ["K", "R", "BK", "TU"].iter().any(|t| {
                base.strip_suffix(t)
                    .is_some_and(|b| b.len() + t.len() == base.len() && base_call(b))
            });
    }
    (3..w.len().saturating_sub(2))
        .any(|i| w.is_char_boundary(i) && is_callsign(&w[..i]) && is_callsign(&w[i..]))
}

/// Whether one more letter after the callsign would still give a valid callsign.
fn extendable(w: &str) -> bool {
    !w.contains('/') && ('A'..='Z').any(|c| base_call(&format!("{w}{c}")))
}

fn kind_is_seen(seen: &[String], text: &str) -> bool {
    seen.iter().any(|s| s == text)
}

/// Calls remembered per track for the repetition rule.
const SEEN_MAX: usize = 32;

/// Token state of one track.
#[derive(Default)]
pub struct Reader {
    /// Characters of the open token, their doubt flags and probabilities.
    open: Vec<(char, bool, f64)>,
    /// Index of the open token's first character.
    open_start: usize,
    /// Number of characters committed so far (spaces included).
    pos: usize,
    /// Callsigns read before on this track.
    seen: Vec<String>,
    /// The track's first character was dropped, so its first token may be incomplete.
    truncated: bool,
    /// A confirmed callsign that one more letter would keep valid, held until the next
    /// token shows whether that token completes it.
    held: Option<Token>,
}

impl Reader {
    /// Adds committed characters; returns the tokens closed by them.
    #[cfg(test)]
    pub fn feed(&mut self, chars: impl Iterator<Item = (char, bool)>) -> Vec<Token> {
        self.feed_p(chars.map(|(c, d)| (c, d, 1.0)))
    }

    /// Adds committed characters with their probabilities; returns the tokens closed by
    /// them.
    pub fn feed_p(&mut self, chars: impl Iterator<Item = (char, bool, f64)>) -> Vec<Token> {
        let mut out = Vec::new();
        for (c, doubt, p) in chars {
            if c == ' ' {
                self.close(&mut out);
            } else {
                if self.open.is_empty() {
                    self.open_start = self.pos;
                }
                self.open.push((c, doubt, p));
            }
            self.pos += 1;
        }
        out
    }

    /// Marks the track's first token as possibly incomplete (its first character was
    /// dropped); has no effect once a token has been closed.
    pub fn truncate_first(&mut self) {
        if self.pos == 0 || (self.open_start == 0 && !self.open.is_empty()) {
            self.truncated = true;
        }
    }

    /// Closes the open token at the end of the track.
    pub fn finish(&mut self) -> Vec<Token> {
        let mut out = Vec::new();
        self.close(&mut out);
        out.extend(self.held.take());
        out
    }

    /// Releases the held callsign before the token `next`: when the two joined form a
    /// longer valid callsign (DEJA0BZ E), the space between them may be a lost character
    /// gap, so the callsign is shown as run together.
    fn release(&mut self, next: &str, out: &mut Vec<Token>) {
        if let Some(mut h) = self.held.take() {
            if !next.is_empty() && is_callsign(&format!("{}{}", h.text, next)) {
                h.verdict = Verdict::RunTogether;
                if !h.repeat {
                    self.seen.retain(|s| *s != h.text);
                }
            }
            out.push(h);
        }
    }

    fn close(&mut self, out: &mut Vec<Token>) {
        if self.open.is_empty() {
            return;
        }
        let open = std::mem::take(&mut self.open);
        let truncated = std::mem::take(&mut self.truncated) && self.open_start == 0;
        let word: String = open.iter().map(|x| x.0.to_ascii_uppercase()).collect();
        self.release(if word.is_ascii() { &word } else { "" }, out);
        if !word.is_ascii() {
            out.push(Token {
                start: self.open_start,
                text: word,
                kind: TokenKind::Unknown,
                verdict: Verdict::Unknown,
                min_lp: f64::NEG_INFINITY,
                sum_lp: f64::NEG_INFINITY,
                repeat: false,
            });
            return;
        }
        let parts = split(&word);
        let last = parts.len() - 1;
        for (i, (o, l)) in parts.into_iter().enumerate() {
            let text = &word[o..o + l];
            let doubt = open[o..o + l].iter().any(|x| x.1);
            let lps = open[o..o + l].iter().map(|x| x.2.max(1e-6).ln());
            let min_lp = lps.clone().fold(0.0, f64::min);
            let sum_lp = lps.sum::<f64>();
            let repeat = kind_is_seen(&self.seen, text);
            let (kind, valid) = classify(text);
            let rt = kind == TokenKind::Callsign && run_together(text, valid);
            let verdict = if truncated && o == 0 && kind != TokenKind::Procedure {
                Verdict::Truncated
            } else if rt {
                Verdict::RunTogether
            } else if valid
                && kind == TokenKind::Callsign
                && self
                    .seen
                    .iter()
                    .any(|s| s.len() > text.len() && (s.starts_with(text) || s.ends_with(text)))
            {
                Verdict::Partial
            } else if !valid {
                if kind == TokenKind::Unknown {
                    Verdict::Unknown
                } else {
                    Verdict::Invalid
                }
            } else if doubt {
                if kind == TokenKind::Callsign && self.seen.iter().any(|s| s == text) {
                    Verdict::Repeated
                } else {
                    Verdict::DoubtfulCharacter
                }
            } else {
                Verdict::Confirmed
            };
            if kind == TokenKind::Callsign && valid && !rt && verdict != Verdict::Partial {
                // every valid read counts towards the repetition rule
                if !self.seen.iter().any(|s| s == text) {
                    if self.seen.len() == SEEN_MAX {
                        self.seen.remove(0);
                    }
                    self.seen.push(text.to_string());
                }
            }
            let tok = Token {
                start: self.open_start + o,
                text: text.to_string(),
                kind,
                verdict,
                min_lp,
                sum_lp,
                repeat,
            };
            if i == last && kind == TokenKind::Callsign && verdict.confirmed() && extendable(text) {
                self.held = Some(tok);
            } else {
                out.push(tok);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str, doubt: &[usize]) -> Vec<(String, TokenKind, Verdict)> {
        let mut r = Reader::default();
        let mut v = r.feed(
            text.chars()
                .enumerate()
                .map(|(i, c)| (c, doubt.contains(&i))),
        );
        v.extend(r.finish());
        v.into_iter().map(|t| (t.text, t.kind, t.verdict)).collect()
    }

    fn verdict(w: &str) -> Verdict {
        read(w, &[])[0].2
    }

    #[test]
    fn valid_calls_are_confirmed() {
        for w in [
            "JA1ZLO",
            "JF2OZH/2",
            "JH3WNZ/3",
            "JA0VTK/7",
            "JR3WXA/1",
            "7L1FFH",
            "JO1ZQG/1",
            "4U1ITU",
            "8J1A",
            "JD1BMH",
            "K1A",
            "WA1A",
            "VK9/JA1ABC",
            "JA1ZLO/QRP",
            "W1AW",
            "DL1ABC/P",
            "JA9MAT",
            "JE6QQN",
            "JR1CCP",
        ] {
            assert_eq!(
                read(w, &[]),
                vec![(w.to_string(), TokenKind::Callsign, Verdict::Confirmed)],
                "{w}"
            );
        }
    }

    #[test]
    fn broken_calls_are_flagged() {
        for w in [
            "JFVOZH/2", "O1ZQG/1", "TP/1", "SEJA1TC", "JA1ZLOO", "7L5FFH", "JD2ABC",
        ] {
            assert_ne!(verdict(w), Verdict::Confirmed, "{w}");
        }
        assert_eq!(verdict("JAM"), Verdict::Unknown);
        assert_eq!(verdict("JA9MATJA9MAT"), Verdict::RunTogether);
    }

    #[test]
    fn exchanges() {
        for w in [
            "5NN", "599", "1317M", "134404H", "11O117M", "2214M", "001", "1T",
        ] {
            assert_eq!(read(w, &[])[0].1, TokenKind::Exchange, "{w}");
            assert_eq!(verdict(w), Verdict::Confirmed, "{w}");
        }
        for w in ["13SM2M", "462NW", "1317MM"] {
            assert_eq!(verdict(w), Verdict::Invalid, "{w}");
        }
        // letters that happen to be cut digits are not a number without a real digit
        assert_eq!(verdict("TEN"), Verdict::Unknown);
    }

    #[test]
    fn glued_tokens_are_split() {
        let t: Vec<String> = read("DEJA0BZE TU5NN1807M RUR5NN134404H", &[])
            .into_iter()
            .map(|x| x.0)
            .collect();
        assert_eq!(
            t,
            ["DE", "JA0BZE", "TU", "5NN", "1807M", "R", "UR", "5NN", "134404H"]
        );
        let t: Vec<String> = read("R5NN29012MK R5NN1332MTU", &[])
            .into_iter()
            .map(|x| x.0)
            .collect();
        assert_eq!(t, ["R", "5NN", "29012M", "K", "R", "5NN", "1332M", "TU"]);
        let t: Vec<String> = read("RUR5NN", &[]).into_iter().map(|x| x.0).collect();
        assert_eq!(t, ["R", "UR", "5NN"]);
        // a valid call starting with R is not split
        assert_eq!(read("RA3ABC", &[])[0].0, "RA3ABC");
        let t = read("TI 5TN1807M", &[]);
        assert!(t.iter().all(|x| !x.2.confirmed()), "{t:?}");
    }

    #[test]
    fn doubtful_characters_and_repetition() {
        let t = read("JA1ZLO JA1ZLO", &[2, 9]);
        assert_eq!(t[0].2, Verdict::DoubtfulCharacter);
        assert_eq!(t[1].2, Verdict::Repeated);
        assert!(t[1].2.confirmed());
        let t = read("JA1ZLO JA1ZLO", &[9]);
        assert_eq!((t[0].2, t[1].2), (Verdict::Confirmed, Verdict::Repeated));
    }

    #[test]
    fn run_together_with_a_procedure_word() {
        assert_eq!(verdict("W1AWK"), Verdict::Confirmed); // three-letter suffix W1AW plus K: still a valid call
        assert_eq!(verdict("WB1AWK"), Verdict::Confirmed);
        assert_eq!(verdict("DL1ABCK"), Verdict::RunTogether);
        // a Japanese call cannot have four suffix letters: the K is split off
        let t: Vec<String> = read("JA6LIMK", &[]).into_iter().map(|x| x.0).collect();
        assert_eq!(t, ["JA6LIM", "K"]);
    }

    #[test]
    fn part_of_an_earlier_call() {
        let t = read("JA9MAT JA9MA T", &[]);
        assert_eq!((t[0].2, t[1].2), (Verdict::Confirmed, Verdict::Partial));
    }

    #[test]
    fn a_letter_after_a_space_that_completes_a_call() {
        // DEJA0BZ E: the space before E may be a lost character gap (JA0BZE)
        let t = read("DEJA0BZ E", &[]);
        assert_eq!(
            t[1],
            ("JA0BZ".into(), TokenKind::Callsign, Verdict::RunTogether)
        );
        assert_eq!(t[2].0, "E");
        // a following word that does not complete the call leaves it confirmed
        let t = read("DL1AB 5NN", &[]);
        assert_eq!(t[0].2, Verdict::Confirmed);
        // the verdict waits for the next token; the end of the track releases it
        let mut r = Reader::default();
        assert!(r.feed("DL1AB ".chars().map(|c| (c, false))).is_empty());
        assert_eq!(r.finish()[0].verdict, Verdict::Confirmed);
        // a call that no letter can extend is not held
        let mut r = Reader::default();
        assert_eq!(r.feed("JA1ZLO ".chars().map(|c| (c, false))).len(), 1);
    }

    #[test]
    fn truncated_first_token() {
        let mut r = Reader::default();
        r.truncate_first();
        let mut v = r.feed("F2OZH/2 JF2OZH/2".chars().map(|c| (c, false)));
        v.extend(r.finish());
        assert_eq!(
            (v[0].verdict, v[1].verdict),
            (Verdict::Truncated, Verdict::Confirmed)
        );
    }

    #[test]
    fn positions_count_spaces() {
        let mut r = Reader::default();
        let v = r.feed("CQ DEJA1ZLO ".chars().map(|c| (c, false)));
        let s: Vec<(usize, String)> = v.into_iter().map(|t| (t.start, t.text)).collect();
        assert_eq!(
            s,
            [(0, "CQ".into()), (3, "DE".into()), (5, "JA1ZLO".into())]
        );
    }
}
