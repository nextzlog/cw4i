//! Soft keyer: explicit-duration (stage-chain) HSMM over keying states, decoded by
//! Viterbi with a fixed lag, plus a forward pass whose normaliser accumulates the
//! log-likelihood ratio of "Morse keying" versus "noise only".

use crate::model::Hyp;
use crate::Element;

const NEG_INF: f64 = f64::NEG_INFINITY;

/// ln I0(z) for z >= 0 (Abramowitz-Stegun 9.8.1/9.8.2).
#[inline]
pub fn ln_i0(z: f64) -> f64 {
    if z < 3.75 {
        let mut t = z / 3.75;
        t *= t;
        return (1.0
            + t * (3.5156229
                + t * (3.0899424
                    + t * (1.2067492 + t * (0.2659732 + t * (0.0360768 + t * 0.0045813))))))
            .ln();
    }
    let t = 3.75 / z;
    let p = 0.39894228
        + t * (0.01328592
            + t * (0.00225319
                + t * (-0.00157565
                    + t * (0.00916281
                        + t * (-0.02057706
                            + t * (0.02635537 + t * (-0.01647633 + t * 0.00392377)))))));
    z - 0.5 * z.ln() + p.ln()
}

#[inline]
pub fn logadd(a: f64, b: f64) -> f64 {
    if a == NEG_INF {
        return b;
    }
    if b == NEG_INF {
        return a;
    }
    let (a, b) = if a < b { (b, a) } else { (a, b) };
    a + (b - a).exp().ln_1p()
}

/// Element types: dit, dah, element gap (1 unit), character gap (3), word gap (7).
const E_DIT: usize = 0;
const E_DAH: usize = 1;
const E_G1: usize = 2;
const E_G3: usize = 3;
const E_G7: usize = 4;
const N_TYPES: usize = 5;
/// Stages per element type (the longest forced-minimum chain).
const K_MAX: usize = 32;
const N_STATES: usize = N_TYPES * K_MAX;
/// Fixed decision lag of the Viterbi output in frames.
const LAG: usize = 20;
/// Frames of evidence history kept (at least the evidence window).
const LAM_HIS: usize = 600;
/// Probabilities of the gap that follows a mark: element, character and word gap.
const P_AFTER_MARK: [f64; 3] = [0.55, 0.35, 0.10];
/// Probability of a dit (and of a dah) after a gap.
const P_AFTER_GAP: f64 = 0.5;
/// Fraction of the expected duration covered by forced stages while the speed is uncertain.
const K_FRAC_BROAD: f64 = 0.3;
/// Smallest mean length of the geometric tail of a chain.
const TAIL_MIN: f64 = 1.0001;
/// Self-loop probability of the last word-gap stage (silence can last long).
const WORD_GAP_STAY: f64 = 0.997;
const WORD_GAP_LEAVE: f64 = 0.003;
/// Unit (s) used before the timing model has an estimate (22 words per minute).
const DEFAULT_UNIT: f64 = 1.2 / 22.0;
/// Default dah ratio and character gap in units.
const DEFAULT_RATIO: f64 = 3.0;
const DEFAULT_CHAR_GAP: f64 = 3.0;

pub struct SoftKeyer {
    dt: f64,
    kappa: f64,
    kfrac: f64,
    h: Hyp,
    k: [usize; N_TYPES],
    stay: [f64; N_STATES],
    adv: [f64; N_STATES],
    built: bool,
    delta: [f64; N_STATES],
    alpha: [f64; N_STATES],
    nd: [f64; N_STATES],
    na: [f64; N_STATES],
    bp: Box<[[u8; N_STATES]; LAG]>,
    t: usize,
    lam: f64,
    lam_his: Vec<f64>,
    lam_start: usize,
    pub cur_mark: bool,
    run: usize,
    started: bool,
    broad: bool,
    end_lp: [[f64; 3]; 2],
}

#[inline]
fn first(j: usize) -> usize {
    j * K_MAX
}

#[inline]
fn typ(i: usize) -> usize {
    i / K_MAX
}

impl SoftKeyer {
    pub fn new(dt: f64, kappa: f64, kfrac: f64) -> SoftKeyer {
        let mut s = SoftKeyer {
            dt,
            kappa,
            kfrac,
            h: Hyp::default(),
            k: [1; N_TYPES],
            stay: [0.0; N_STATES],
            adv: [0.0; N_STATES],
            built: false,
            delta: [NEG_INF; N_STATES],
            alpha: [NEG_INF; N_STATES],
            nd: [NEG_INF; N_STATES],
            na: [NEG_INF; N_STATES],
            bp: Box::new([[0u8; N_STATES]; LAG]),
            t: 0,
            lam: 0.0,
            lam_his: Vec::with_capacity(2 * LAM_HIS),
            lam_start: 0,
            cur_mark: false,
            run: 0,
            started: false,
            broad: false,
            end_lp: [
                [
                    P_AFTER_MARK[0].ln(),
                    P_AFTER_MARK[1].ln(),
                    P_AFTER_MARK[2].ln(),
                ],
                [P_AFTER_GAP.ln(), P_AFTER_GAP.ln(), 0.0],
            ],
        };
        let last = first(E_G7) + K_MAX - 1;
        s.delta[last] = 0.0;
        s.alpha[last] = 0.0;
        s
    }

