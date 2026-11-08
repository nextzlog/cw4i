//! Token re-segmentation of committed text for the token reader (the committed text is never
//! changed): a dynamic programme over the committed character stream of a
//! track scores tokens with a contest grammar (callsigns, numbers, procedure words) and
//! charges for removing a decoded word space or adding a missing one. A signal report
//! followed by a number always stands alone (TU5NN11H reads TU 5NN 11H). An invalid token
//! with a digit is repaired to a Japanese callsign or a number when one lost character gap or
//! element explains it (13SM2M reads 1332M).

use crate::model;

/// Procedure words and abbreviations of contest exchanges.
const PROCEDURE: &[&str] = &[
    "CQ", "TEST", "UR", "5NN", "599", "TU", "R", "DE", "K", "QSL", "BK", "NR", "QRZ", "AGN", "73",
    "QSO", "B4",
];
/// Signal reports that anchor an exchange.
const REPORT: &[&str] = &["5NN", "599"];
/// Characters a report character is often misread as (one element lost, added or merged).
const CONFUSED: &[(u8, &[u8])] = &[(b'5', b"H4S"), (b'N', b"TADI"), (b'9', b"0O8")];
/// Scores of token kinds and of an unknown token (per token and per character).
const S_JA_CALL: f64 = 4.0;
const S_CALL: f64 = 2.0;
const S_PROC: f64 = 3.0;
const S_NUM: f64 = 2.0;
const S_UNKNOWN: f64 = -1.0;
const S_UNKNOWN_CHAR: f64 = -0.25;
/// Costs of removing a decoded space and of adding a missing one.
const MERGE_COST: f64 = 3.0;
const SPLIT_COST: f64 = 3.5;
/// Costs of a repair: a lost character gap (SM read for 3) and a lost or extra element.
const GAP_COST: f64 = 1.5;
const ELEMENT_COST: f64 = 1.5;
/// Most character edits between a fused callsign and its earlier copy.
const FUSE_EDITS: usize = 2;
/// Longest token considered.
const MAX_TOKEN: usize = 10;
/// Characters that must follow a token before it is released.
pub const HOLD: usize = 6;

/// Japanese callsign: JA-JS (not JB, JC), 7J-7N (7K-7N only in areas 1-4) or 8J-8N, an area
/// digit and a suffix of 2 or 3 letters, optionally portable /digit or /P.
fn ja_call(t: &[u8]) -> bool {
    let core = match t.iter().position(|&c| c == b'/') {
        Some(i) if t.len() == i + 2 && (t[i + 1].is_ascii_digit() || t[i + 1] == b'P') => &t[..i],
        Some(_) => return false,
        None => t,
    };
    if !(5..=6).contains(&core.len()) || !core[2].is_ascii_digit() {
        return false;
    }
    let prefix = match core[0] {
        b'J' => (b'A'..=b'S').contains(&core[1]) && !matches!(core[1], b'B' | b'C'),
        b'7' => {
            (b'J'..=b'N').contains(&core[1])
                && (core[1] == b'J' || (b'1'..=b'4').contains(&core[2]))
        }
        b'8' => (b'J'..=b'N').contains(&core[1]),
        _ => false,
    };
    prefix && core[3..].iter().all(|c| c.is_ascii_uppercase())
}

fn call(t: &[u8]) -> bool {
    let base: &[u8] = match t.iter().position(|&c| c == b'/') {
        Some(i) if i >= 3 => &t[..i],
        Some(_) => return false,
        None => t,
    };
    if !(3..=7).contains(&base.len()) {
        return false;
    }
    let digits: Vec<usize> = (0..base.len())
        .filter(|&i| base[i].is_ascii_digit())
        .collect();
    let Some(&d) = digits.last() else {
        return false;
    };
    (1..=3).contains(&d)
        && d + 1 < base.len()
        && base[d + 1..].iter().all(|c| c.is_ascii_uppercase())
        && base[..d].iter().any(|c| c.is_ascii_uppercase())
}

fn cut_digit(c: u8) -> bool {
    c.is_ascii_digit() || matches!(c, b'O' | b'A' | b'N' | b'T')
}

/// Exchange number: 2 to 6 digits (area, JCC, JCG or ku code; cut digits allowed) and an
/// optional power letter.
fn number(t: &[u8]) -> bool {
    let body = match t.last() {
        Some(b'P' | b'L' | b'M' | b'H') if t.len() > 1 => &t[..t.len() - 1],
        _ => t,
    };
    (2..=6).contains(&body.len())
        && body.iter().all(|&c| cut_digit(c))
        && body.iter().any(|c| c.is_ascii_digit())
}

