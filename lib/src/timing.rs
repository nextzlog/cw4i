//! Timing tracker: a speed grid inside a Morse-tree hypothesis search.
//!
//! Each discrete hypothesis (Morse tree node, uncommitted text, word context and
//! mark/space bias offset) carries a probability vector over a log-spaced grid of the
//! unit u (half a dit period). Speed moves only at mark onsets: a narrow Laplace kernel
//! inside words, a wide kernel plus a reset to the prior after word gaps. Durations are
//! scored in log units against dit 1+b, dah r+b, element space 1-b, character space
//! 3gc-b and word space 7gw-b. b, r, gc, gw and a jitter scale are learned only from
//! committed characters. A character is committed when its posterior (marginalised over
//! speed and segmentation) exceeds 0.99, or after a number of events at lower thresholds
//! (doubtful best guesses); an open gap is used as a censored observation so that
//! commits do not wait for the next mark. The Morse tree carries a tempered character
//! frequency prior, so that ambiguous gaps favour whole, likely characters, and the
//! first character of a track is dropped when it may be the tail of a cut-off one.
//!
//! Hypothesis keys are packed integers with interned texts, probability vectors are
//! fixed-size arrays, and the shifted space likelihoods are shared, cached and pruned by
//! an exact bound, so that the results equal those of a direct evaluation.

use crate::model::*;
use crate::syntax;
use crate::Element;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// Initial unit (s): 25 words per minute.
const INITIAL_UNIT: f64 = 0.048;
/// Initial dah ratio.
const INITIAL_RATIO: f64 = 3.0;
/// Initial forgotten sums and weights of the dit, element-gap and dah durations (units).
const INITIAL_STATS: [[f64; 2]; 3] = [[6.0, 6.0], [6.0, 6.0], [18.0, 6.0]];
/// Half width (log units) of the speed window and the posterior mass inside it that
/// make the speed estimate unambiguous.
const SPEED_WINDOW: f64 = 0.15;
const SPEED_CONFIDENT: f64 = 0.95;
/// Ambiguity values reported for a confident and an uncertain speed estimate.
const AMBIG_CONFIDENT: f64 = 10.0;
const AMBIG_UNCERTAIN: f64 = 0.0;
/// Ambiguity above which the speed estimate is trusted for word spaces.
const AMBIG_TRUSTED: f64 = 3.0;
/// Bound of one element's log-likelihood ratio and forgetting of the Morse evidence.
const LLR_STEP_MAX: f64 = 4.0;
const LLR_FORGET: f64 = 0.93;
/// Number of character-terminating gaps remembered for the gap model.
const GAP_MEMORY: usize = 40;
/// Forgetting of the duration statistics; dits and element gaps adapt faster.
const LEARN_FORGET: f64 = 0.95;
const LEARN_FORGET_FAST: f64 = 0.8;
/// Range of observed durations (units) used for learning.
const LEARN_MIN: f64 = 0.2;
const LEARN_MAX: f64 = 8.0;
/// Bound of one normalised residual in the jitter estimate.
const RESIDUAL_MAX: f64 = 3.0;
/// Prior pseudo-counts of the jitter scale and its range.
const SCALE_PRIOR_SUM: f64 = 8.0;
const SCALE_PRIOR_N: f64 = 10.0;
const SCALE_NORM: f64 = 0.8;
const SCALE_MIN: f64 = 0.35;
const SCALE_MAX: f64 = 1.6;
/// Range of the mark lengthening (units) and of the dah ratio.
const BIAS_MAX: f64 = 0.6;
const RATIO_MIN: f64 = 2.2;
const RATIO_MAX: f64 = 6.5;
/// Interned texts kept before unused ones are dropped.
const TEXTS_GC: usize = 4096;
/// Hypotheses below this fraction of the total mass are dropped.
const PRUNE_REL: f64 = 1e-9;
/// Number of boundary shifts tried and their step in units.
const N_SHIFTS: usize = 8;
const SHIFT_STEP: f64 = 0.25;
/// Shortest shifted mark and shortest remaining space in units.
const SHIFT_MARK_MIN: f64 = 0.3;
const SHIFT_SPACE_MIN: f64 = 0.2;
/// Relative safety margin of the pruning bounds (covers rounding).
const BOUND_MARGIN: f64 = 1.000_001;
const BOUND_MARGIN_TIGHT: f64 = 1.000_000_001;
/// Power applied to the character frequencies in the Morse-tree prior (0 = flat).
const PRIOR_POWER: f64 = 0.3;
/// Prior factor of one-character words.
const ONE_CHAR_WORD: f64 = 0.25;
/// Shortest element duration (s).
const ELEMENT_MIN: f64 = 0.002;
/// Fraction of a character's mass that must be preceded by a word gap to emit a space.
const SPACE_FRAC: f64 = 0.5;
/// Shortest silence (s) that ends a transmission.
const END_MIN_SEC: f64 = 0.6;
/// The open gap is evaluated after this many units, and again every `CENSOR_STEP` units.
const CENSOR_START: f64 = 1.6;
const CENSOR_STEP: f64 = 1.0;
/// Smallest log-domain gap considered and smallest cluster spread in the gap model.
const GAP_LOG_MIN: f64 = 0.3;
const GAP_SD_MIN: f64 = 0.03;
/// Smallest separation of the two gap clusters (ratio, and multiple of their spread).
const GAP_SPLIT_RATIO: f64 = 1.15;
const GAP_SPLIT_SD: f64 = 3.0;
/// Smallest word gap relative to the character gap when only one cluster is found.
const WORD_GAP_MIN_REL: f64 = 1.6;
/// Ranges of the learned character and word gap factors.
const GC_MIN: f64 = 0.6;
const GW_MIN: f64 = 0.4;
const G_MAX: f64 = 9.0;
/// Nominal character and word gaps in units.
const CHAR_GAP: f64 = 3.0;
const WORD_GAP: f64 = 7.0;
/// Smallest likelihood used as a divisor.
const LIK_MIN: f64 = 1e-300;
/// Characters of emitted text matched when another timing takes over.
const ALIGN_CHARS: usize = 6;
const ALIGN_SKIP: usize = 2;
/// Marginal speed mass, relative to its peak, below which grid points are dropped.
const WIN_EPS: f64 = 1e-9;
/// Smallest duration (s) in a log.
const DUR_MIN: f64 = 1e-4;

/// Emitted text, learned parameters and bookkeeping of the timing tracker.
pub struct Common {
    pub best: Hyp,
    pub ambig: f64,
    pub out: Vec<char>,
    pub held: Vec<char>,
    pub spaced: bool,
    pub nchars: i64,
    pub b: f64,
    pub r: f64,
    pub gc: f64,
    pub gw: f64,
    pub scale: f64,
    pub st: [[f64; 2]; 3],
    pub res: [f64; 2],
    pub gaps: Vec<f64>,
    pub evs: Vec<Element>,
    /// Per event of evs: whether a boundary lay under a louder station's keying edge.
    pub unsure: Vec<bool>,
    pub lu: Vec<f64>,
    pub started: bool,
    pub ahead: Vec<char>,
    pub last_cens: f64,
    pub confirmed_now: bool,
    pub llr: f64,
    pub pend_since: i64,
    pub ev_base: i64,
    pub nemit: i64,
    pub last_mark: f64,
    pub ended: bool,
    pub cur: CharMeta,
    pub held_meta: Vec<CharMeta>,
    pub out_meta: Vec<CharMeta>,
    pub inited: bool,
    /// Silence (s) before the first mark of the track.
    pub lead: f64,
    pub fast_prior: bool,
    /// Whether the first character has been decided.
    pub first_done: bool,
    /// Whether the first character was dropped as a possible cut-off tail and the next
    /// one followed it without a word space (the first token may be incomplete).
    pub first_dropped: bool,
    /// Whether a clearly stronger confirmed station is near (set by the decoder).
    pub outshone: bool,
    /// Key-down SNR of the track (dB, set by the decoder), how the character being
    /// emitted was committed (0 at a space, 1 during an open gap, 2 at the end of a
    /// transmission) and how many events it waited.
    pub snr_db: f64,
    /// Smoothed key-down power of the track against its recent peak (dB, set by the
    /// decoder; low in a fade).
    pub fade_db: f64,
    pub path: u8,
    pub age: i64,
    /// Whether a much stronger confirmed station is very near (set by the decoder): then
    /// characters below the full commit threshold, or keyed under its edges, are held
    /// back like doubtful ones.
    pub guarded: bool,
    /// Whether the next element has a boundary under a louder station's keying edge (set
    /// by the decoder).
    pub next_unsure: bool,
    /// Whether the track's look-back reaches the start of the stream (set by the decoder).
    pub at_start: bool,
    /// A first character after a short silence at the stream start, held until its
    /// token closes.
    pub first_hold: Option<(char, CharMeta)>,
}

impl Common {
    pub fn new() -> Common {
        Common {
            best: Hyp::default(),
            ambig: 0.0,
            out: Vec::new(),
            held: Vec::new(),
            spaced: false,
            nchars: 0,
            b: 0.0,
            r: 0.0,
            gc: 0.0,
            gw: 0.0,
            scale: 0.0,
            st: [[0.0; 2]; 3],
            res: [0.0; 2],
            gaps: Vec::new(),
            evs: Vec::new(),
            unsure: Vec::new(),
            lu: Vec::new(),
            started: false,
            ahead: Vec::new(),
            last_cens: 0.0,
            confirmed_now: false,
            llr: 0.0,
            pend_since: 0,
            ev_base: 0,
            nemit: 0,
            last_mark: 0.0,
            ended: false,
            cur: CharMeta::default(),
            held_meta: Vec::new(),
            out_meta: Vec::new(),
            inited: false,
            lead: 0.0,
            fast_prior: false,
            first_done: false,
            first_dropped: false,
            outshone: false,
            snr_db: 0.0,
            fade_db: 0.0,
            path: 0,
            age: 0,
            guarded: false,
            next_unsure: false,
            at_start: false,
            first_hold: None,
        }
    }

