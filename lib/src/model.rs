//! Shared tables and constants of the timing model: the speed grid, its prior,
//! the speed kernels, the Morse table and the duration likelihoods.

use crate::special::erfc;
use std::sync::OnceLock;

/// Step of the speed grid in natural log units.
pub const GRID_STEP: f64 = 0.04;
/// Speed range of the grid in words per minute.
pub const WPM_LO: f64 = 6.0;
pub const WPM_HI: f64 = 160.0;
/// Unit length (s) at one word per minute (PARIS timing: 1.2 / WPM).
pub const PARIS: f64 = 1.2;
/// Speed prior: log-normal around 25 WPM with log sd 0.4, mixed with a uniform part.
pub const PRIOR_WPM: f64 = 25.0;
pub const PRIOR_LOG_SD: f64 = 0.4;
pub const PRIOR_PEAK_W: f64 = 0.85;
pub const PRIOR_FLAT_W: f64 = 0.45;
/// Highest speed of the prior of the 40 ms keyer's timing (the short-window keyer's
/// timing uses the whole grid).
pub const PRIOR_TOP: f64 = 100.0;
/// Speed kernels (Laplace scale in log units, half width in grid steps) within words and
/// after word gaps.
pub const KERNEL_IN: (f64, usize) = (0.015, 2);
pub const KERNEL_WORD: (f64, usize) = (0.08, 10);
/// Range of the log-uniform duration null (ratio of longest to shortest duration).
pub const NULL_RANGE: f64 = 300.0;
/// Offset between the log-mean and the mean of exponentially distributed power
/// (Euler's constant rounded to four digits).
#[allow(clippy::approx_constant)]
pub const GAMMA: f64 = 0.5772;
/// Posterior that commits a character at once.
pub const TAU_COMMIT: f64 = 0.99;
/// Largest number of hypothesis groups kept.
pub const MAX_GROUPS: usize = 96;
/// Step (units) and range of the mark/space bias offsets, and the probability of moving
/// to a neighbouring offset at each mark.
pub const BI_STEP: f64 = 0.15;
pub const BI_MAX: i8 = 2;
pub const BI_MOVE: f64 = 0.03;
pub const BI_STAY: f64 = 1.0 - 2.0 * BI_MOVE;
/// Mass reset to the speed prior after a word gap.
pub const RESET_PROB: f64 = 0.02;
pub const RESET_KEEP: f64 = 1.0 - RESET_PROB;
/// Events after which an unresolved character is committed at `TAU_SOFT`, and after
/// which it is committed as a doubtful best guess.
pub const LAG_SOFT: i64 = 6;
pub const LAG_HARD: i64 = 14;
pub const TAU_SOFT: f64 = 0.9;
/// Weight and width factor of the wide (outlier) component of the duration model.
pub const OUTLIER_W: f64 = 0.0001;
pub const OUTLIER_KEEP: f64 = 1.0 - OUTLIER_W;
pub const WIDE_FACTOR: f64 = 4.0;
/// Morse evidence (log-likelihood ratio) required before characters are committed.
pub const LLR_GATE: f64 = 10.0;
/// Prior probability of a displaced mark/space boundary (fading or receiver tails).
pub const SHIFT_P: f64 = 0.03;
/// A silence longer than this many word gaps ends a transmission.
pub const END_K: f64 = 1.5;
/// Smallest posterior of a best guess; below it an erasure ('*') is emitted.
pub const GUESS_MIN: f64 = 0.5;
/// Smallest posterior of the pending text committed at the end of a transmission.
pub const END_MIN: f64 = 0.8;
/// Log sd of dit, dah, element space, character space, word space at scale 1.
pub const SIG_DEF: [f64; 5] = [0.16, 0.16, 0.22, 0.25, 0.3];
/// Gaps observed before the right side of the gap model is no longer widened, and the
/// largest widening.
pub const GAPS_LEARNED: usize = 8;
pub const GAP_WIDEN: f64 = 0.8;
/// Floor of the word-gap likelihood beyond its mean (long silences are word gaps).
pub const WORD_FLOOR: f64 = 0.3;
pub const WORD_FLOOR_SPREAD: f64 = 3.0;
/// Lower bounds of the mean durations in units: dit, element gap, character gap, and
/// word gap relative to the character gap.
pub const DIT_MIN: f64 = 0.2;
pub const IE_MIN: f64 = 0.15;
pub const CG_MIN: f64 = 0.5;
pub const WG_MIN_REL: f64 = 1.25;
/// Prior factor of rare punctuation characters.
pub const RARE_LP: f64 = 0.2;