/// Grammar score of one token (punctuation at its ends is ignored).
pub fn token_score(t: &[u8]) -> f64 {
    let a = t
        .iter()
        .position(|c| c.is_ascii_alphanumeric())
        .unwrap_or(t.len());
    let b = t
        .iter()
        .rposition(|c| c.is_ascii_alphanumeric())
        .map_or(a, |b| b + 1);
    let t = &t[a..b];
    if PROCEDURE.iter().any(|p| p.as_bytes() == t) {
        S_PROC
    } else if ja_call(t) {
        S_JA_CALL
    } else if call(t) {
        S_CALL
    } else if number(t) {
        S_NUM
    } else {
        S_UNKNOWN + S_UNKNOWN_CHAR * t.len() as f64
    }
}

/// Japanese callsign or number reached from a token with a digit that is neither by the
/// cheapest repair: one lost character gap, one lost or extra element, or both. Returns the
/// repaired text and its score after the repair cost; none when the cheapest repairs disagree.
fn repair(t: &[u8]) -> Option<(Vec<u8>, f64)> {
    if !t.iter().any(|c| c.is_ascii_digit())
        || ja_call(t)
        || number(t)
        || PROCEDURE.iter().any(|p| p.as_bytes() == t)
    {
        return None;
    }
    let best = variants(t)?.into_iter().find(|l| !l.is_empty())?;
    let top = best.iter().map(|c| c.1).fold(f64::NEG_INFINITY, f64::max);
    let mut winners: Vec<&(Vec<u8>, f64)> = best.iter().filter(|c| c.1 == top).collect();
    winners.sort_by(|a, b| a.0.cmp(&b.0));
    winners.dedup_by(|a, b| a.0 == b.0);
    (winners.len() == 1).then(|| winners[0].clone())
}

/// Japanese callsigns and numbers reached from `t` by one lost character gap (level 0), one
/// lost or extra element (level 1) or both (level 2), with their scores after the cost.
fn variants(t: &[u8]) -> Option<[Vec<Scored>; 3]> {
    let codes: Vec<&str> = t.iter().map(|&c| model::code_of(c as char)).collect();
    if codes.iter().any(|c| c.is_empty()) {
        return None;
    }
    let target = |w: &[u8]| (ja_call(w) || number(w)).then(|| token_score(w));
    let mut levels: [Vec<Scored>; 3] = Default::default();
    for gap in 0..t.len() {
        let mut base: Vec<String> = codes.iter().map(|c| c.to_string()).collect();
        if gap > 0 {
            let c = base.remove(gap);
            base[gap - 1].push_str(&c);
        }
        let mut edits = vec![base.clone()];
        for i in 0..base.len() {
            for j in 0..=base[i].len() {
                for e in ['.', '-'] {
                    let mut v = base.clone();
                    v[i].insert(j, e);
                    edits.push(v);
                }
                if j < base[i].len() && base[i].len() > 1 {
                    let mut v = base.clone();
                    v[i].remove(j);
                    edits.push(v);
                }
            }
        }
        for (k, v) in edits.into_iter().enumerate() {
            if gap == 0 && k == 0 {
                continue;
            }
            let w: Option<Vec<u8>> = v
                .iter()
                .map(|c| model::char_of(c).map(|c| c as u8))
                .collect();
            if let Some(s) = w.as_deref().and_then(target) {
                let level = usize::from(k > 0) + usize::from(gap > 0 && k > 0);
                let cost =
                    if gap > 0 { GAP_COST } else { 0.0 } + if k > 0 { ELEMENT_COST } else { 0.0 };
                levels[level].push((w.unwrap(), s - cost));
            }
        }
    }
    Some(levels)
}

/// A candidate text and its score.
type Scored = (Vec<u8>, f64);

/// Characters of earlier text kept per track to check a callsign against an earlier copy.
pub const HISTORY: usize = 24;

/// Dots and dashes of `w` with a space between characters.
fn elements(w: &[u8]) -> Vec<u8> {
    let codes: Vec<&str> = w.iter().map(|&c| model::code_of(c as char)).collect();
    codes.join(" ").into_bytes()
}

/// Fewest edits turning `w` into some stretch of `prev`.
fn distance_within(w: &[u8], prev: &[u8]) -> usize {
    let mut d: Vec<usize> = vec![0; prev.len() + 1];
    for (i, &c) in w.iter().enumerate() {
        let mut next = vec![i + 1; prev.len() + 1];
        for j in 1..=prev.len() {
            next[j] = (d[j] + 1)
                .min(next[j - 1] + 1)
                .min(d[j - 1] + usize::from(prev[j - 1] != c));
        }
        d = next;
    }
    d.into_iter().min().unwrap_or(w.len())
}