    /// Initial parameters; returns false if already initialised.
    pub fn init(&mut self) -> bool {
        if self.inited {
            return false;
        }
        self.inited = true;
        self.b = 0.0;
        self.r = INITIAL_RATIO;
        self.gc = 1.0;
        self.gw = 1.0;
        self.scale = 1.0;
        self.pend_since = -1;
        self.st = INITIAL_STATS;
        self.best.u = INITIAL_UNIT;
        true
    }

    pub fn means(&self) -> Means {
        means_b(self.b, self.r, self.gc, self.gw, 0)
    }

    pub fn means_b(&self, bi: i8) -> Means {
        means_b(self.b, self.r, self.gc, self.gw, bi)
    }

    /// Posterior summary from the marginal speed distribution p.
    pub fn posterior_stats(&mut self, p: &[f64; NG]) {
        let tb = tables();
        let (mut s, mut m) = (0.0, 0.0);
        for i in 0..NG {
            s += p[i];
            m += p[i] * tb.g_log_u[i];
        }
        if s <= 0.0 {
            return;
        }
        m /= s;
        self.best.u = m.exp();
        self.best.r = self.r;
        self.best.b = self.b;
        self.best.cg = CHAR_GAP * self.gc;
        let mut inw = 0.0;
        for i in 0..NG {
            if (tb.g_log_u[i] - m).abs() < SPEED_WINDOW {
                inw += p[i];
            }
        }
        self.ambig = if inw / s > SPEED_CONFIDENT {
            AMBIG_CONFIDENT
        } else {
            AMBIG_UNCERTAIN
        };
    }

    /// Morse-presence evidence update from the predictive mass of an element.
    pub fn update_llr(&mut self, pred: f64) {
        let l = pred.max(LIK_MIN).ln() - tables().null_lp;
        let l = l.clamp(-LLR_STEP_MAX, LLR_STEP_MAX);
        self.llr = LLR_FORGET * self.llr + l;
    }

    /// Calibrator features of an uncommitted guess of probability p (no commit path).
    pub fn guess_features(&self, p: f64) -> [f32; crate::calib::NF] {
        let margin = ((p + 1e-3) / (1.0 - p + 1e-3)).ln().clamp(-5.0, 10.0);
        let f = [
            crate::calib::logit(p),
            0.0,
            0.0,
            0.0,
            0.0,
            margin / 10.0,
            self.snr_db / 20.0,
            self.scale,
            (self.llr / 20.0).clamp(-2.0, 2.0),
            self.outshone as u8 as f64,
            self.guarded as u8 as f64,
            0.0,
            0.5,
            (1.0 + self.nemit as f64).ln() / 4.0,
            0.0,
            self.fade_db / 10.0,
        ];
        f.map(|x| x as f32)
    }

    /// Calibrator features of character c from the current metadata and track state.
    fn char_features(&self, c: char) -> [f32; crate::calib::NF] {
        let m = &self.cur;
        let second = m.alts.get(1).map(|a| a.p).unwrap_or(0.0);
        let margin = ((m.conf + 1e-3) / (second + 1e-3)).ln().clamp(-5.0, 10.0);
        let f = [
            crate::calib::logit(m.conf),
            m.doubt as u8 as f64,
            (self.path == 1) as u8 as f64,
            (self.path == 2) as u8 as f64,
            self.age as f64 / LAG_HARD as f64,
            margin / 10.0,
            self.snr_db / 20.0,
            self.scale,
            (self.llr / 20.0).clamp(-2.0, 2.0),
            self.outshone as u8 as f64,
            self.guarded as u8 as f64,
            self.unsure_char(c) as u8 as f64,
            code_of(c).len() as f64 / 6.0,
            (1.0 + self.nemit as f64).ln() / 4.0,
            (self.at_start && self.nemit == 0) as u8 as f64,
            self.fade_db / 10.0,
        ];
        f.map(|x| x as f32)
    }

    pub fn emit_char(&mut self, c: char, space: bool) {
        self.cur.feat = self.char_features(c);
        if self.first_hold.is_some() && space && self.nemit == 0 {
            // the held character was a word of its own
            self.decide_first();
        }
        if !self.first_done {
            self.first_done = true;
            if !self.lead_ok(c) && self.at_start && self.lead > 0.0 {
                // a clean rise after silence at the stream start: the character is kept
                // only if it turns the token that follows into a callsign
                self.first_hold = Some((c, std::mem::take(&mut self.cur)));
                return;
            }
            if !self.lead_ok(c) {
                // the first character may be the tail of one cut off by the track start
                self.cur = CharMeta::default();
                self.first_dropped = true;
                return;
            }
        } else if self.first_dropped && self.nemit == 0 && space {
            // a word space separates the dropped character from the first token
            self.first_dropped = false;
        }
        if space && self.nemit > 0 && !self.spaced {
            self.emit_space(self.confirmed_now);
        }
        self.held.push(c);
        self.held_meta.push(std::mem::take(&mut self.cur));
        self.spaced = false;
        self.nemit += 1;
        // doubtful characters do not count towards confirming a track
        if informative(c) && !self.held_meta.last().is_some_and(|m| m.doubt) {
            self.nchars += 1;
        }
        if self.confirmed_now {
            self.release();
        }
    }

    pub fn emit_space(&mut self, confirmed: bool) {
        if self.spaced || self.nemit == 0 {
            return;
        }
        if self.first_hold.is_some() {
            self.decide_first();
        }
        self.spaced = true;
        self.held.push(' ');
        self.held_meta.push(CharMeta {
            conf: 1.0,
            doubt: false,
            alts: Vec::new(),
            ..CharMeta::default()
        });
        if confirmed {
            self.release();
        }
    }

    /// Decides a held first character: it is kept only if it makes a callsign of the
    /// token that follows, which is none without it, and then marked doubtful, since the
    /// syntax that decided it cannot also confirm the callsign.
    pub fn decide_first(&mut self) {
        let Some((c, mut meta)) = self.first_hold.take() else {
            return;
        };
        let tok: String = self.held.iter().take_while(|&&x| x != ' ').collect();
        let with = format!("{c}{tok}");
        if !self.outshone
            && !tok.is_empty()
            && syntax::is_callsign(&with)
            && !syntax::is_callsign(&tok)
        {
            meta.doubt = true;
            self.held.insert(0, c);
            self.held_meta.insert(0, meta);
            self.nemit += 1;
        } else {
            self.first_dropped = true;
        }
        if self.confirmed_now {
            self.release();
        }
    }

    pub fn release(&mut self) {
        if self.first_hold.is_some() {
            return;
        }
        self.out.append(&mut self.held);
        self.out_meta.append(&mut self.held_meta);
    }