/// The Morse table: character and code.
pub const MORSE: [(char, &str); 41] = [
    ('A', ".-"),
    ('B', "-..."),
    ('C', "-.-."),
    ('D', "-.."),
    ('E', "."),
    ('F', "..-."),
    ('G', "--."),
    ('H', "...."),
    ('I', ".."),
    ('J', ".---"),
    ('K', "-.-"),
    ('L', ".-.."),
    ('M', "--"),
    ('N', "-."),
    ('O', "---"),
    ('P', ".--."),
    ('Q', "--.-"),
    ('R', ".-."),
    ('S', "..."),
    ('T', "-"),
    ('U', "..-"),
    ('V', "...-"),
    ('W', ".--"),
    ('X', "-..-"),
    ('Y', "-.--"),
    ('Z', "--.."),
    ('0', "-----"),
    ('1', ".----"),
    ('2', "..---"),
    ('3', "...--"),
    ('4', "....-"),
    ('5', "....."),
    ('6', "-...."),
    ('7', "--..."),
    ('8', "---.."),
    ('9', "----."),
    ('/', "-..-."),
    ('?', "..--.."),
    ('=', "-...-"),
    ('.', ".-.-.-"),
    (',', "--..--"),
];

pub struct Tables {
    pub g_u: Vec<f64>,
    pub g_log_u: Vec<f64>,
    pub g_prior: Vec<f64>,
    pub g_prior_fast: Vec<f64>,
    pub k_in: Vec<f64>,
    pub k_word: Vec<f64>,
    /// ln(1 / ln(300)) of the log-uniform null.
    pub null_lp: f64,
}

fn laplace(tau: f64, j: usize) -> Vec<f64> {
    let mut k: Vec<f64> = (0..2 * j + 1)
        .map(|i| (-((i as f64 - j as f64) * GRID_STEP).abs() / tau).exp())
        .collect();
    let s: f64 = k.iter().fold(0.0, |a, &b| a + b);
    for v in &mut k {
        *v /= s;
    }
    k
}

pub fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let mut g_u = Vec::new();
        let mut g_log_u = Vec::new();
        let mut lw = WPM_HI.ln();
        while lw >= WPM_LO.ln() - 1e-9
        /* rounding tolerance */
        {
            let u = PARIS / lw.exp();
            g_u.push(u);
            g_log_u.push(u.ln());
            lw -= GRID_STEP;
        }
        let prior = |top: f64| {
            let n = g_u.iter().filter(|&&u| PARIS / u <= top + 1e-9).count();
            let mut p: Vec<f64> = g_u
                .iter()
                .map(|&u| {
                    let w = PARIS / u;
                    let z = (w / PRIOR_WPM).ln() / PRIOR_LOG_SD;
                    let v = PRIOR_PEAK_W * (-0.5 * z * z).exp() + PRIOR_FLAT_W / n as f64;
                    if w <= top + 1e-9 {
                        v
                    } else {
                        0.0
                    }
                })
                .collect();
            let s: f64 = p.iter().sum();
            for v in &mut p {
                *v /= s;
            }
            p
        };
        let g_prior = prior(PRIOR_TOP);
        let g_prior_fast = prior(WPM_HI);
        Tables {
            g_u,
            g_log_u,
            g_prior,
            g_prior_fast,
            k_in: laplace(KERNEL_IN.0, KERNEL_IN.1),
            k_word: laplace(KERNEL_WORD.0, KERNEL_WORD.1),
            null_lp: (1.0 / NULL_RANGE.ln()).ln(),
        }
    })
}

/// Number of points of the speed grid.
pub const NG: usize = 83;

/// Code of character c ("" if c is not a Morse character).
pub fn code_of(c: char) -> &'static str {
    MORSE.iter().find(|e| e.0 == c).map(|e| e.1).unwrap_or("")
}

/// Character of a code.
pub fn char_of(code: &str) -> Option<char> {
    MORSE.iter().find(|e| e.1 == code).map(|e| e.0)
}

/// Relative prior of a character (rarer punctuation is penalised).
pub fn char_lp(c: char) -> f64 {
    if matches!(c, '?' | '=' | '/' | '.' | ',') {
        RARE_LP
    } else {
        1.0
    }
}