    /// Sets the duration chains for timing h. The state space is fixed (K_MAX stages
    /// per element type); a chain of k stages is entered at stage K_MAX-k.
    pub fn build(&mut self, h: Hyp, broad: bool) {
        let h = if h.u == 0.0 {
            Hyp {
                u: DEFAULT_UNIT,
                r: DEFAULT_RATIO,
                b: 0.0,
                cg: DEFAULT_CHAR_GAP,
            }
        } else {
            h
        };
        if self.built
            && broad == self.broad
            && (h.u / self.h.u).ln().abs() < 0.02
            && h.r == self.h.r
            && h.b == self.h.b
            && h.cg == self.h.cg
        {
            return;
        }
        self.built = true;
        let means = [
            1.0 + h.b,
            h.r + h.b,
            1.0 - h.b,
            h.cg - h.b,
            h.cg * 7.0 / 3.0 - h.b,
        ];
        self.h = h;
        self.broad = broad;
        for (j, &mean) in means.iter().enumerate() {
            let l = (mean * h.u / self.dt).max(1.0);
            let kk = if broad {
                (K_FRAC_BROAD * l) as i64
            } else {
                (self.kfrac * l) as i64
            };
            let kk = kk.clamp(1, K_MAX as i64) as usize;
            self.k[j] = kk;
            let tail: f64 = (l - kk as f64 + 1.0).max(TAIL_MIN);
            for i in 0..K_MAX {
                let q = first(j) + i;
                self.stay[q] = NEG_INF;
                self.adv[q] = 0.0;
                if i == K_MAX - 1 {
                    self.stay[q] = (1.0 - 1.0 / tail).ln();
                    self.adv[q] = (1.0 / tail).ln();
                    if j == E_G7 {
                        self.stay[q] = WORD_GAP_STAY.ln();
                        self.adv[q] = WORD_GAP_LEAVE.ln();
                    }
                }
            }
        }
    }

    #[inline]
    fn entry(&self, j: usize) -> usize {
        first(j) + K_MAX - self.k[j]
    }

    /// Tempered Rician log-likelihood ratio of key down versus key up for one frame
    /// (x = power / noise power, s = key-down SNR).
    pub fn rician_llr(&self, x: f64, s: f64) -> f64 {
        self.kappa * (-s + ln_i0(2.0 * (s * x).sqrt()))
    }

    /// Consumes one frame given its key-down log-likelihood ratio and emits elements with a fixed lag.
    pub fn step(&mut self, llr: f64, mut emit: impl FnMut(Element)) {
        self.nd = [NEG_INF; N_STATES];
        self.na = [NEG_INF; N_STATES];
        let bpi = self.t % LAG;
        let entries = [
            self.entry(0),
            self.entry(1),
            self.entry(2),
            self.entry(3),
            self.entry(4),
        ];
        {
            let bp = &mut self.bp[bpi];
            for i in 0..N_STATES {
                let (v, a) = (self.delta[i] + self.stay[i], self.alpha[i] + self.stay[i]);
                if v > self.nd[i] {
                    self.nd[i] = v;
                    bp[i] = i as u8;
                }
                self.na[i] = logadd(self.na[i], a);
                let j = typ(i);
                if i + 1 < first(j) + K_MAX {
                    let (v, a) = (self.delta[i] + self.adv[i], self.alpha[i] + self.adv[i]);
                    if v > self.nd[i + 1] {
                        self.nd[i + 1] = v;
                        bp[i + 1] = i as u8;
                    }
                    self.na[i + 1] = logadd(self.na[i + 1], a);
                } else {
                    let (tos, lps): (&[usize], &[f64]) = if j <= E_DAH {
                        (&[E_G1, E_G3, E_G7], &self.end_lp[0][..])
                    } else {
                        (&[E_DIT, E_DAH], &self.end_lp[1][..2])
                    };
                    for (&to, &lp) in tos.iter().zip(lps) {
                        let f = entries[to];
                        let v = self.delta[i] + self.adv[i] + lp;
                        let a = self.alpha[i] + self.adv[i] + lp;
                        if v > self.nd[f] {
                            self.nd[f] = v;
                            bp[f] = i as u8;
                        }
                        self.na[f] = logadd(self.na[f], a);
                    }
                }
            }
        }
        let mut best = NEG_INF;
        let mut bi = 0;
        let mut c = NEG_INF;
        for i in 0..N_STATES {
            if typ(i) <= E_DAH {
                self.nd[i] += llr;
                self.na[i] += llr;
            }
            if self.nd[i] > best {
                best = self.nd[i];
                bi = i;
            }
            c = logadd(c, self.na[i]);
        }
        for i in 0..N_STATES {
            self.delta[i] = self.nd[i] - best;
            self.alpha[i] = self.na[i] - c;
        }
        self.lam += c;
        self.lam_his.push(self.lam);
        if self.lam_his.len() - self.lam_start > LAM_HIS {
            self.lam_start = self.lam_his.len() - LAM_HIS;
            if self.lam_start >= LAM_HIS {
                self.lam_his.drain(..self.lam_start);
                self.lam_start = 0;
            }
        }
        self.t += 1;
        if self.t < LAG {
            return;
        }
        let mut q = bi;
        for d in 0..LAG - 1 {
            q = self.bp[(self.t - 1 - d) % LAG][q] as usize;
        }
        let mark = typ(q) <= E_DAH;
        if !self.started {
            self.started = true;
            self.cur_mark = mark;
            self.run = 1;
            return;
        }
        if mark == self.cur_mark {
            self.run += 1;
            return;
        }
        emit(Element {
            mark: self.cur_mark,
            d: self.run as f64 * self.dt,
        });
        self.cur_mark = mark;
        self.run = 1;
    }