    /// Releases the held text and drops its part up to the last match of the end of the
    /// text another timing has already emitted, whose last characters may be wrong (all
    /// of it without a match).
    pub fn align(&mut self, sent: &[char]) {
        self.release();
        let (v, m) = if self.out.is_empty() {
            (&mut self.held, &mut self.held_meta)
        } else {
            (&mut self.out, &mut self.out_meta)
        };
        let pos: Vec<usize> = (0..v.len()).filter(|&i| v[i] != ' ').collect();
        let seq: Vec<char> = pos.iter().map(|&i| v[i]).collect();
        let mut cut = v.len();
        'found: for d in 0..=ALIGN_SKIP.min(sent.len()) {
            let sent = &sent[..sent.len() - d];
            for k in (2..=sent.len().min(ALIGN_CHARS)).rev() {
                let pat = &sent[sent.len() - k..];
                if let Some(j) = (k..=seq.len()).rev().find(|&j| &seq[j - k..j] == pat) {
                    cut = pos[j - 1] + 1 + usize::from(d > 0);
                    break 'found;
                }
            }
        }
        let cut = cut.min(v.len());
        v.drain(..cut);
        let mc = cut.min(m.len());
        m.drain(..mc);
    }

    pub fn discard_held(&mut self) {
        self.held.clear();
        self.held_meta.clear();
        self.nchars = 0;
    }

    /// Updates b, r, gc, gw and the jitter scale from the events of committed character c.
    /// Whether an element of character c, the next to be committed, or the space before
    /// it lay under a louder station's keying edge.
    pub fn unsure_char(&self, c: char) -> bool {
        let skip = self.evs.iter().take_while(|e| !e.mark).count();
        let n = (2 * code_of(c).len()).saturating_sub(1);
        self.unsure.iter().take(skip + n).any(|&u| u)
    }

    /// Drops the events of character c without learning from them.
    pub fn forget(&mut self, c: char) {
        let skip = self.evs.iter().take_while(|e| !e.mark).count();
        let n = (skip + (2 * code_of(c).len()).saturating_sub(1)).min(self.evs.len());
        self.evs.drain(..n);
        self.unsure.drain(..n);
        self.lu.drain(..n);
    }

    pub fn learn(&mut self, c: char) {
        let code = code_of(c).as_bytes();
        while !self.evs.is_empty() && !self.evs[0].mark {
            let x = self.evs[0].d / self.lu[0].exp();
            self.gaps.push(x);
            if self.gaps.len() > GAP_MEMORY {
                self.gaps.remove(0);
            }
            self.evs.remove(0);
            self.unsure.remove(0);
            self.lu.remove(0);
        }
        if code.is_empty() || self.evs.len() < 2 * code.len() - 1 {
            self.evs.clear();
            self.unsure.clear();
            self.lu.clear();
            return;
        }
        let n = 2 * code.len() - 1;
        let m = self.means();
        let f = LEARN_FORGET;
        for j in 0..n {
            let x = (self.evs[j].d / self.lu[j].exp()).clamp(LEARN_MIN, LEARN_MAX);
            let (li, mu) = if self.evs[j].mark {
                if code[j / 2] == b'.' {
                    (0, m.dit)
                } else {
                    (2, m.dah)
                }
            } else {
                (1, m.ie)
            };
            let si = if li == 1 { SIG_DEF[2] } else { SIG_DEF[0] };
            self.res[0] = f * self.res[0] + ((x / mu).ln().abs() / si).min(RESIDUAL_MAX);
            self.res[1] = f * self.res[1] + 1.0;
            let fl = if li < 2 { LEARN_FORGET_FAST } else { f };
            self.st[li][0] = fl * self.st[li][0] + x;
            self.st[li][1] = fl * self.st[li][1] + 1.0;
        }
        self.evs.drain(..n);
        self.unsure.drain(..n);
        self.lu.drain(..n);
        self.scale = ((self.res[0] + SCALE_PRIOR_SUM) / (self.res[1] + SCALE_PRIOR_N) / SCALE_NORM)
            .clamp(SCALE_MIN, SCALE_MAX);
        let md = self.st[0][0] / self.st[0][1];
        let mi = self.st[1][0] / self.st[1][1];
        let mh = self.st[2][0] / self.st[2][1];
        let k = (md + mi) / 2.0;
        self.b = ((md - mi) / (2.0 * k)).clamp(-BIAS_MAX, BIAS_MAX);
        self.r = (mh / k - self.b).clamp(RATIO_MIN, RATIO_MAX);
        let (gc, gw) = fit_gaps(&self.gaps, k, self.b, self.gc, self.gw);
        self.gc = gc;
        self.gw = gw;
    }

    /// Whether the first character c cannot be the tail of a longer character cut off by
    /// the track start: the silence before its first mark is at least a character gap, or
    /// its code is not the end of any other code.
    fn lead_ok(&self, c: char) -> bool {
        let m = self.means();
        self.lead >= (m.ie * m.cg).sqrt() * self.best.u || !tail_of_other(c, self.lead <= 0.0)
    }

    /// Whether the open gap is clearly a word gap (word space).
    pub fn word_gap(&self, gap: f64) -> bool {
        let m = self.means();
        let x = gap / self.best.u;
        x.ln() > 0.5 * (m.cg.ln() + m.wg.ln()) + 2.0 * SIG_DEF[4] * self.scale * 0.5
            && self.ambig > AMBIG_TRUSTED
    }
}

impl Default for Common {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Packed hypothesis keys and interned texts

/// Morse tree node: (1 << length) | bits, with dot = 0 and dash = 1 (root = 1).
type Node = u8;
const ROOT: Node = 1;

struct Tree {
    ok: [bool; 128],
    ch: [u8; 128],
    dah_last: [bool; 128],
    /// Prior weights of the next mark (dit, dah) at a node, of continuing the character
    /// after an element space, and of ending it (mean weight 1 over valid branches).
    w_dit: [f64; 128],
    w_dah: [f64; 128],
    w_cont: [f64; 128],
    w_end: [f64; 128],
}

fn node_of(code: &str) -> Node {
    let mut n: u32 = 1;
    for c in code.bytes() {
        n = (n << 1) | (c == b'-') as u32;
    }
    n as Node
}

fn node_code(n: Node) -> String {
    let mut s = Vec::new();
    let mut n = n as u32;
    while n > 1 {
        s.push(if n & 1 == 1 { '-' } else { '.' });
        n >>= 1;
    }
    s.iter().rev().collect()
}

fn tree() -> &'static Tree {
    static T: std::sync::OnceLock<Tree> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = Tree {
            ok: [false; 128],
            ch: [0; 128],
            dah_last: [false; 128],
            w_dit: [1.0; 128],
            w_dah: [1.0; 128],
            w_cont: [1.0; 128],
            w_end: [1.0; 128],
        };
        for &(c, code) in &MORSE {
            t.ch[node_of(code) as usize] = c as u8;
            for i in 1..=code.len() {
                t.ok[node_of(&code[..i]) as usize] = true;
            }
        }
        for n in 2..128 {
            t.dah_last[n] = n & 1 == 1;
        }
        let mut pc = [0.0; 128];
        for &(c, code) in &MORSE {
            pc[node_of(code) as usize] = char_freq(c);
        }
        // subtree sums, children before parents
        let mut sub = pc;
        for n in (2..128).rev() {
            sub[n / 2] += sub[n];
        }
        // tempered branch probabilities, scaled so that the weights of the valid branches
        // sum to their number (the total weight of each step is unchanged)
        let split = |a: f64, b: f64| -> (f64, f64) {
            let (x, y) = (a.powf(PRIOR_POWER), b.powf(PRIOR_POWER));
            if a <= 0.0 || b <= 0.0 {
                return (1.0, 1.0);
            }
            (2.0 * x / (x + y), 2.0 * y / (x + y))
        };
        for n in 1..64 {
            let (sd, sh) = (sub[2 * n], sub[2 * n + 1]);
            let (wd, wh) = split(sd, sh);
            t.w_dit[n] = wd;
            t.w_dah[n] = wh;
            if n > 1 && pc[n] > 0.0 {
                let (wc, we) = split(sub[n] - pc[n], pc[n]);
                t.w_cont[n] = wc;
                t.w_end[n] = we;
            }
        }
        t
    })
}

#[inline]
fn child(n: Node, dah: bool) -> usize {
    ((n as usize) << 1) | dah as usize
}

/// Fast non-cryptographic hasher for integer keys.
#[derive(Default)]
struct Fx(u64);

impl Hasher for Fx {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ b as u64).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
        }
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.write_u64(i as u64);
    }
}

type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<Fx>>;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Key {
    node: Node,
    word: bool,
    wl: i8,
    bi: i8,
    txt: u32,
}

impl Key {
    #[inline]
    fn pack(self) -> u64 {
        (self.txt as u64) << 16
            | (self.node as u64) << 8
            | (self.word as u64) << 7
            | ((self.wl as u64) & 3) << 4
            | ((self.bi + 4) as u64 & 7)
    }
}

/// Interned uncommitted texts; id 0 is the empty text.
struct Texts {
    data: Vec<Vec<u8>>,
    map: FxMap<Vec<u8>, u32>,
    /// Per id: (leading space, first non-space char or 0, id of the rest after it).
    strip: Vec<Option<(bool, u8, u32)>>,
    app: FxMap<(u32, u8), u32>,
}

impl Texts {
    fn new() -> Texts {
        let mut t = Texts {
            data: Vec::new(),
            map: FxMap::default(),
            strip: Vec::new(),
            app: FxMap::default(),
        };
        t.intern(Vec::new());
        t
    }

    fn intern(&mut self, v: Vec<u8>) -> u32 {
        if let Some(&id) = self.map.get(&v) {
            return id;
        }
        let id = self.data.len() as u32;
        self.map.insert(v.clone(), id);
        self.data.push(v);
        self.strip.push(None);
        id
    }

    fn append(&mut self, id: u32, c: u8) -> u32 {
        if let Some(&r) = self.app.get(&(id, c)) {
            return r;
        }
        let mut v = self.data[id as usize].clone();
        v.push(c);
        let r = self.intern(v);
        self.app.insert((id, c), r);
        r
    }

    /// (leading space, first non-space char or 0, rest after that char).
    fn strip(&mut self, id: u32) -> (bool, u8, u32) {
        if let Some(s) = self.strip[id as usize] {
            return s;
        }
        let v = &self.data[id as usize];
        let has = v.first() == Some(&b' ');
        let p = v.iter().position(|&c| c != b' ');
        let s = match p {
            None => (has, 0, 0),
            Some(p) => {
                let c = v[p];
                let rest = v[p + 1..].to_vec();
                (has, c, self.intern(rest))
            }
        };
        self.strip[id as usize] = Some(s);
        s
    }
}

/// Insertion-ordered set of hypotheses with fixed-size probability vectors.
#[derive(Clone)]
struct HSet {
    keys: Vec<Key>,
    vals: Vec<[f64; NG]>,
    idx: FxMap<u64, u32>,
    /// Grid indices outside which every vector is zero.
    win: (usize, usize),
}

impl Default for HSet {
    fn default() -> Self {
        HSet {
            keys: Vec::new(),
            vals: Vec::new(),
            idx: FxMap::default(),
            win: (0, NG),
        }
    }
}

impl HSet {
    fn len(&self) -> usize {
        self.keys.len()
    }

    fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    fn clear(&mut self) {
        self.keys.clear();
        self.vals.clear();
        self.idx.clear();
    }

    #[inline]
    fn slot(&mut self, k: Key) -> &mut [f64; NG] {
        let n = self.keys.len() as u32;
        let i = *self.idx.entry(k.pack()).or_insert(n);
        if i == n {
            self.keys.push(k);
            self.vals.push([0.0; NG]);
        }
        &mut self.vals[i as usize]
    }