/// Whether a character has at least three elements of both kinds.
pub fn informative(c: char) -> bool {
    let code = code_of(c);
    code.len() >= 3 && code.contains('.') && code.contains('-')
}

/// Mean durations in units (dit, dah, element space, character space, word space).
#[derive(Clone, Copy, Debug)]
pub struct Means {
    pub dit: f64,
    pub dah: f64,
    pub ie: f64,
    pub cg: f64,
    pub wg: f64,
}

pub fn means_b(b0: f64, r: f64, gc: f64, gw: f64, bi: i8) -> Means {
    let b = b0 + BI_STEP * bi as f64;
    let dit = (1.0 + b).max(DIT_MIN);
    let dah = r + b;
    let ie = (1.0 - b).max(IE_MIN);
    let cg = (3.0 * gc - b).max(CG_MIN);
    let wg = (7.0 * gw - b).max(WG_MIN_REL * cg);
    Means {
        dit,
        dah,
        ie,
        cg,
        wg,
    }
}

/// Spread parameters of a duration class: left sd, right sd and normaliser.
#[derive(Clone, Copy)]
pub struct Spread {
    pub sg: f64,
    pub sg_r: f64,
    pub norm: f64,
}

pub fn spread(li: usize, scale: f64, ngaps: usize) -> Spread {
    let sg = SIG_DEF[li] * scale;
    let mut sg_r = sg;
    if li >= 3 && ngaps < GAPS_LEARNED {
        sg_r = sg.max(SIG_DEF[li])
            * (1.0 + GAP_WIDEN * (GAPS_LEARNED - ngaps) as f64 / GAPS_LEARNED as f64);
    }
    Spread {
        sg,
        sg_r,
        norm: 2.0 / (sg + sg_r),
    }
}

/// Likelihood (or survival if censored) of the log-ratio x = ln d - ln mu - ln u.
#[inline]
pub fn lik_x(x: f64, sp: Spread, censored: bool, word: bool) -> f64 {
    let (sg, sg_r) = (sp.sg, sp.sg_r);
    let s = if x > 0.0 { sg_r } else { sg };
    let z = x / s;
    let sqrt2 = std::f64::consts::SQRT_2;
    if censored {
        let core = if x > 0.0 {
            sg_r / (sg + sg_r) * erfc(z / sqrt2)
        } else {
            1.0 - sg / (sg + sg_r) * erfc(-z / sqrt2)
        };
        let mut p = OUTLIER_KEEP * core + OUTLIER_W * 0.5 * erfc(x / (WIDE_FACTOR * sg * sqrt2));
        if word && x > 0.0 {
            p = p.max(WORD_FLOOR);
        }
        p
    } else {
        let mut p = OUTLIER_KEEP * (-0.5 * z * z).exp() * sp.norm
            + OUTLIER_W * (-0.5 * x * x / (WIDE_FACTOR * WIDE_FACTOR * sg * sg)).exp()
                / (sg * WIDE_FACTOR);
        if word && x > 0.0 {
            p = p.max(WORD_FLOOR / (sg * WORD_FLOOR_SPREAD));
        }
        p
    }
}

/// Timing summary used by the keyers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hyp {
    /// Posterior mean unit in seconds.
    pub u: f64,
    /// Dah/dit ratio.
    pub r: f64,
    /// Mark lengthening in units.
    pub b: f64,
    /// Character gap in units.
    pub cg: f64,
}

/// One alternative character with its posterior mass.
#[derive(Clone, Debug, PartialEq)]
pub struct Alt {
    pub ch: char,
    pub p: f64,
}

/// Metadata of one emitted character.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CharMeta {
    /// Posterior mass of the character when it was committed.
    pub conf: f64,
    /// Forced best guess (posterior below the staged commit threshold).
    pub doubt: bool,
    /// Up to three alternatives including the chosen character.
    pub alts: Vec<Alt>,
    /// Features of the calibrator, set when the character is emitted.
    pub feat: [f32; crate::calib::NF],
}

/// One hypothesis group of the uncommitted text of a track.
#[derive(Clone, Debug, PartialEq)]
pub struct Guess {
    /// Uncommitted characters before the open character.
    pub text: String,
    /// Dot/dash pattern of the open character.
    pub pattern: String,
    /// The open character if it ended now ("" if the pattern is not a valid code).
    pub ch: String,
    /// Probability of this group (calibrated in events and track states).
    pub p: f64,
}