/// Japanese callsign that one lost character gap or element away from `t` matches an earlier
/// copy in `prev` (the station's earlier text without spaces) better than `t` does, such as
/// JA1ZLO for a garbled second copy after JA1ZLO.
pub fn fuse(prev: &[u8], t: &[u8]) -> Option<Vec<u8>> {
    if prev.is_empty() || t.len() < 4 || !t.iter().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let own = if ja_call(t) {
        distance_within(t, prev)
    } else {
        usize::MAX
    };
    let mut best: Vec<(usize, Vec<u8>)> = variants(t)?
        .into_iter()
        .take(2)
        .flatten()
        .filter(|c| ja_call(&c.0))
        .map(|c| (distance_within(&c.0, prev), c.0))
        .collect();
    let low = best.iter().map(|c| c.0).min()?;
    if low > FUSE_EDITS || low >= own {
        return None;
    }
    best.retain(|c| c.0 == low);
    // ties are broken by the dots and dashes of the earlier copy
    let pe = elements(prev);
    let mut best: Vec<(usize, Vec<u8>)> = best
        .into_iter()
        .map(|c| (distance_within(&elements(&c.1), &pe), c.1))
        .collect();
    let low = best.iter().map(|c| c.0).min()?;
    best.retain(|c| c.0 == low);
    best.sort();
    best.dedup();
    (best.len() == 1).then(|| best.remove(0).1)
}

/// Whether `w` is a report, or one with a single commonly misread character (5TN, H99) when
/// a real digit follows.
fn report_at(w: &[u8], next: u8) -> bool {
    REPORT.iter().any(|r| {
        let r = r.as_bytes();
        let diff: Vec<usize> = (0..3).filter(|&i| w[i] != r[i]).collect();
        match diff[..] {
            [] => cut_digit(next),
            [i] => {
                next.is_ascii_digit()
                    && CONFUSED
                        .iter()
                        .any(|(c, alts)| *c == r[i] && alts.contains(&w[i]))
            }
            _ => false,
        }
    })
}

/// Boundaries forced around each report (or near-miss report) followed by a number, unless
/// the report lies inside a Japanese callsign (JA5NNA).
fn anchors(chars: &[u8]) -> Vec<bool> {
    let n = chars.len();
    let mut cut = vec![false; n + 1];
    for p in 0..n.saturating_sub(3) {
        let q = p + 3;
        if !report_at(&chars[p..q], chars[q]) {
            continue;
        }
        let in_call = (p.saturating_sub(5)..=p)
            .any(|i| (q.max(i + 5)..=(i + 8).min(n)).any(|j| ja_call(&chars[i..j])));
        if !in_call {
            cut[p] = true;
            cut[q] = true;
        }
    }
    cut
}

/// Best tokens of `chars` (no spaces); `space[i]` is true when the decoder put a word space
/// after character i. Returns the end index (exclusive) of each token and its repaired text.
pub fn segment(chars: &[u8], space: &[bool]) -> Vec<(usize, Option<Vec<u8>>)> {
    let n = chars.len();
    let forced = anchors(chars);
    let mut best = vec![f64::NEG_INFINITY; n + 1];
    let mut from = vec![0usize; n + 1];
    let mut fixed: Vec<Option<Vec<u8>>> = vec![None; n + 1];
    best[0] = 0.0;
    for j in 1..=n {
        for i in j.saturating_sub(MAX_TOKEN)..j {
            if best[i] == f64::NEG_INFINITY || (i + 1..j).any(|k| forced[k]) {
                continue;
            }
            let split = j < n && !space[j - 1];
            if split
                && !forced[j]
                && !(chars[j - 1].is_ascii_alphanumeric() && chars[j].is_ascii_alphanumeric())
            {
                continue;
            }
            let merged = (i..j - 1).filter(|&k| space[k]).count() as f64;
            let split = if split && !forced[j] { 1.0 } else { 0.0 };
            let plain = token_score(&chars[i..j]);
            let (score, text) = match repair(&chars[i..j]) {
                Some((t, s)) if s > plain => (s, Some(t)),
                _ => (plain, None),
            };
            let v = best[i] + score - MERGE_COST * merged - SPLIT_COST * split;
            if v > best[j] {
                best[j] = v;
                from[j] = i;
                fixed[j] = text;
            }
        }
        if best[j] == f64::NEG_INFINITY {
            // a run longer than MAX_TOKEN without a decoded space: keep the decoder's split
            best[j] = best[j - 1] + S_UNKNOWN_CHAR;
            from[j] = j - 1;
            fixed[j] = None;
        }
    }
    let mut ends = Vec::new();
    let mut j = n;
    while j > 0 {
        ends.push((j, fixed[j].take()));
        j = from[j];
    }
    ends.reverse();
    // an invalid word just before a Japanese callsign that is DE with one element changed
    let mut a = 0;
    for i in 0..ends.len().saturating_sub(1) {
        let e = ends[i].0;
        let next = ends[i + 1]
            .1
            .clone()
            .unwrap_or_else(|| chars[e..ends[i + 1].0].to_vec());
        let word = &chars[a..e];
        if ends[i].1.is_none()
            && token_score(word) < 0.0
            && ja_call(&next)
            && one_element(word, b"DE")
        {
            ends[i].1 = Some(b"DE".to_vec());
        }
        a = e;
    }
    ends
}