    #[inline]
    fn add_into(&mut self, k: Key, v: &[f64; NG], w: &[f64; NG]) {
        let (a, b) = self.win;
        let dst = self.slot(k);
        for i in a..b {
            dst[i] += v[i] * w[i];
        }
    }

    #[inline]
    fn add_plain(&mut self, k: Key, v: &[f64; NG]) {
        let (a, b) = self.win;
        let dst = self.slot(k);
        for i in a..b {
            dst[i] += v[i];
        }
    }

    #[inline]
    fn set(&mut self, k: Key, v: &[f64; NG]) {
        *self.slot(k) = *v;
    }

    fn retain(&mut self, keep: &[bool]) {
        let mut j = 0;
        for i in 0..self.keys.len() {
            if keep[i] {
                self.keys[j] = self.keys[i];
                self.vals[j] = self.vals[i];
                j += 1;
            }
        }
        self.keys.truncate(j);
        self.vals.truncate(j);
        self.idx.clear();
        for (i, k) in self.keys.iter().enumerate() {
            self.idx.insert(k.pack(), i as u32);
        }
    }
}

#[inline]
fn vsum(v: &[f64; NG]) -> f64 {
    let mut s = 0.0;
    for &x in v {
        s += x;
    }
    s
}

fn norm_prune(h: &mut HSet, prior: &[f64]) {
    for v in h.vals.iter_mut() {
        for (x, &p) in v.iter_mut().zip(prior) {
            if p == 0.0 {
                *x = 0.0;
            }
        }
    }
    let mut ks: Vec<(usize, f64)> = Vec::with_capacity(h.len());
    let mut tot = 0.0;
    for (i, v) in h.vals.iter().enumerate() {
        let s = vsum(v);
        ks.push((i, s));
        tot += s;
    }
    if tot <= 0.0 || tot.is_nan() || tot.is_infinite() {
        h.clear();
        h.win = (0, NG);
        h.set(
            Key {
                node: ROOT,
                word: true,
                wl: 0,
                bi: 0,
                txt: 0,
            },
            prior.try_into().unwrap(),
        );
        return;
    }
    let mut keep = vec![true; h.len()];
    if ks.len() > MAX_GROUPS {
        ks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        tot = 0.0;
        for e in &ks[..MAX_GROUPS] {
            tot += e.1;
        }
        for e in &ks[MAX_GROUPS..] {
            keep[e.0] = false;
        }
    }
    let mut all = true;
    for (i, v) in h.vals.iter_mut().enumerate() {
        if !keep[i] {
            all = false;
            continue;
        }
        if vsum(v) < PRUNE_REL * tot {
            keep[i] = false;
            all = false;
            continue;
        }
        for x in v.iter_mut() {
            *x /= tot;
        }
    }
    if !all {
        h.retain(&keep);
    }
}

/// Cache of the mark-side factor of the shifted space likelihood, valid while the last
/// mark and the parameters it depends on are unchanged.
struct MarkCache {
    valid: bool,
    dm: f64,
    b: f64,
    r: f64,
    scale: f64,
    win: (usize, usize),
    /// [bi][dah] -> per grid index and shift j (1..=8): ratio Lm(dm+dl)/max(Lm(dm),1e-300),
    /// NaN when the shift is not admissible on the mark side.
    ratio: Vec<Option<Box<[[f64; N_SHIFTS]; NG]>>>,
}

/// Dit and dah likelihoods of a mark for one bias index.
type MarkLiks = ([f64; NG], [f64; NG]);

/// Element, character and word space likelihoods: unshifted, after a dit, after a dah.
type SpaceLiks = [[[f64; NG]; 3]; 3];

/// The timing tracker.
pub struct Timing {
    pub c: Common,
    h: HSet,
    /// Grid indices outside which the hypotheses carry no mass.
    win: (usize, usize),
    texts: Texts,
    mark_cache: MarkCache,
    gen: u64,
    guess_cache: std::cell::RefCell<(u64, usize, Vec<Guess>)>,
}

impl Default for Timing {
    fn default() -> Self {
        Self::new()
    }
}

impl Timing {
    /// A timing whose speed prior covers the whole grid instead of ending at 100 WPM.
    pub fn fast() -> Timing {
        let mut t = Timing::new();
        t.c.fast_prior = true;
        t
    }