    /// Log LR accumulated over the last w frames.
    pub fn evidence(&self, w: usize) -> f64 {
        let n = self.lam_his.len() - self.lam_start;
        if n == 0 {
            return 0.0;
        }
        if n < 1 + w {
            return self.lam;
        }
        self.lam - self.lam_his[self.lam_start + n - 1 - w]
    }

    /// Open space duration known at the output (lagged).
    pub fn gap(&self) -> f64 {
        if !self.started || self.cur_mark {
            0.0
        } else {
            self.run as f64 * self.dt
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ln_i0_values() {
        // ln I0(z) for small and large arguments (I0(1) = 1.2660658777, I0(10) = 2815.716628)
        assert!(ln_i0(0.0).abs() < 1e-12);
        assert!((ln_i0(1.0) - 1.266_065_877_752_008_4f64.ln()).abs() < 1e-6);
        assert!((ln_i0(10.0) - 2_815.716_628_466_254f64.ln()).abs() < 1e-6);
    }

    #[test]
    fn logadd_is_log_of_sum() {
        assert!((logadd(1f64.ln(), 2f64.ln()) - 3f64.ln()).abs() < 1e-15);
        assert_eq!(logadd(NEG_INF, 0.5), 0.5);
        assert_eq!(logadd(0.5, NEG_INF), 0.5);
    }

    #[test]
    fn keys_a_clean_pattern() {
        // 25 WPM at 5 ms frames: dit 48 ms (about 10 frames)
        let dt = 0.005;
        let mut sk = SoftKeyer::new(dt, 0.35, 0.6);
        sk.build(
            Hyp {
                u: 0.048,
                r: 3.0,
                b: 0.0,
                cg: 3.0,
            },
            false,
        );
        let pattern = [
            (false, 60),
            (true, 10),
            (false, 10),
            (true, 29),
            (false, 30),
            (true, 10),
            (false, 80),
        ];
        let mut els = Vec::new();
        for &(on, n) in &pattern {
            for _ in 0..n {
                let x = if on { 100.0 } else { 1.0 };
                let llr = sk.rician_llr(x, 99.0);
                sk.step(llr, |e| els.push(e));
            }
        }
        let marks: Vec<f64> = els.iter().filter(|e| e.mark).map(|e| e.d).collect();
        assert_eq!(marks.len(), 3, "{els:?}");
        assert!(
            (marks[0] - 0.05).abs() < 0.011 && (marks[1] - 0.145).abs() < 0.011,
            "{marks:?}"
        );
        assert!(sk.evidence(300) > 8.0);
    }

    #[test]
    fn noise_gives_low_evidence() {
        let mut sk = SoftKeyer::new(0.005, 0.35, 0.6);
        sk.build(Hyp::default(), true);
        let mut s: u64 = 1;
        for _ in 0..2000 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let u = (s >> 11) as f64 / (1u64 << 53) as f64;
            let llr = sk.rician_llr(-(u.max(1e-12)).ln(), 2.0);
            sk.step(llr, |_| {});
        }
        assert!(sk.evidence(300) < 8.0, "{}", sk.evidence(300));
    }
}