/// Whether `w` becomes `to` by changing, adding or removing one dot or dash of one character.
fn one_element(w: &[u8], to: &[u8]) -> bool {
    if w.len() != to.len() {
        return false;
    }
    let diff: Vec<usize> = (0..w.len()).filter(|&i| w[i] != to[i]).collect();
    let [i] = diff[..] else {
        return false;
    };
    let (x, y) = (
        model::code_of(w[i] as char).as_bytes(),
        model::code_of(to[i] as char).as_bytes(),
    );
    if x.is_empty() || y.is_empty() {
        return false;
    }
    let same = |a: &[u8], b: &[u8]| a == b;
    match x.len() as i64 - y.len() as i64 {
        0 => (0..x.len()).filter(|&k| x[k] != y[k]).count() == 1,
        1 => (0..x.len()).any(|k| same(&[&x[..k], &x[k + 1..]].concat(), y)),
        -1 => (0..y.len()).any(|k| same(&[&y[..k], &y[k + 1..]].concat(), x)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(s: &str) -> String {
        let mut chars = Vec::new();
        let mut space = Vec::new();
        for c in s.bytes() {
            if c == b' ' {
                if let Some(l) = space.last_mut() {
                    *l = true;
                }
            } else {
                chars.push(c);
                space.push(false);
            }
        }
        let ends = segment(&chars, &space);
        let mut out = Vec::new();
        let mut a = 0;
        for (e, r) in ends {
            out.push(String::from_utf8(r.unwrap_or_else(|| chars[a..e].to_vec())).unwrap());
            a = e;
        }
        out.join(" ")
    }

    #[test]
    fn splits_and_merges_contest_tokens() {
        assert_eq!(seg("DEJA1ZLO"), "DE JA1ZLO");
        assert_eq!(seg("JA 1ZLO"), "JA1ZLO");
        assert_eq!(seg("CQ TEST JA1ZLO"), "CQ TEST JA1ZLO");
        assert_eq!(seg("5NN1807M"), "5NN 1807M");
    }

    #[test]
    fn report_anchors_the_exchange() {
        assert_eq!(seg("TU5NN11H"), "TU 5NN 11H");
        assert_eq!(seg("R5NN11H"), "R 5NN 11H");
        assert_eq!(seg("RUR5NN11H"), "R UR 5NN 11H");
        assert_eq!(seg("R UR5NN11H"), "R UR 5NN 11H");
        assert_eq!(seg("TU5NN100110H"), "TU 5NN 100110H");
        assert_eq!(seg("UR5NN100116L"), "UR 5NN 100116L");
        assert_eq!(seg("TU5NNAO"), "TU 5NN AO");
        assert_eq!(seg("JA5NNA 5NN13M"), "JA5NNA 5NN 13M");
        assert_eq!(seg("TI 5TN1807M"), "TI 5TN 1807M");
        assert_eq!(seg("H5TNK"), "H5TNK");
    }

    #[test]
    fn repairs_split_or_broken_characters() {
        assert_eq!(seg("R5NN13SM2M TU"), "R 5NN 1332M TU");
        // 2 (..---) and 3 (...--) are equally near, so the token is left alone
        assert_eq!(seg("JAIMZLO"), "JAIMZLO");
        assert_eq!(seg("JA1ZLO"), "JA1ZLO");
        // a Japanese contest has no callsign starting with O: J lost its dit
        assert_eq!(seg("OA1ZLO"), "JA1ZLO");
        assert_eq!(seg("CQ TEST"), "CQ TEST");
        // S (...) is D (-..) with its dash read as a dot
        assert_eq!(seg("SEJA1ZLO"), "DE JA1ZLO");
        assert_eq!(seg("SE QRZ"), "SE QRZ");
    }

    #[test]
    fn fuses_a_callsign_with_its_earlier_copy() {
        assert_eq!(fuse(b"CQJA1ZLO", b"OA1ZLO"), Some(b"JA1ZLO".to_vec()));
        assert_eq!(fuse(b"JA1ZLO", b"JA1ZLO"), None);
        assert_eq!(fuse(b"", b"OA1ZLO"), None);
        assert_eq!(fuse(b"CQTEST", b"JAIMZLO"), None);
    }

    #[test]
    fn leaves_plain_text_alone() {
        assert_eq!(seg("NAME IS TARO"), "NAME IS TARO");
        assert_eq!(seg("HW CPY"), "HW CPY");
        assert_eq!(seg("TEST, DE JA1ZLO."), "TEST, DE JA1ZLO.");
    }
}