    fn prior(&self) -> &'static [f64] {
        if self.c.fast_prior {
            &tables().g_prior_fast
        } else {
            &tables().g_prior
        }
    }

    pub fn new() -> Timing {
        Timing {
            c: Common::new(),
            h: HSet::default(),
            win: (0, NG),
            texts: Texts::new(),
            mark_cache: MarkCache {
                valid: false,
                dm: 0.0,
                b: 0.0,
                r: 0.0,
                scale: 0.0,
                win: (0, NG),
                ratio: (0..10).map(|_| None).collect(),
            },
            gen: 0,
            guess_cache: std::cell::RefCell::new((u64::MAX, 0, Vec::new())),
        }
    }

    fn init(&mut self) {
        if !self.c.init() {
            return;
        }
        let pr = self.prior();
        self.h.clear();
        for bi in -BI_MAX..=BI_MAX {
            let mut w = [0.0; NG];
            for i in 0..NG {
                w[i] = pr[i] / (2 * BI_MAX + 1) as f64;
            }
            self.h.set(
                Key {
                    node: ROOT,
                    word: true,
                    wl: 0,
                    bi,
                    txt: 0,
                },
                &w,
            );
        }
    }

    fn set_h(&mut self, h: HSet) {
        self.win = h.win;
        self.h = h;
        self.gen += 1;
    }

    /// Drops interned texts no longer referenced by the hypothesis set.
    fn gc_texts(&mut self) {
        if self.texts.data.len() < TEXTS_GC {
            return;
        }
        let mut nt = Texts::new();
        for k in &mut self.h.keys {
            let v = self.texts.data[k.txt as usize].clone();
            k.txt = nt.intern(v);
        }
        self.texts = nt;
        let keys = self.h.keys.clone();
        self.h.idx.clear();
        for (i, k) in keys.iter().enumerate() {
            self.h.idx.insert(k.pack(), i as u32);
        }
        self.gen += 1;
    }

    fn lik(&self, dst: &mut [f64; NG], d: f64, mu: f64, li: usize, censored: bool, word: bool) {
        let sp = spread(li, self.c.scale, self.c.gaps.len());
        let ld = d.ln() - mu.ln();
        let glu = &tables().g_log_u;
        for i in self.win.0..self.win.1 {
            dst[i] = lik_x(ld - glu[i], sp, censored, word);
        }
    }

    fn step_mark(&mut self, d: f64) -> HSet {
        let pon = self.prior_on();
        let tb = tables();
        let tr = tree();
        let pad = tb.k_word.len() / 2;
        self.win = (self.win.0.saturating_sub(pad), (self.win.1 + pad).min(NG));
        let (wa, wb) = self.win;
        let mut cache: [Option<Box<MarkLiks>>; 5] = Default::default();
        let mut nh = HSet {
            win: self.win,
            ..HSet::default()
        };
        let mut tmp = [0.0; NG];
        let mut sc = [0.0; NG];
        for idx in 0..self.h.len() {
            let k = self.h.keys[idx];
            let v = &self.h.vals[idx];
            let ker = if k.word { &tb.k_word } else { &tb.k_in };
            let j = ker.len() / 2;
            for (i, t) in tmp.iter_mut().enumerate().take(wb).skip(wa) {
                let mut s = 0.0;
                for (a, &kv) in ker.iter().enumerate() {
                    let ii = (i + a).saturating_sub(j).min(NG - 1);
                    s += v[ii] * kv;
                }
                *t = s;
            }
            let dot = child(k.node, false);
            let dash = child(k.node, true);
            let (okd, okh) = (dot < 128 && tr.ok[dot], dash < 128 && tr.ok[dash]);
            for db in -1i8..=1 {
                let bi = k.bi + db;
                if !(-BI_MAX..=BI_MAX).contains(&bi) {
                    continue;
                }
                let pm = if db != 0 { BI_MOVE } else { BI_STAY };
                for i in wa..wb {
                    sc[i] = tmp[i] * pm;
                }
                let slot = (bi + BI_MAX) as usize;
                if cache[slot].is_none() {
                    let m = self.c.means_b(bi);
                    let mut c = Box::new(([0.0; NG], [0.0; NG]));
                    self.lik(&mut c.0, d, m.dit, 0, false, false);
                    self.lik(&mut c.1, d, m.dah, 1, false, false);
                    cache[slot] = Some(c);
                }
                let c = cache[slot].as_ref().unwrap();
                if okd {
                    let wd = if pon { tr.w_dit[k.node as usize] } else { 1.0 };
                    let mut s2 = [0.0; NG];
                    for i in wa..wb {
                        s2[i] = sc[i] * wd;
                    }
                    nh.add_into(
                        Key {
                            node: dot as Node,
                            word: false,
                            bi,
                            ..k
                        },
                        &s2,
                        &c.0,
                    );
                }
                if okh {
                    let wh = if pon { tr.w_dah[k.node as usize] } else { 1.0 };
                    for i in wa..wb {
                        sc[i] *= wh;
                    }
                    nh.add_into(
                        Key {
                            node: dash as Node,
                            word: false,
                            bi,
                            ..k
                        },
                        &sc,
                        &c.1,
                    );
                }
            }
        }
        nh
    }

    /// Ratio table of the mark side for (bi, dah), computed once per mark and parameter set.
    fn mark_ratio(&mut self, bi: i8, dah: bool) -> &[[f64; N_SHIFTS]; NG] {
        let c = &self.c;
        let mc = &mut self.mark_cache;
        if !mc.valid
            || mc.dm != c.last_mark
            || mc.b != c.b
            || mc.r != c.r
            || mc.scale != c.scale
            || mc.win != self.win
        {
            mc.valid = true;
            mc.win = self.win;
            mc.dm = c.last_mark;
            mc.b = c.b;
            mc.r = c.r;
            mc.scale = c.scale;
            mc.ratio.fill(None);
        }
        let slot = (bi + BI_MAX) as usize * 2 + dah as usize;
        if mc.ratio[slot].is_none() {
            let tb = tables();
            let m = c.means_b(bi);
            let (mu_m, li_m) = if dah { (m.dah, 1) } else { (m.dit, 0) };
            let sp = spread(li_m, c.scale, c.gaps.len());
            let lmu = mu_m.ln();
            let dm = c.last_mark;
            let mut t = Box::new([[f64::NAN; N_SHIFTS]; NG]);
            for i in self.win.0..self.win.1 {
                let u = tb.g_u[i];
                let lm0 = lik_x(dm.max(DUR_MIN).ln() - lmu - tb.g_log_u[i], sp, false, false);
                for j in 1..=N_SHIFTS {
                    let dl = j as f64 * SHIFT_STEP * u;
                    if dm + dl < SHIFT_MARK_MIN * u {
                        continue;
                    }
                    let lm = lik_x(
                        (dm + dl).max(DUR_MIN).ln() - lmu - tb.g_log_u[i],
                        sp,
                        false,
                        false,
                    );
                    t[i][j - 1] = lm / lm0.max(LIK_MIN);
                }
            }
            mc.ratio[slot] = Some(t);
        }
        mc.ratio[slot].as_ref().unwrap()
    }

    /// Space likelihoods (element, character, word space) for bias index bi, unshifted
    /// and, after a mark, shifted for a preceding dit and dah.
    fn space_liks(&mut self, d: f64, censored: bool, bi: i8, need: [bool; 2]) -> SpaceLiks {
        let tb = tables();
        let m = self.c.means_b(bi);
        let mus = [m.ie, m.cg, m.wg];
        let mut base = [[0.0; NG]; 3];
        self.lik(&mut base[0], d, m.ie, 2, censored, false);
        self.lik(&mut base[1], d, m.cg, 3, censored, false);
        self.lik(&mut base[2], d, m.wg, 4, censored, true);
        if self.c.last_mark <= 0.0 {
            return [base, base, base];
        }
        let sps = [
            spread(2, self.c.scale, self.c.gaps.len()),
            spread(3, self.c.scale, self.c.gaps.len()),
            spread(4, self.c.scale, self.c.gaps.len()),
        ];
        let lmus = [mus[0].ln(), mus[1].ln(), mus[2].ln()];
        let words = [false, false, true];
        // upper bound of the space likelihood (exact pruning of shifts that cannot win)
        let bound: [f64; 3] = std::array::from_fn(|q| {
            if censored {
                BOUND_MARGIN
            } else {
                let s = sps[q];
                let mut b = OUTLIER_KEEP * s.norm + OUTLIER_W / (s.sg * WIDE_FACTOR);
                if words[q] {
                    b = b.max(WORD_FLOOR / (s.sg * WORD_FLOOR_SPREAD));
                }
                b * BOUND_MARGIN
            }
        });
        // a variant that no hypothesis uses gets ratio -inf (never wins, never evaluated)
        let ratio_d = if need[0] {
            *self.mark_ratio(bi, false)
        } else {
            [[f64::NEG_INFINITY; N_SHIFTS]; NG]
        };
        let ratio_h = if need[1] {
            *self.mark_ratio(bi, true)
        } else {
            [[f64::NEG_INFINITY; N_SHIFTS]; NG]
        };
        let dm = self.c.last_mark;
        let mut outd = base;
        let mut outh = base;
        let mut js = [0usize; N_SHIFTS];
        for i in self.win.0..self.win.1 {
            let u = tb.g_u[i];
            let glu = tb.g_log_u[i];
            // admissible shifts, largest first (smallest remaining space)
            let mut nj = 0;
            for j in (1..=N_SHIFTS).rev() {
                let dl = j as f64 * SHIFT_STEP * u;
                if dm + dl < SHIFT_MARK_MIN * u || d - dl < SHIFT_SPACE_MIN * u {
                    continue;
                }
                js[nj] = j;
                nj += 1;
            }
            if nj == 0 {
                continue;
            }
            for q in 0..3 {
                let eval = |j: usize| {
                    let dl = j as f64 * SHIFT_STEP * u;
                    lik_x(
                        (d - dl).max(DUR_MIN).ln() - lmus[q] - glu,
                        sps[q],
                        censored,
                        words[q],
                    )
                };
                let mut bq = bound[q];
                let mut start = 0;
                if censored {
                    // the survival grows as the remaining space shrinks: the largest shift
                    // bounds all others (exact pruning)
                    let j = js[0];
                    let ls = eval(j);
                    let (rd, rh) = (ratio_d[i][j - 1], ratio_h[i][j - 1]);
                    let vd = rd * ls * SHIFT_P;
                    if vd > outd[q][i] {
                        outd[q][i] = vd;
                    }
                    let vh = rh * ls * SHIFT_P;
                    if vh > outh[q][i] {
                        outh[q][i] = vh;
                    }
                    bq = ls * BOUND_MARGIN_TIGHT;
                    start = 1;
                }
                for &j in &js[start..nj] {
                    let (rd, rh) = (ratio_d[i][j - 1], ratio_h[i][j - 1]);
                    if rd * bq * SHIFT_P <= outd[q][i] && rh * bq * SHIFT_P <= outh[q][i] {
                        continue;
                    }
                    let ls = eval(j);
                    let vd = rd * ls * SHIFT_P;
                    if vd > outd[q][i] {
                        outd[q][i] = vd;
                    }
                    let vh = rh * ls * SHIFT_P;
                    if vh > outh[q][i] {
                        outh[q][i] = vh;
                    }
                    if censored {
                        // shifts are visited in decreasing size: this survival bounds the rest
                        bq = ls * BOUND_MARGIN_TIGHT;
                    }
                }
            }
        }
        [base, outd, outh]
    }

    fn step_space(&mut self, d: f64, censored: bool) -> HSet {
        let pon = self.prior_on();
        let tr = tree();
        let mut cache: [Option<Box<SpaceLiks>>; 5] = Default::default();
        let mut nh = HSet {
            win: self.win,
            ..HSet::default()
        };
        let (wa, wb) = self.win;
        let shifted = self.c.last_mark > 0.0;
        // shifted variants (after a dit: bit 0, after a dah: bit 1) used per bias index
        let mut need_variant = [0u8; 5];
        for k in &self.h.keys {
            if k.node != ROOT {
                need_variant[(k.bi + BI_MAX) as usize] |= 1 << tr.dah_last[k.node as usize] as u8;
            }
        }
        for idx in 0..self.h.len() {
            let k = self.h.keys[idx];
            if k.node == ROOT {
                let v = self.h.vals[idx];
                nh.add_plain(k, &v);
                continue;
            }
            let slot = (k.bi + BI_MAX) as usize;
            if cache[slot].is_none() {
                let need = [need_variant[slot] & 1 != 0, need_variant[slot] & 2 != 0];
                cache[slot] = Some(Box::new(self.space_liks(d, censored, k.bi, need)));
            }
            let c = cache[slot].as_ref().unwrap();
            let cc = if shifted {
                if tr.dah_last[k.node as usize] {
                    &c[2]
                } else {
                    &c[1]
                }
            } else {
                &c[0]
            };
            let v = &self.h.vals[idx];
            let (dot, dash) = (child(k.node, false), child(k.node, true));
            if (dot < 128 && tr.ok[dot]) || (dash < 128 && tr.ok[dash]) {
                let wc = if pon { tr.w_cont[k.node as usize] } else { 1.0 };
                if wc == 1.0 {
                    nh.add_into(Key { word: false, ..k }, v, &cc[0]);
                } else {
                    let mut w = [0.0; NG];
                    for i in wa..wb {
                        w[i] = cc[0][i] * wc;
                    }
                    nh.add_into(Key { word: false, ..k }, v, &w);
                }
            }
            let ch = tr.ch[k.node as usize];
            if ch == 0 {
                continue;
            }
            let lp = char_lp(ch as char) * if pon { tr.w_end[k.node as usize] } else { 1.0 };
            let mut w = [0.0; NG];
            for i in wa..wb {
                w[i] = cc[1][i] * lp;
            }
            let wl = (k.wl + 1).min(2);
            let t1 = self.texts.append(k.txt, ch);
            nh.add_into(
                Key {
                    node: ROOT,
                    txt: t1,
                    word: false,
                    wl,
                    bi: k.bi,
                },
                v,
                &w,
            );
            let wp = if k.wl == 0 { ONE_CHAR_WORD } else { 1.0 };
            for i in wa..wb {
                w[i] = cc[2][i] * lp * wp;
            }
            let t2 = self.texts.append(t1, b' ');
            nh.add_into(
                Key {
                    node: ROOT,
                    txt: t2,
                    word: true,
                    wl: 0,
                    bi: k.bi,
                },
                v,
                &w,
            );
        }
        if !censored && nh.keys.iter().any(|k| k.word) {
            nh.win = (0, NG);
        }
        if !censored {
            let prior = self.prior();
            for (k, v) in nh.keys.iter().zip(nh.vals.iter_mut()) {
                if k.word {
                    let s = vsum(v);
                    for i in 0..NG {
                        v[i] = RESET_KEEP * v[i] + RESET_PROB * s * prior[i];
                    }
                }
            }
        }
        nh
    }

    /// Whether the character prior applies: not on a track outshone by a nearby station,
    /// whose spurious elements the prior would turn into confident characters.
    fn prior_on(&self) -> bool {
        !self.c.outshone
    }

    /// Narrows the grid window to the indices whose marginal mass is not negligible and
    /// zeroes the rest.
    fn narrow(&mut self, p: &[f64; NG]) {
        let mx = p.iter().cloned().fold(0.0, f64::max);
        if mx <= 0.0 {
            self.win = (0, NG);
            return;
        }
        let a = p.iter().position(|&x| x >= WIN_EPS * mx).unwrap_or(0);
        let b = NG - p.iter().rev().position(|&x| x >= WIN_EPS * mx).unwrap_or(0);
        self.win = (a, b.max(a + 1));
        for v in self.h.vals.iter_mut() {
            v[..a].fill(0.0);
            v[b..].fill(0.0);
        }
        self.h.win = self.win;
    }

    fn marginal(&self) -> [f64; NG] {
        let mut p = [0.0; NG];
        for v in &self.h.vals {
            for i in 0..NG {
                p[i] += v[i];
            }
        }
        p
    }

    /// Records a completed mark or space.
    pub fn add_element(&mut self, mut e: Element) {
        self.init();
        if !self.c.started {
            if !e.mark {
                self.c.lead += e.d;
                return;
            }
            self.c.started = true;
        }
        self.gc_texts();
        if e.d < ELEMENT_MIN {
            e.d = ELEMENT_MIN;
        }
        let mut nh = if e.mark {
            self.step_mark(e.d)
        } else {
            self.step_space(e.d, false)
        };
        if e.mark {
            self.c.last_mark = e.d;
            self.c.ended = false;
        }
        let mut pred = 0.0;
        for v in &nh.vals {
            pred += vsum(v);
        }
        self.c.update_llr(pred);
        norm_prune(&mut nh, self.prior());
        self.set_h(nh);
        let p = self.marginal();
        self.narrow(&p);
        self.c.posterior_stats(&p);
        self.c.evs.push(e);
        self.c.unsure.push(self.c.next_unsure);
        self.c.next_unsure = false;
        self.c.ev_base += 1;
        self.c.lu.push(self.c.best.u.ln());
        self.c.last_cens = 0.0;
        if !e.mark {
            self.apply_ahead();
            self.commit(None);
        }
    }

    fn apply_ahead(&mut self) {
        while !self.c.ahead.is_empty() {
            let c = self.c.ahead.remove(0);
            self.condition(c, -1);
            self.c.learn(c);
        }
    }

    fn condition(&mut self, c: char, sp: i32) {
        let mut nh = HSet::default();
        for idx in 0..self.h.len() {
            let k = self.h.keys[idx];
            let (has_sp, c0, rest) = self.texts.strip(k.txt);
            if c0 == 0 || c0 as char != c {
                continue;
            }
            if (sp == 0 && has_sp) || (sp == 1 && !has_sp) {
                continue;
            }
            nh.add_plain(Key { txt: rest, ..k }, &self.h.vals[idx]);
        }
        if !nh.is_empty() {
            norm_prune(&mut nh, self.prior());
            self.set_h(nh);
        }
    }

    fn strip_first(&mut self, h: &HSet, c: char) -> HSet {
        let mut nh = HSet::default();
        for (idx, k) in h.keys.iter().enumerate() {
            let (_, c0, rest) = self.texts.strip(k.txt);
            if c0 != 0 && c0 as char == c {
                nh.add_plain(Key { txt: rest, ..*k }, &h.vals[idx]);
            }
        }
        nh
    }

    fn commit(&mut self, prov: Option<HSet>) {
        if self.c.llr < LLR_GATE {
            return;
        }
        let censored = prov.is_some();
        let mut own = prov;
        loop {
            let mut mass: Vec<(char, f64)> = Vec::new();
            let mut spm: Vec<(char, f64)> = Vec::new();
            {
                let hh = own.as_ref().unwrap_or(&self.h);
                for (idx, k) in hh.keys.iter().enumerate() {
                    let (has_sp, c0, _) = self.texts.strip(k.txt);
                    if c0 == 0 {
                        continue;
                    }
                    let s = vsum(&hh.vals[idx]);
                    add_mass(&mut mass, c0 as char, s);
                    if has_sp {
                        add_mass(&mut spm, c0 as char, s);
                    }
                }
            }
            if mass.is_empty() {
                return;
            }
            let mut best = ' ';
            let mut bm = -1.0;
            for &(c, m) in &mass {
                if m > bm {
                    best = c;
                    bm = m;
                }
            }
            if bm < TAU_COMMIT {
                if censored {
                    return;
                }
                if self.c.pend_since < 0 {
                    self.c.pend_since = self.c.ev_base;
                }
                let age = self.c.ev_base - self.c.pend_since;
                let soft = if self.c.guarded { TAU_COMMIT } else { TAU_SOFT };
                if age < LAG_SOFT || (bm < soft && age < LAG_HARD) {
                    return;
                }
                if bm < soft {
                    let c = best;
                    self.c.path = 0;
                    self.c.age = age;
                    self.c.pend_since = -1;
                    self.c.cur = CharMeta {
                        conf: bm,
                        doubt: true,
                        alts: top_alts(&mass, 3),
                        ..CharMeta::default()
                    };
                    let space = get_mass(&spm, c) > SPACE_FRAC * get_mass(&mass, c);
                    if self.c.outshone {
                        // next to a stronger station a doubtful character is held back
                    } else if bm >= GUESS_MIN {
                        self.c.emit_char(c, space);
                    } else {
                        self.c.emit_char('*', space);
                    }
                    self.condition(c, -1);
                    self.c.learn(c);
                    continue;
                }
            }
            self.c.path = censored as u8;
            self.c.age = if self.c.pend_since >= 0 {
                self.c.ev_base - self.c.pend_since
            } else {
                0
            };
            self.c.pend_since = -1;
            let space = get_mass(&spm, best) > SPACE_FRAC * bm;
            self.c.cur = CharMeta {
                conf: bm,
                doubt: false,
                alts: top_alts(&mass, 3),
                ..CharMeta::default()
            };
            let held_back = !censored && self.c.guarded && self.c.unsure_char(best);
            if held_back {
                // keyed under a much stronger neighbour's edges: held back like a doubtful
                // character, and not learnt from
                self.c.cur.doubt = true;
            } else {
                self.c.emit_char(best, space);
            }
            if censored {
                self.c.ahead.push(best);
                let hh = own.take().unwrap();
                let mut nh = HSet::default();
                for (idx, k) in hh.keys.iter().enumerate() {
                    let (_, c0, rest) = self.texts.strip(k.txt);
                    if c0 != 0 && c0 as char == best {
                        nh.set(Key { txt: rest, ..*k }, &hh.vals[idx]);
                    }
                }
                own = Some(nh);
                continue;
            }
            self.condition(best, if space { 1 } else { 0 });
            if held_back {
                self.c.forget(best);
            } else {
                self.c.learn(best);
            }
        }
    }

    /// Called every frame with the current open space duration (0 if a mark is on).
    pub fn tick(&mut self, gap: f64, confirmed: bool, fin: bool) {
        self.init();
        self.c.confirmed_now = confirmed;
        if !self.c.started || gap <= 0.0 {
            return;
        }
        if fin {
            self.flush();
            return;
        }
        if !self.c.ended {
            let wg = self.c.means().wg;
            if gap > (END_K * wg * self.c.best.u).max(END_MIN_SEC) {
                self.end_over(gap);
                return;
            }
        }
        if gap < CENSOR_START * self.c.best.u
            || gap - self.c.last_cens < CENSOR_STEP * self.c.best.u
        {
            return;
        }
        self.c.last_cens = gap;
        if self.c.llr < LLR_GATE {
            // the censored commit below would return at once: the provisional set is not needed
            return;
        }
        let mut h = self.step_space(gap, true);
        let ahead = self.c.ahead.clone();
        for c in ahead {
            h = self.strip_first(&h, c);
        }
        if h.is_empty() {
            return;
        }
        norm_prune(&mut h, self.prior());
        let n0 = self.c.ahead.len();
        self.commit(Some(h));
        if self.c.ahead.len() != n0 {
            self.gen += 1;
        }
    }

    fn pending_code(&self) -> bool {
        self.h.keys.iter().any(|k| k.node != ROOT)
    }

    /// Commits a word space once the open gap is clearly a word gap.
    pub fn open_gap(&mut self, gap: f64, confirmed: bool) {
        let c = &self.c;
        if !c.started
            || gap <= 0.0
            || c.spaced
            || c.nemit == 0
            || c.gaps.len() < 6
            || (c.ahead.is_empty() && self.pending_code())
        {
            return;
        }
        if self.c.word_gap(gap) {
            self.c.emit_space(confirmed);
        }
    }

    fn best_key(h: &HSet) -> (Key, f64) {
        let mut bk = h.keys[0];
        let mut bm = -1.0;
        for (idx, k) in h.keys.iter().enumerate() {
            let s = vsum(&h.vals[idx]);
            if s > bm {
                bk = *k;
                bm = s;
            }
        }
        (bk, bm)
    }

    fn flush(&mut self) {
        self.c.path = 2;
        self.c.age = 0;
        if self.h.is_empty() {
            return;
        }
        let mut h = self.step_space(10.0, false);
        let ahead = self.c.ahead.clone();
        for c in ahead {
            h = self.strip_first(&h, c);
        }
        if h.is_empty() {
            self.set_h(HSet::default());
            self.c.ahead.clear();
            return;
        }
        norm_prune(&mut h, self.prior());
        let (bk, bm) = Self::best_key(&h);
        let txt = self.texts.data[bk.txt as usize].clone();
        for &c in &txt {
            if c == b' ' {
                continue;
            }
            self.c.cur = CharMeta {
                conf: bm,
                doubt: bm < TAU_SOFT,
                alts: Vec::new(),
                ..CharMeta::default()
            };
            self.c.emit_char(c as char, false);
        }
        self.set_h(HSet::default());
        self.c.ahead.clear();
    }

    fn end_over(&mut self, gap: f64) {
        self.c.ended = true;
        let mut h = self.step_space(gap, true);
        let ahead = self.c.ahead.clone();
        for c in ahead {
            h = self.strip_first(&h, c);
        }
        if h.is_empty() {
            return;
        }
        norm_prune(&mut h, self.prior());
        self.end_chars(&h);
        self.c.emit_space(self.c.confirmed_now);
        let mut nh = HSet::default();
        for (idx, k) in h.keys.iter().enumerate() {
            nh.add_plain(
                Key {
                    node: ROOT,
                    txt: 0,
                    word: true,
                    wl: 0,
                    bi: k.bi,
                },
                &h.vals[idx],
            );
        }
        norm_prune(&mut nh, self.prior());
        self.set_h(nh);
        self.c.ahead.clear();
        self.c.evs.clear();
        self.c.unsure.clear();
        self.c.lu.clear();
        self.c.pend_since = -1;
    }

    /// Commits the pending characters at the end of a transmission one by one while the
    /// posterior of the next character (marginalised over all hypotheses) is high enough.
    fn end_chars(&mut self, h: &HSet) {
        self.c.path = 2;
        self.c.age = 0;
        let mut h = h.clone();
        loop {
            let mut mass: Vec<(char, f64)> = Vec::new();
            let mut spm: Vec<(char, f64)> = Vec::new();
            let mut tot = 0.0;
            for (idx, k) in h.keys.iter().enumerate() {
                let s = vsum(&h.vals[idx]);
                tot += s;
                let (has_sp, c0, _) = self.texts.strip(k.txt);
                if c0 == 0 {
                    continue;
                }
                add_mass(&mut mass, c0 as char, s);
                if has_sp {
                    add_mass(&mut spm, c0 as char, s);
                }
            }
            let Some(&(best, bm)) = mass
                .iter()
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            else {
                return;
            };
            let p = bm / tot;
            if p < END_MIN {
                return;
            }
            self.c.cur = CharMeta {
                conf: p,
                doubt: p < TAU_SOFT,
                alts: top_alts(
                    &mass.iter().map(|&(c, m)| (c, m / tot)).collect::<Vec<_>>(),
                    3,
                ),
                ..CharMeta::default()
            };
            let space = get_mass(&spm, best) > SPACE_FRAC * bm;
            let soft = if self.c.guarded { TAU_COMMIT } else { TAU_SOFT };
            let held_back = self.c.guarded && self.c.unsure_char(best);
            if (p >= soft || !self.c.outshone) && !held_back {
                self.c.emit_char(best, space);
            }
            h = self.strip_first(&h, best);
        }
    }

    /// Top hypothesis groups of the uncommitted text (best first); cached until the
    /// hypothesis set or the queued characters change.
    pub fn guesses(&self, n: usize) -> Vec<Guess> {
        {
            let gc = self.guess_cache.borrow();
            if gc.0 == self.gen && gc.1 == n {
                return gc.2.clone();
            }
        }
        let mut m: Vec<Guess> = Vec::new();
        let mut idx: HashMap<(String, String), usize> = HashMap::new();
        let mut tot = 0.0;
        for (i, k) in self.h.keys.iter().enumerate() {
            let s = vsum(&self.h.vals[i]);
            let mut txt: Vec<char> = self.texts.data[k.txt as usize]
                .iter()
                .filter(|&&c| c != b' ')
                .map(|&c| c as char)
                .collect();
            let mut code = node_code(k.node);
            let mut ok = true;
            for &c in &self.c.ahead {
                if !txt.is_empty() {
                    if txt[0] != c {
                        ok = false;
                    }
                    txt.remove(0);
                } else if char_of(&code) == Some(c) {
                    code.clear();
                } else {
                    ok = false;
                }
            }
            if !ok {
                continue;
            }
            let key = (txt.iter().collect::<String>(), code);
            let gi = *idx.entry(key.clone()).or_insert_with(|| {
                m.push(Guess {
                    text: key.0.clone(),
                    pattern: key.1.clone(),
                    ch: char_of(&key.1).map(String::from).unwrap_or_default(),
                    p: 0.0,
                });
                m.len() - 1
            });
            m[gi].p += s;
            tot += s;
        }
        let out = if tot <= 0.0 {
            Vec::new()
        } else {
            for g in &mut m {
                g.p /= tot;
            }
            finish_guesses(m, n)
        };
        *self.guess_cache.borrow_mut() = (self.gen, n, out.clone());
        out
    }
}