/// Rounds to 1e-3 and keeps the first n entries, dropping those that round to zero (except the first).
pub fn top_alts(mass: &[(char, f64)], n: usize) -> Vec<Alt> {
    let mut a: Vec<Alt> = mass.iter().map(|&(ch, p)| Alt { ch, p }).collect();
    a.sort_by(|x, y| {
        y.p.partial_cmp(&x.p)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(x.ch.cmp(&y.ch))
    });
    let mut out = Vec::new();
    for (i, mut x) in a.into_iter().enumerate() {
        x.p = (x.p * 1000.0).round() / 1000.0;
        if i >= n || (i > 0 && x.p == 0.0) {
            break;
        }
        out.push(x);
    }
    out
}

/// Sorts guess groups (best first), rounds and keeps at most n.
pub fn finish_guesses(mut a: Vec<Guess>, n: usize) -> Vec<Guess> {
    a.sort_by(|x, y| {
        y.p.partial_cmp(&x.p)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                (x.text.clone() + "|" + &x.pattern).cmp(&(y.text.clone() + "|" + &y.pattern))
            })
    });
    let mut out = Vec::new();
    for (i, mut g) in a.into_iter().enumerate() {
        g.p = (g.p * 1000.0).round() / 1000.0;
        if i >= n || (i > 0 && g.p == 0.0) {
            break;
        }
        out.push(g);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_and_prior() {
        let t = tables();
        assert_eq!(t.g_u.len(), NG);
        assert!((1.2 / t.g_u[0] - WPM_HI).abs() < 1e-9);
        assert!(1.2 / t.g_u[NG - 1] >= 6.0 - 1e-6);
        let s: f64 = t.g_prior.iter().sum();
        assert!((s - 1.0).abs() < 1e-12);
        let k: f64 = t.k_word.iter().sum();
        assert!((k - 1.0).abs() < 1e-12 && t.k_in.len() == 5 && t.k_word.len() == 21);
    }

    #[test]
    fn morse_table() {
        assert_eq!(code_of('K'), "-.-");
        assert_eq!(char_of("..--.."), Some('?'));
        assert_eq!(char_of("......"), None);
        assert!(informative('K') && informative('R') && !informative('S') && !informative('5'));
        assert!(!informative('A') && informative('J'));
        assert_eq!(char_lp('/'), 0.2);
        assert_eq!((char_lp('.'), char_lp(','), char_lp('A')), (0.2, 0.2, 1.0));
        assert_eq!(char_of(".-.-.-"), Some('.'));
        assert!(informative(','));
    }

    #[test]
    fn likelihoods() {
        let sp = spread(0, 1.0, 0);
        // density peaks at x = 0 and decays symmetrically for marks
        assert!(lik_x(0.0, sp, false, false) > lik_x(0.2, sp, false, false));
        assert!((lik_x(0.2, sp, false, false) - lik_x(-0.2, sp, false, false)).abs() < 1e-12);
        // survival decreases with the duration and is 0.5 at the median
        let sp = spread(3, 1.0, 10);
        assert!((lik_x(0.0, sp, true, false) - 0.5).abs() < 1e-3);
        assert!(lik_x(-0.5, sp, true, false) > lik_x(0.5, sp, true, false));
        assert!(lik_x(5.0, spread(4, 1.0, 10), true, true) >= 0.3);
        // the right side is widened while the gap model is unlearned
        let w = spread(3, 1.0, 0);
        assert!(w.sg_r > w.sg);
    }

    #[test]
    fn means_respect_bounds() {
        let m = means_b(0.0, 3.0, 1.0, 1.0, 0);
        assert_eq!((m.dit, m.dah, m.ie, m.cg, m.wg), (1.0, 3.0, 1.0, 3.0, 7.0));
        let m = means_b(0.6, 3.0, 1.0, 1.0, 2);
        assert!(m.ie >= 0.15 && m.wg >= 1.25 * m.cg);
    }

    #[test]
    fn alternatives_are_sorted_and_rounded() {
        let a = top_alts(&[('B', 0.0004), ('A', 0.9), ('C', 0.0996)], 3);
        assert_eq!(a.len(), 2);
        assert_eq!((a[0].ch, a[0].p, a[1].ch, a[1].p), ('A', 0.9, 'C', 0.1));
    }
}