/// Whether the code of c could be the end of another character's code; with `cut_mark`
/// the first mark may itself be cut short (a dah seen as a dit, or the whole character).
fn tail_of_other(c: char, cut_mark: bool) -> bool {
    let code = code_of(c);
    if code.is_empty() {
        return true;
    }
    let alt = format!("-{}", &code[1..]);
    MORSE.iter().any(|&(_, other)| {
        (other.len() > code.len() && other.ends_with(code))
            || (cut_mark && other != code && other.ends_with(alt.as_str()))
    })
}

/// Relative frequency of a character in amateur traffic.
fn char_freq(c: char) -> f64 {
    match c {
        'E' => 10.0,
        'T' | 'A' | 'O' | 'N' => 7.0,
        'I' | 'S' | 'R' | 'H' => 5.0,
        'D' | 'L' | 'C' | 'U' | 'M' | 'W' | 'Q' | 'K' => 3.0,
        _ => 2.0,
    }
}

fn add_mass(m: &mut Vec<(char, f64)>, c: char, s: f64) {
    if let Some(e) = m.iter_mut().find(|e| e.0 == c) {
        e.1 += s;
    } else {
        m.push((c, s));
    }
}

fn get_mass(m: &[(char, f64)], c: char) -> f64 {
    m.iter().find(|e| e.0 == c).map(|e| e.1).unwrap_or(0.0)
}

impl Timing {
    pub fn best(&self) -> Hyp {
        self.c.best
    }
    pub fn ambig(&self) -> f64 {
        self.c.ambig
    }
    pub fn nchars(&self) -> i64 {
        self.c.nchars
    }
    pub fn initialized(&self) -> bool {
        self.c.inited
    }
    pub fn means(&self) -> Means {
        self.c.means()
    }
    pub fn learned_b(&self) -> f64 {
        self.c.b
    }
    /// Ends the track: a held first character is decided and all text released.
    pub fn finish(&mut self) {
        self.c.decide_first();
        self.c.release()
    }

    pub fn release(&mut self) {
        self.c.release()
    }
    pub fn discard_held(&mut self) {
        self.c.discard_held()
    }
    pub fn align(&mut self, sent: &[char]) {
        self.c.align(sent)
    }
    pub fn out(&self) -> &[char] {
        &self.c.out
    }
    pub fn held(&self) -> &[char] {
        &self.c.held
    }
    pub fn held_meta(&self) -> &[CharMeta] {
        &self.c.held_meta
    }
    pub fn take_out(&mut self) -> (String, Vec<CharMeta>) {
        (
            self.c.out.drain(..).collect(),
            std::mem::take(&mut self.c.out_meta),
        )
    }
}

fn mean(x: &[f64]) -> f64 {
    let mut s = 0.0;
    for &v in x {
        s += v;
    }
    s / x.len() as f64
}

fn ss(x: &[f64], m: f64) -> f64 {
    let mut s = 0.0;
    for &v in x {
        s += (v - m) * (v - m);
    }
    s
}

/// Fits one or two clusters to the log of recent character-terminating gaps; returns (gc, gw).
fn fit_gaps(gaps: &[f64], k: f64, b: f64, gc0: f64, gw0: f64) -> (f64, f64) {
    if gaps.len() < 5 {
        return (gc0, gw0);
    }
    let g: Vec<f64> = gaps
        .iter()
        .map(|&x| (x / k + b).max(GAP_LOG_MIN).ln())
        .collect();
    let mut s = g.clone();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let (mut c1, mut c2) = (s[s.len() / 4], s[s.len() * 9 / 10]);
    let mut a: Vec<f64> = Vec::new();
    let mut bb: Vec<f64> = Vec::new();
    for _ in 0..10 {
        a.clear();
        bb.clear();
        for &x in &g {
            if (x - c1).abs() <= (x - c2).abs() {
                a.push(x);
            } else {
                bb.push(x);
            }
        }
        if !a.is_empty() {
            c1 = mean(&a);
        }
        if !bb.is_empty() {
            c2 = mean(&bb);
        }
    }
    let sd = ((ss(&a, c1) + ss(&bb, c2)) / 1f64.max(g.len() as f64 - 2.0)).sqrt();
    let sd = sd.max(GAP_SD_MIN);
    let (gc, gw);
    if a.len() >= 3
        && !bb.is_empty()
        && c2 - c1 > GAP_SPLIT_RATIO.ln()
        && c2 - c1 > GAP_SPLIT_SD * sd
    {
        gc = c1.exp() / CHAR_GAP;
        gw = c2.exp() / WORD_GAP;
    } else {
        let med = s[s.len() / 2];
        gc = med.exp() / CHAR_GAP;
        gw = gw0.max(gc * CHAR_GAP * WORD_GAP_MIN_REL / WORD_GAP);
    }
    (gc.clamp(GC_MIN, G_MAX), gw.clamp(GW_MIN, G_MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(t: &mut Timing, code: &str, u: f64) {
        // one word of characters separated by character gaps, ended by a long gap
        let mut first = true;
        for ch in code.split(' ') {
            if !first {
                t.add_element(Element {
                    mark: false,
                    d: 3.0 * u,
                });
            }
            first = false;
            for (i, e) in ch.chars().enumerate() {
                if i > 0 {
                    t.add_element(Element { mark: false, d: u });
                }
                t.add_element(Element {
                    mark: true,
                    d: if e == '.' { u } else { 3.0 * u },
                });
            }
        }
    }

    fn run(code: &str, lead: f64) -> (String, Hyp) {
        let mut t = Timing::new();
        let u = 0.06;
        t.add_element(Element {
            mark: false,
            d: lead * u,
        });
        feed(&mut t, code, u);
        let mut g = 0.0;
        while g < 1.5 {
            g += 0.005;
            t.tick(g, true, false);
        }
        t.add_element(Element { mark: false, d: g });
        (t.take_out().0, t.best())
    }

    #[test]
    fn decodes_elements_and_tracks_speed() {
        let (text, best) = run("-.-. --.- - . ... -", 10.0);
        assert_eq!(text.trim(), "CQTEST");
        assert!((1.2 / best.u - 20.0).abs() < 2.0, "wpm {}", 1.2 / best.u);
    }

    #[test]
    fn first_character_needs_a_leading_character_gap() {
        // the tail of a cut-off C (dah, dit) without a preceding gap is not committed
        let (text, _) = run("-. --.- - . ... -", 0.0);
        assert_eq!(text.trim(), "QTEST");
        let (text, _) = run("-. --.- - . ... -", 0.8);
        assert_eq!(text.trim(), "QTEST");
        // a leading silence of a character gap shows the start of the character
        let (text, _) = run("-. --.- - . ... -", 3.0);
        assert_eq!(text.trim(), "NQTEST");
        // a code that ends no other code cannot be a tail
        let (text, _) = run("--.- - . ... -", 0.8);
        assert_eq!(text.trim(), "QTEST");
    }

    /// Decodes words (separated by word gaps) after a leading silence of `lead` units, the
    /// track's look-back reaching the stream start.
    fn run_start(words: &[&str], lead: f64) -> (String, Vec<CharMeta>) {
        let mut t = Timing::new();
        t.c.at_start = true;
        let u = 0.06;
        t.add_element(Element {
            mark: false,
            d: lead * u,
        });
        for (i, w) in words.iter().enumerate() {
            if i > 0 {
                t.add_element(Element {
                    mark: false,
                    d: 7.0 * u,
                });
            }
            feed(&mut t, w, u);
        }
        let mut g = 0.0;
        while g < 1.5 {
            g += 0.005;
            t.tick(g, true, false);
        }
        t.add_element(Element { mark: false, d: g });
        t.finish();
        t.take_out()
    }

    #[test]
    fn first_character_at_stream_start_is_kept_if_it_completes_a_callsign() {
        // O (the tail of J) after 0.4 units of silence: OO1ZQG/1 is a callsign and
        // O1ZQG/1 is none
        let call = "--- --- .---- --.. --.- --. -..-. .----";
        let (text, meta) = run_start(&[call], 0.4);
        assert_eq!(text.trim(), "OO1ZQG/1");
        // the kept character is doubtful: the syntax that decided it cannot confirm it
        assert!(meta[0].doubt && !meta[1].doubt);
        // T (the tail of many codes) before JA1ZLO: TJA1ZLO is no callsign
        let (text, _) = run_start(&["- .--- .- .---- --.. .-.. ---"], 0.4);
        assert_eq!(text.trim(), "JA1ZLO");
        // a first character that is a word of its own is dropped
        let (text, _) = run_start(&["-", ".--- .- .---- --.. .-.. ---"], 0.4);
        assert_eq!(text.trim(), "JA1ZLO");
        // without the stream start the first character is dropped as before
        let (text, _) = run(call, 0.4);
        assert_eq!(text.trim(), "O1ZQG/1");
    }

    /// Decodes C Q T E S T on a guarded track, the elements of the character at index
    /// `flag` lying under a stronger station's edges.
    fn guarded_run(flag: usize) -> String {
        let u = 0.06;
        let mut t = Timing::new();
        t.c.outshone = true;
        t.c.guarded = true;
        t.add_element(Element {
            mark: false,
            d: 10.0 * u,
        });
        for (n, ch) in "-.-. --.- - . ... -".split(' ').enumerate() {
            if n > 0 {
                t.add_element(Element {
                    mark: false,
                    d: 3.0 * u,
                });
            }
            for (i, e) in ch.chars().enumerate() {
                if i > 0 {
                    t.c.next_unsure = n == flag;
                    t.add_element(Element { mark: false, d: u });
                }
                t.c.next_unsure = n == flag;
                t.add_element(Element {
                    mark: true,
                    d: if e == '.' { u } else { 3.0 * u },
                });
            }
        }
        let mut g = 0.0;
        while g < 1.5 {
            g += 0.005;
            t.tick(g, true, false);
        }
        t.add_element(Element { mark: false, d: g });
        t.take_out().0.trim().to_string()
    }

    #[test]
    fn guarded_track_holds_characters_keyed_under_edges() {
        let clean = guarded_run(usize::MAX);
        assert!(clean.starts_with("CQT"), "{clean}");
        assert_eq!(guarded_run(1), clean.replacen('Q', "", 1));
    }

    #[test]
    fn tree_prior_prefers_whole_characters() {
        let tr = tree();
        let n = |c: &str| node_of(c) as usize;
        // weights of the valid branches at a node sum to their number
        for c in ["", "-", "-.", "...", "-.-"] {
            assert!((tr.w_dit[n(c)] + tr.w_dah[n(c)] - 2.0).abs() < 1e-12);
            assert!((tr.w_cont[n(c)] + tr.w_end[n(c)] - 2.0).abs() < 1e-12 || c.is_empty());
        }
        // a leaf has no alternative to ending; after "-.-" continuing (C, Y) is weighed
        // against ending as K
        assert_eq!(tr.w_end[n("-.-.")], 1.0);
        assert!(tr.w_cont[n("-.-")] > 0.0 && tr.w_end[n("-.-")] > 0.0);
        // the characters below a dit outweigh E, so ending after one dit weighs less
        assert!(tr.w_end[n(".")] < 1.0);
    }

    #[test]
    fn tails_of_other_codes() {
        assert!(tail_of_other('N', false) && tail_of_other('J', false));
        assert!(!tail_of_other('Q', false) && !tail_of_other('5', false));
        // with a cut first mark, 5 may be the end of 6 and C stays safe
        assert!(tail_of_other('5', true) && !tail_of_other('C', true));
        assert!(tail_of_other('*', false));
    }

    #[test]
    fn tree_nodes() {
        assert_eq!(node_of(""), ROOT);
        assert_eq!(node_code(node_of(".-..")), ".-..");
        let tr = tree();
        assert_eq!(tr.ch[node_of("-.-") as usize], b'K');
        assert!(tr.ok[node_of("..--") as usize]);
        assert!(!tr.ok[node_of("-----.") as usize]);
    }

    #[test]
    fn interned_texts() {
        let mut t = Texts::new();
        let a = t.append(0, b'A');
        let a2 = t.append(0, b'A');
        assert_eq!(a, a2);
        let sp = t.append(a, b' ');
        let b = t.append(sp, b'B');
        let s0 = t.intern(b" B".to_vec());
        assert_eq!(t.strip(s0), (true, b'B', 0));
        let (has, c, rest) = t.strip(b);
        assert_eq!((has, c), (false, b'A'));
        assert_eq!(t.data[rest as usize], b" B".to_vec());
        assert_eq!(t.strip(0), (false, 0, 0));
    }

    #[test]
    fn packed_keys_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for node in [1u8, 2, 3, 127] {
            for word in [false, true] {
                for wl in 0..3 {
                    for bi in -2..=2 {
                        for txt in [0u32, 1, 70000] {
                            assert!(seen.insert(
                                Key {
                                    node,
                                    word,
                                    wl,
                                    bi,
                                    txt
                                }
                                .pack()
                            ));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn fit_gaps_finds_two_clusters() {
        let gaps: Vec<f64> = (0..20)
            .map(|i| if i % 4 == 0 { 7.0 } else { 3.0 } * (1.0 + 0.01 * (i % 3) as f64))
            .collect();
        let (gc, gw) = fit_gaps(&gaps, 1.0, 0.0, 1.0, 1.0);
        assert!(
            (gc - 1.0).abs() < 0.05 && (gw - 1.0).abs() < 0.05,
            "{gc} {gw}"
        );
    }
}
