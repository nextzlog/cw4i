//! Streaming multi-signal decoder core.
//!
//! Each frame (5 ms hop), the complex spectrum is explained as a sum of window-shaped
//! lines at the frequencies of the tracked stations, fitted jointly by least squares.
//! Lines that remain in the residual and persist become new tracks, decoded
//! retroactively from a look-back buffer. Each track refines its frequency from the
//! phase advance of its own amplitude, keys marks with a soft (HSMM) keyer, and decodes
//! characters with a timing model; characters are committed only when their
//! interpretation is unambiguous, and never revised.

use crate::complex::C64;
use crate::confirm;
use crate::fast::FastKeyer;
use crate::frontend::Frontend;
use crate::hsmm::SoftKeyer;
use crate::keyer::Keyer;
use crate::model::{CharMeta, Hyp, GAMMA, PARIS, WPM_HI};
use crate::nn::{self, NnState, INPUTS};
use crate::special::remainder;
use crate::timing::Timing;
use crate::{Config, Element};
use std::collections::{HashMap, VecDeque};
use std::f64::consts::PI;

/// Gap in seconds assumed after the last element when a stream is flushed.
const FLUSH_GAP: f64 = 10.0;
/// Frames over which the per-bin noise floor starts as a running mean.
const FLOOR_WARMUP: i64 = 100;
/// Residual power above this multiple of the floor is excluded from the floor update.
const FLOOR_OUTLIER: f64 = 10.0;
/// Update rate of the log-domain noise floor after the warm-up.
const FLOOR_RATE: f64 = 0.01;
/// Share of the bins above the outlier level that marks a lifted noise level (receiver
/// AGC or a level step) rather than signals, and the faster update rate then used.
const FLOOR_LIFT: f64 = 0.5;
const FLOOR_RATE_LIFT: f64 = 0.1;
/// Frames of noise floor estimation before births are allowed.
const BIRTH_WARMUP: i64 = 40;
/// Weight of the previous value in the smoothed residual SNR per bin.
const SNR_KEEP: f64 = 0.6;
/// Weight of the new frame in the smoothed residual SNR per bin.
const SNR_NEW: f64 = 0.4;
/// Attenuation (dB) of a track's skirt mask next to its line.
const MASK_ATT_DB: f64 = 25.0;
/// Offset (Hz) beyond which the skirt mask attenuation grows.
const MASK_FLAT_HZ: f64 = 30.0;
/// Growth of the skirt mask attenuation in dB per Hz.
const MASK_SLOPE_DB_HZ: f64 = 0.2;
/// Largest skirt mask attenuation (dB).
const MASK_MAX_DB: f64 = 60.0;
/// Attenuation (dB) of the steady skirt of a track's current power within
/// `EDGE_GATE_HZ`: a birth candidate below it starts again.
const STEADY_ATT_DB: f64 = 40.0;
/// Range (Hz) within which a candidate explained by a louder track's skirt or keying-edge
/// leakage is frozen (neither counted nor reset) instead of reset.
const EDGE_GATE_HZ: f64 = 150.0;
/// Frames a frozen candidate keeps its count (about the passage of one edge).
const FREEZE_FRAMES: i64 = 10;
/// Spectra kept, half width (bins) and steps of the joint position search of a new line.
const JOINT_FRAMES: usize = 4;
const JOINT_SPAN: f64 = 2.0;
const JOINT_STEPS: usize = 16;
/// Floor of the birth threshold relative to the strongest residual (dynamic range).
const BIRTH_RANGE: f64 = 1e-5;
/// Decay per frame of the remembered strongest residual.
const GMAX_DECAY: f64 = 0.9995;
/// Half width (Hz) of the frequency window of the local order statistic (CFAR).
const CFAR_HZ: f64 = 150.0;
/// Guard half width (Hz) excluded around the cell under test.
const CFAR_GUARD_HZ: f64 = 30.0;
/// Median of an exponential distribution relative to its mean.
const EXP_MEDIAN: f64 = 0.69;
/// Minimum spacing (Hz) of tracks born in the same frame.
const BIRTH_SPACING_HZ: f64 = 300.0;
/// The 33rd percentile of exponentially distributed power relative to its mean, for the
/// initial noise estimate (two roundings are used: a conservative one and the exact one).
const P33_LOW: f64 = 0.4;
const P33: f64 = 0.405;
/// The 10th percentile of exponentially distributed power relative to its mean.
const P10: f64 = 0.105;
/// Ratio of peak to quiet-third power below which keying may fill the quiet third.
const HIGH_DUTY: f64 = 10.0;
/// Smallest noise level relative to the peak.
const NOISE_MIN_REL: f64 = 1e-7;
/// Smallest initial key-down level relative to the noise.
const PEAK_MIN_REL: f64 = 10.0;
/// Silence (s) after which an unconfirmed track is dropped.
const CANDIDATE_DEAD_SEC: f64 = 2.5;
/// Value of `last_on` that marks a track for removal.
const KILL: i64 = 1 << 30;
/// Frame power above this multiple of the noise allows frequency refinement.
const REFINE_SNR: f64 = 10.0;
/// Gain of the phase-advance frequency refinement.
const REFINE_GAIN: f64 = 0.03;
/// Speed ambiguity at or below which the timing estimate is not trusted.
const AMBIG_LOW: f64 = 3.0;
/// Decision lag of the soft keyer in frames.
const SOFT_LAG: i64 = 20;
/// Smallest key-down SNR given to the soft keyer.
const SOFT_SNR_MIN: f64 = 2.0;
/// A confirmed station within this distance (Hz) and this many dB above a track
/// outshines it.
const OUTSHONE_HZ: f64 = 300.0;
const OUTSHONE_DB: f64 = 6.0;
/// Number of keying edges remembered per track.
const EDGE_MEMORY: usize = 64;
/// Weight of the previous value in the smoothed key-down power.
const LEVEL_KEEP: f64 = 0.95;
/// Weight of the new frame in the smoothed key-down power.
const LEVEL_NEW: f64 = 0.05;
/// Decay per frame of the remembered peak power (masking).
const PEAK_DECAY: f64 = 0.98;
/// A neighbour at least this many times stronger can own a track's keying (ghost check).
const GHOST_LEVEL: f64 = 4.0;
/// Frequency range (Hz) of the ghost check.
const GHOST_HZ: f64 = 400.0;
/// A neighbour this many times stronger can own a track's keying at any distance (receiver
/// AGC pumps the whole band with its keying).
const GHOST_FAR_LEVEL: f64 = 4.0;
/// Morse evidence a confirmed track needs for its pending text to be flushed at its end.
const FLUSH_EV: f64 = 8.0;
/// Morse evidence needed to confirm a track for `NOISY_FRAMES` after a static crash or a
/// lifted noise level (receiver AGC), when noise keys tracks like Morse.
const EV_CONF_NOISY: f64 = 300.0;
const NOISY_FRAMES: i64 = 2000;
/// AGC pumping: share of its key-down level at which the strong station counts as on,
/// frames needed on each side, and the ratio of key-down shares that marks pumping.
const PUMP_ON: f64 = 0.5;
const PUMP_FRAMES: f64 = 50.0;
const PUMP_RATIO: f64 = 0.25;
/// Frames within which an onset coincides with a neighbour's edge.
const GHOST_FRAMES: i64 = 4;
/// Fraction of coinciding onsets that marks a ghost.
const GHOST_FRAC: f64 = 0.6;
/// Range (Hz) of louder neighbours that suspend frequency refinement.
const DOMINANCE_HZ: f64 = 80.0;
/// Scale of the network's log-odds in the soft keyer and its bound.
const NN_TAU: f64 = 1.0;
const NN_CLIP: f64 = 8.0;
/// Frames over which a new track's likelihood moves from the Rician model to the network;
/// a track born beside a somewhat louder track (a pile-up) counts only live frames (its look-back is
/// replayed with the Rician model).
const NN_BLEND: u32 = 200;
const CROWD_RATIO: f64 = 30.0;
/// Release of the log-power peak follower per frame (about 43 dB/s).
const NN_PEAK_RELEASE: f64 = 0.05;
/// The noise reference stays within this log-power distance below the peak (40 dB).
const NN_RANGE: f64 = 9.2;
/// Up and down steps of the noise quantile tracker (about the 20th percentile).
const NN_UP: f64 = 0.005;
const NN_DOWN: f64 = 0.02;
/// Range (Hz) of the neighbour features.
const NN_NEIGHBOUR_HZ: f64 = 300.0;
/// Bounds of the log-power features.
const NN_LA: (f64, f64) = (-10.0, 15.0);
const NN_LB: (f64, f64) = (-10.0, 20.0);
const NN_LP: (f64, f64) = (-15.0, 0.0);
/// Smallest power in a logarithm.
const POWER_MIN: f64 = 1e-30;
/// Regularisation of the joint fit relative to the mean diagonal of the Gram matrix.
const FIT_REG: f64 = 1e-3;
/// Ridge on the quadrature ramp relative to its diagonal and update rate of its slow
/// average.
const QUAD_RIDGE: f64 = 4.0;
const QUAD_RATE: f64 = 0.02;
/// Keying-edge leakage model: amplitude share below which a frame has no edge, knee of
/// the low side, amplitude step per frame of a full edge, and noise multiple subtracted.
const EDGE_U_MIN: f64 = 0.05;
const EDGE_U_KNEE: f64 = 0.12;
const EDGE_STEP: f64 = 0.1;
const EDGE_NOISE: f64 = 2.0;
/// Key-down level over the line noise above which an unconfirmed track with Morse
/// evidence is a source of edge leakage.
const SOURCE_SNR: f64 = 10.0;
/// Edge leakage, relative to a track's key-down power, above which the key-state network
/// is given the last clean frame instead.
const HOLD_LEAK: f64 = 0.1;
/// Edge leakage, relative to a track's key-down power, that makes nearby element
/// boundaries unsure, and the distance (frames) of such a boundary.
const UNSURE_LEAK: f64 = 0.3;
const UNSURE_FRAMES: i64 = 2;
/// Common fate: a louder neighbour is silent below this share of its key-down power;
/// a track key-down in this many such frames, and this share of them, is keyed
/// independently.
const QUIET_FRAC: f64 = 0.01;
const FATE_ON: u32 = 10;
const FATE_SHARE: f64 = 0.1;
/// A ghost misses at most this share of the onsets that chance coincidence would miss.
const GHOST_MISS: f64 = 0.5;
/// Chance probability of the coinciding onsets below which a track is a ghost.
const GHOST_CHANCE: f64 = 0.01;
/// A confirmed station within this distance (Hz) and this many dB above a track guards
/// it: characters below the full commit threshold, or keyed under its edges, are held.
const GUARD_HZ: f64 = 70.0;
const GUARD_DB: f64 = 10.0;

/// Keying spectrum within the main lobe of a track's shortest element, relative to its
/// key-down power times the element over the window (random keying),
/// and the soft keyer's elements used to estimate the element.
const KEY_SPECTRUM: f64 = 0.25;
const KEY_ELEMENTS: usize = 16;
/// Frames after its birth in which a track's element is taken as the shortest of the grid.
const KEY_YOUNG: i64 = 40;
/// Short-window keying: started on a track whose shortest element is at most
/// `FAST_GATE` (s) at an SNR above `FAST_SNR`, used from when its own shortest element
/// and its timing's speed are below `FAST_ON` until the element exceeds `FAST_OFF`.
const FAST_GATE: f64 = 0.03;
const FAST_SNR: f64 = 10.0;
/// Largest shortest element (s) of the short-window keyer that starts its timing.
const FAST_GATE_FK: f64 = 0.02;
const FAST_ON: f64 = 0.015;
const FAST_OFF: f64 = 0.03;
/// Shorter elements (s) are taken as noise or interference, not as keying.
const FAST_MIN: f64 = 0.0085;
/// A louder track within this distance (Hz) keeps a track on the 40 ms keyer.
const FAST_CLEAR_HZ: f64 = 200.0;
/// Morse evidence by which the other keyer's timing must lead to take over.
const FAST_LLR: f64 = 15.0;
/// Short-window elements kept for the timing started by the gate.
const FAST_REPLAY: usize = 64;
const SENT_CHARS: usize = 8;

/// Static crash: power over the bin floor and share of the bins above it.
const CRASH_RISE: f64 = 2.0;
const CRASH_SHARE: f64 = 0.4;
/// Smallest median over mean of the residual power of a (flat) crash; noise gives 0.69.
const CRASH_FLAT: f64 = 0.3;
/// Largest peak over mean of a crash (a tone stands out; noise of 130 bins gives about 5).
const CRASH_PEAK: f64 = 15.0;
/// Smallest crash bin power relative to the loudest track's line peak (rounding residue).
const CRASH_MIN_REL: f64 = 1e-5;
/// Bin floor below which the input is digital silence and has no crashes.
const CRASH_FLOOR_MIN: f64 = 1e-20;
/// Frames after a crash without births (its decay and the window).
const CRASH_HOLD: i64 = 20;
/// Key-down level over the line noise of an unconfirmed track whose clicks are not crashes.
const CRASH_SOURCE: f64 = 1000.0;

/// A frame below this multiple of the band's current noise is not key-down while that
/// noise is lifted by `BAND_LIFT` above its slow average (`BAND_RATE` per frame), as a
/// receiver AGC does after a strong station.
const BAND_GATE: f64 = 4.0;
const BAND_LIFT: f64 = 3.0;
const BAND_RATE: f64 = 0.01;

/// Natural log of 10 over 10: power ratio exp(-dB * DB_LN).
const DB_LN: f64 = std::f64::consts::LN_10 / 10.0;

/// Committed text of one track (internal event).
#[derive(Clone, Debug)]
pub struct Out {
    pub track: u32,
    pub freq: f64,
    pub text: String,
    pub meta: Vec<CharMeta>,
}

#[derive(Clone, Copy)]
struct Edge {
    fr: i64,
    on: bool,
}

pub struct Track {
    pub id: u32,
    /// Frequency in fractional bins of the zero-padded FFT.
    pub nu: f64,
    c: C64,
    prev_c: C64,
    pub key: Keyer,
    pub tim: Timing,
    pub confirmed: bool,
    pub born: i64,
    pub level: f64,
    edges: VecDeque<Edge>,
    peak_e: f64,
    /// Smoothed share of the worst-case keying-edge leakage (births).
    edge_m: f64,
    /// Line power smoothed like the residual SNR of births.
    e_sm: f64,
    /// Frames in which every clearly louder confirmed neighbour was silent, and those of
    /// them in which this track was key-down (common fate).
    quiet: u32,
    quiet_on: u32,
    /// Frames in which the loudest much stronger confirmed track is key-down or silent,
    /// and those of them in which this track is key-down (receiver AGC pumping).
    pump: [u32; 4],
    /// Slow average of the quadrature ramp over the line (2 pi T times the frequency
    /// error).
    quad: f64,
    /// Line power of the last frame without strong edge leakage.
    e_held: f64,
    /// Frames (bit j: j frames ago) dominated by a louder track's edge leakage.
    leak_bits: u128,
    /// Frame in which the soft keyer last decided a mark.
    soft_mark: i64,
    /// Recent element durations of the soft keyer and their lower quartile (s), 0 if unknown.
    durs: VecDeque<f64>,
    key_unit: f64,
    fk: FastKeyer,
    /// Timing fed by the inactive keyer (the short-window one until `fast`).
    alt: Option<Timing>,
    fast: bool,
    /// A confirmed track whose transmission ended holds its text until it shows Morse
    /// again (characters counted from `dormant_n`).
    dormant: bool,
    dormant_n: i64,
    /// Born within `OUTSHONE_HZ` of a track louder by less than `CROWD_RATIO`.
    crowded_birth: bool,
    /// End of the delivered text, without spaces.
    sent: Vec<char>,
    pub sk: Option<SoftKeyer>,
    nst: NnState,
}

impl Track {
    fn gap(&self) -> f64 {
        if self.fast {
            self.fk.gap()
        } else {
            self.key.gap()
        }
    }
}

/// Snapshot of one track in one frame (neighbour features of the key-state network and
/// neighbour state of the edge leakage model).
#[derive(Clone, Copy)]
struct NbRec {
    id: u32,
    nu: f64,
    la: f64,
    pc: f64,
    level: f64,
    /// Line power, share of the worst-case edge leakage in this frame.
    e: f64,
    edge: f64,
    confirmed: bool,
    /// Whether its keying edges are a leakage source.
    source: bool,
}

/// Parameters shared by the per-track functions.
struct Ctx<'a> {
    cfg: &'a Config,
    rate: f64,
    m: f64,
    /// Per-bin noise floor, its first bin and the energy of a line in the bins.
    floor: &'a [f64],
    lo: usize,
    knorm: f64,
    /// Recent frames with a static crash.
    crashes: &'a VecDeque<i64>,
    /// Noise power in a line coefficient from this frame's median residual bin while it is
    /// lifted well above its slow average (receiver AGC), else 0.
    band: f64,
    /// A static crash or a lifted noise level was seen recently.
    noisy: bool,
}

impl Ctx<'_> {
    #[inline]
    fn hz(&self, nu: f64) -> f64 {
        nu * self.rate / self.m
    }

    #[inline]
    fn hz_bin(&self, f: f64) -> f64 {
        f * self.m / self.rate
    }

    /// Noise floor at the bin nearest to frequency nu (bins).
    fn noise_at(&self, nu: f64) -> f64 {
        let k = (nu.round() as i64 - self.lo as i64).clamp(0, self.floor.len() as i64 - 1);
        self.floor[k as usize]
    }
}

pub struct Core {
    pub cfg: Config,
    pub fe: Frontend,
    pub tracks: Vec<Track>,
    next_id: u32,
    floor: Vec<f64>,
    logm: Vec<f64>,
    nfloor: i64,
    smooth: Vec<f64>,
    /// Birth candidates per bin: frames counted and the frame last counted.
    cand: HashMap<i64, (i64, i64)>,
    ring: VecDeque<(Vec<C64>, Vec<NbRec>)>,
    pub frame_no: i64,
    pub out: Vec<Out>,
    gmax: f64,
    spec: Vec<C64>,
    /// The last spectra (joint position of new lines).
    specs: VecDeque<Vec<C64>>,
    /// Keyer output (track, frame, element) when recording is enabled.
    pub elements: Option<Vec<(u32, i64, Element)>>,
    /// Audio of the look-back span, replayed through a new track's short-window keyer.
    raw: VecDeque<f64>,
    crash_until: i64,
    crashes: VecDeque<i64>,
    band: f64,
    band_slow: f64,
    noisy_fr: i64,
}

impl Core {
    pub fn new(rate: usize, cfg: Config) -> Core {
        let fe = Frontend::new(rate, cfg.win_sec, cfg.lo_hz, cfg.hi_hz);
        let nb = fe.hi - fe.lo;
        Core {
            cfg,
            fe,
            tracks: Vec::new(),
            next_id: 0,
            floor: vec![0.0; nb],
            logm: vec![0.0; nb],
            nfloor: 0,
            smooth: vec![0.0; nb],
            cand: HashMap::new(),
            ring: VecDeque::new(),
            frame_no: 0,
            out: Vec::new(),
            gmax: 0.0,
            spec: Vec::new(),
            specs: VecDeque::new(),
            elements: None,
            raw: VecDeque::new(),
            crash_until: 0,
            crashes: VecDeque::new(),
            band: 0.0,
            band_slow: 0.0,
            noisy_fr: -NOISY_FRAMES,
        }
    }

    #[inline]
    pub fn hz(&self, nu: f64) -> f64 {
        self.fe.bin_hz(nu)
    }

    /// Frame hop in seconds.
    pub fn frame_seconds(&self) -> f64 {
        self.fe.hop as f64 / self.fe.rate as f64
    }

    /// Appends samples to the input buffer.
    pub fn feed(&mut self, x: impl Iterator<Item = f64>) {
        self.fe.feed(x);
    }

    /// Processes the next complete frame, if any, and returns its end sample.
    pub fn next_frame(&mut self) -> Option<usize> {
        let mut spec = std::mem::take(&mut self.spec);
        let r = self.fe.next_frame(&mut spec).map(|start| {
            let x = self.fe.newest().to_vec();
            self.frame(&spec, &x);
            start + self.fe.n
        });
        self.spec = spec;
        r
    }

    /// Finalises all tracks (end of stream).
    pub fn flush(&mut self) {
        let ctx = Ctx {
            cfg: &self.cfg,
            rate: self.fe.rate as f64,
            m: self.fe.m as f64,
            floor: &self.floor,
            lo: self.fe.lo,
            knorm: self.fe.kernel.norm,
            crashes: &self.crashes,
            band: self.band,
            noisy: self.frame_no - self.noisy_fr < NOISY_FRAMES,
        };
        for t in &mut self.tracks {
            let ev = t.sk.as_ref().map_or(0.0, |k| k.evidence(self.cfg.ev_win));
            if !t.confirmed || t.dormant || ev < FLUSH_EV {
                continue;
            }
            t.tim.tick(t.gap() + FLUSH_GAP, true, true);
            t.tim.finish();
            let f = ctx.hz(t.nu);
            deliver(&mut self.out, t, f);
        }
    }

    /// Whether a flat broadband burst (static crash) lifts most bins of the residual well above
    /// their floors while no strong (confirmed, or 30 dB) track is on or keying (clicks);
    /// no track is born from it.
    fn crash(&self, resid: &[C64]) -> bool {
        if self.nfloor <= BIRTH_WARMUP {
            return false;
        }
        let up = resid
            .iter()
            .zip(&self.floor)
            .filter(|(v, &f)| v.re * v.re + v.im * v.im > CRASH_RISE * f)
            .count();
        if (up as f64) < CRASH_SHARE * resid.len() as f64 {
            return false;
        }
        let mut p: Vec<f64> = resid.iter().map(|v| v.re * v.re + v.im * v.im).collect();
        let k = p.len() / 2;
        let med = *p.select_nth_unstable_by(k, f64::total_cmp).1;
        let mean = p.iter().sum::<f64>() / p.len() as f64;
        let floor = self.floor.iter().sum::<f64>() / self.floor.len() as f64;
        let max = p.iter().cloned().fold(0.0, f64::max);
        if floor < CRASH_FLOOR_MIN
            || med < CRASH_FLAT * mean
            || med < CRASH_RISE * floor
            || max > CRASH_PEAK * mean
        {
            return false;
        }
        let k0 = self.fe.kernel.at(0.0).abs();
        let loud = self.tracks.iter().map(|t| t.peak_e).fold(0.0, f64::max) * k0 * k0;
        if med < CRASH_MIN_REL * loud {
            return false;
        }
        !self.tracks.iter().any(|t| {
            let nl = self.line_noise(t.nu);
            let e = t.c.re * t.c.re + t.c.im * t.c.im;
            let src = if t.confirmed {
                SOURCE_SNR
            } else {
                CRASH_SOURCE
            };
            t.level > src * nl
                && (e > SOURCE_SNR * nl || edge_fraction(e, t.prev_c, t.level, nl) > 0.0)
        })
    }

    /// Joint least squares for all tracks (line and ramp per track, the quadrature part
    /// of the ramp held near its slow average); returns the residual.
    fn fit(&mut self, spec: &[C64]) -> Vec<C64> {
        let mut resid = spec.to_vec();
        let lo = self.fe.lo as f64;
        let k = &self.fe.kernel;
        self.tracks
            .sort_by(|a, b| a.nu.partial_cmp(&b.nu).unwrap_or(std::cmp::Ordering::Equal));
        let mut i = 0;
        while i < self.tracks.len() {
            let mut j = i + 1;
            while j < self.tracks.len() && self.tracks[j].nu - self.tracks[j - 1].nu < 2.0 * k.r {
                j += 1;
            }
            let cl = &mut self.tracks[i..j];
            let b0 = (cl[0].nu - k.r - lo).floor().max(0.0) as usize;
            let b1 = (cl[cl.len() - 1].nu + k.r - lo)
                .ceil()
                .min((spec.len() - 1) as f64) as usize;
            let n = 2 * cl.len();
            let w = b1 - b0 + 1;
            let mut g = vec![C64::ZERO; n * n];
            let mut h = vec![C64::ZERO; n];
            let mut cols = vec![C64::ZERO; n * w];
            let mut span = vec![(0usize, 0usize); n];
            for (a, t) in cl.iter().enumerate() {
                let q0 = ((t.nu - k.r - lo).floor().max(b0 as f64) as usize).min(b1) - b0;
                let q1 = ((t.nu + k.r - lo).ceil().min(b1 as f64).max(b0 as f64) as usize) - b0 + 1;
                span[2 * a] = (q0, q1);
                span[2 * a + 1] = (q0, q1);
                for q in q0..q1 {
                    let kk = b0 + q;
                    cols[2 * a * w + q] = k.at(kk as f64 + lo - t.nu);
                    cols[(2 * a + 1) * w + q] = k.ramp(kk as f64 + lo - t.nu);
                }
            }
            for a in 0..n {
                for b in a..n {
                    let (q0, q1) = (span[a].0.max(span[b].0), span[a].1.min(span[b].1));
                    let mut s = C64::ZERO;
                    for q in q0..q1.max(q0) {
                        s += cols[a * w + q].conj() * cols[b * w + q];
                    }
                    g[a * n + b] = s;
                    g[b * n + a] = s.conj();
                }
                let mut s = C64::ZERO;
                for q in span[a].0..span[a].1 {
                    s += cols[a * w + q].conj() * spec[b0 + q];
                }
                h[a] = s;
            }
            let mut reg = 0.0;
            for a in 0..n {
                reg += g[a * n + a].re;
            }
            reg = FIT_REG * reg / n as f64;
            for a in 0..n {
                g[a * n + a] += C64::new(reg, 0.0);
            }
            let quads: Vec<f64> = cl.iter().map(|t| t.quad).collect();
            let c = solve_ramp(&g, &h, &quads);
            for (a, t) in cl.iter_mut().enumerate() {
                t.prev_c = t.c;
                t.c = c[2 * a];
                let e = t.c.re * t.c.re + t.c.im * t.c.im;
                if t.key.on && e > REFINE_SNR * t.key.noise {
                    let q = (c[2 * a + 1] * t.c.conj()).im / e;
                    t.quad += QUAD_RATE * (q - t.quad);
                }
                for q in 0..2 {
                    let (q0, q1) = span[2 * a + q];
                    let col = &cols[(2 * a + q) * w..(2 * a + q + 1) * w];
                    for kk in q0..q1 {
                        resid[b0 + kk] -= c[2 * a + q] * col[kk];
                    }
                }
            }
            i = j;
        }
        resid
    }

    fn frame(&mut self, spec: &[C64], x: &[f64]) {
        self.frame_no += 1;
        let resid = self.fit(spec);
        let mut p: Vec<f64> = resid.iter().map(|v| v.re * v.re + v.im * v.im).collect();
        let mid = p.len() / 2;
        let band =
            *p.select_nth_unstable_by(mid, f64::total_cmp).1 / EXP_MEDIAN / self.fe.kernel.norm;
        self.band_slow = if self.band_slow == 0.0 {
            band
        } else {
            self.band_slow + BAND_RATE * (band - self.band_slow)
        };
        self.band = if band > BAND_LIFT * self.band_slow {
            band
        } else {
            0.0
        };
        let crashed_now = self.crash(&resid);
        if crashed_now || self.band > 0.0 {
            self.noisy_fr = self.frame_no;
        }
        if crashed_now {
            self.crash_until = self.frame_no + CRASH_HOLD;
            self.cand.clear();
            self.crashes.push_back(self.frame_no);
            if self.crashes.len() > EDGE_MEMORY {
                self.crashes.pop_front();
            }
        }
        let crashed = self.frame_no <= self.crash_until;
        let rate = self.fe.rate as f64;
        self.raw.extend(x);
        let keep = self.cfg.lookback * self.fe.hop;
        if self.raw.len() > keep {
            self.raw.drain(..self.raw.len() - keep);
        }
        for i in 0..self.tracks.len() {
            let hz = self.hz(self.tracks[i].nu);
            let lv = self.tracks[i].level;
            let crowded = self
                .tracks
                .iter()
                .any(|o| (self.hz(o.nu) - hz).abs() < FAST_CLEAR_HZ && o.level > lv);
            let t = &mut self.tracks[i];
            let fu = t.fk.unit();
            if t.alt.is_some() && !t.fast && fu > FAST_OFF {
                t.alt = None;
            }
            if t.alt.is_none()
                && !crowded
                && t.key_unit > 0.0
                && t.key_unit <= FAST_GATE
                && fu > 0.0
                && fu < FAST_GATE_FK
                && t.key.peak > FAST_SNR * t.key.noise
            {
                let mut a = Timing::fast();
                a.c.at_start = t.tim.c.at_start;
                t.alt = Some(a);
            }
            t.fk.feed(x, hz, rate);
            if t.alt.is_none() && t.fk.out.len() > FAST_REPLAY {
                let k = t.fk.out.len() - FAST_REPLAY;
                t.fk.out.drain(..k);
            }
        }
        let lifted = resid
            .iter()
            .zip(&self.floor)
            .filter(|(v, &f)| v.re * v.re + v.im * v.im >= FLOOR_OUTLIER * f)
            .count() as f64
            > FLOOR_LIFT * resid.len() as f64
            && !crashed_now
            && self.band > 0.0
            && self.floor.iter().sum::<f64>() / self.floor.len() as f64 > CRASH_FLOOR_MIN;
        for (k, v) in resid.iter().enumerate() {
            let p = v.re * v.re + v.im * v.im + 1e-30;
            let lp = p.ln();
            if self.nfloor == 0 {
                self.logm[k] = lp;
            } else if p < FLOOR_OUTLIER * self.floor[k] || self.nfloor < FLOOR_WARMUP || lifted {
                let a = if self.nfloor < FLOOR_WARMUP {
                    1.0 / (self.nfloor + 1) as f64
                } else if lifted {
                    FLOOR_RATE_LIFT
                } else {
                    FLOOR_RATE
                };
                self.logm[k] += a * (lp - self.logm[k]);
            }
            self.floor[k] = (self.logm[k] + GAMMA).exp();
        }
        self.nfloor += 1;
        let dt = self.frame_seconds();
        let nb = self.snapshot(dt);
        for i in 0..self.tracks.len() {
            let ctx = Ctx {
                cfg: &self.cfg,
                rate: self.fe.rate as f64,
                m: self.fe.m as f64,
                floor: &self.floor,
                lo: self.fe.lo,
                knorm: self.fe.kernel.norm,
                crashes: &self.crashes,
                band: self.band,
                noisy: self.frame_no - self.noisy_fr < NOISY_FRAMES,
            };
            let (a, rest) = self.tracks.split_at_mut(i);
            let (t, b) = rest.split_first_mut().unwrap();
            let c = t.c;
            advance(
                &ctx,
                t,
                (a, b),
                c,
                dt,
                self.frame_no,
                true,
                &nb,
                &mut self.out,
                &mut self.elements,
            );
        }
        self.specs.push_back(spec.to_vec());
        if self.specs.len() > JOINT_FRAMES {
            self.specs.pop_front();
        }
        self.ring.push_back((resid.clone(), nb));
        if self.ring.len() > self.cfg.lookback {
            self.ring.pop_front();
        }
        if self.nfloor > BIRTH_WARMUP && !crashed {
            self.births(&resid, dt);
        }
        self.prune(dt);
    }

    fn births(&mut self, resid: &[C64], dt: f64) {
        let nb = resid.len();
        let mut snr = vec![0.0; nb];
        for (k, v) in resid.iter().enumerate() {
            let p = v.re * v.re + v.im * v.im;
            self.smooth[k] = SNR_KEEP * self.smooth[k] + SNR_NEW * p / self.floor[k];
            snr[k] = self.smooth[k];
        }
        let th = 10f64.powf(self.cfg.birth_db / 10.0);
        let k0 = self.fe.kernel.at(0.0).abs();
        let k0 = k0 * k0;
        // Within EDGE_GATE_HZ of a track, a candidate below the steady skirt of its current
        // power starts again (`mask`), and one below its usual skirt or its keying-edge
        // leakage is frozen (`emask`): those can be the track's own model error, while a
        // weak neighbour shows itself in the frames where the track is silent.
        let steady = 10f64.powf(-STEADY_ATT_DB / 10.0);
        let mut mask = vec![0.0f64; nb];
        let mut emask = vec![0.0f64; nb];
        for i in 0..self.tracks.len() {
            let nl = self.line_noise(self.tracks[i].nu);
            let t = &mut self.tracks[i];
            let e = t.c.re * t.c.re + t.c.im * t.c.im;
            t.edge_m = SNR_KEEP * t.edge_m + SNR_NEW * edge_fraction(e, t.prev_c, t.level, nl);
            t.e_sm = SNR_KEEP * t.e_sm + SNR_NEW * e;
        }
        for t in &self.tracks {
            let pk = t.peak_e.max(t.c.re * t.c.re + t.c.im * t.c.im) * k0;
            let cur = t.e_sm * k0;
            let source = t.confirmed || t.level > SOURCE_SNR * self.line_noise(t.nu);
            let lk = if source { t.level * k0 * t.edge_m } else { 0.0 };
            let unit = if t.key_unit > 0.0 {
                t.key_unit
            } else if self.frame_no - t.born < KEY_YOUNG {
                PARIS / WPM_HI
            } else {
                0.0
            };
            let ks = pk * KEY_SPECTRUM * unit / self.cfg.win_sec;
            for k in 0..nb {
                let df = self.hz((k + self.fe.lo) as f64 - t.nu).abs();
                let mut att = MASK_ATT_DB + MASK_SLOPE_DB_HZ * (df - MASK_FLAT_HZ).max(0.0);
                if att > MASK_MAX_DB {
                    att = MASK_MAX_DB;
                }
                let (m, me) = if df <= EDGE_GATE_HZ {
                    let skirt = (-att * DB_LN).exp();
                    let hard = cur * steady;
                    let x = df * unit;
                    let side = if x < 1.0 && x > 0.0 {
                        ks * ((PI * x).sin() / (PI * x)).powi(2)
                    } else {
                        ks * f64::from(x == 0.0)
                    };
                    let leak = if lk > 0.0 { lk * edge_leak(df) } else { 0.0 };
                    (hard, hard.max(cur * skirt).max(side) + leak)
                } else {
                    let m = pk * (-att.max(STEADY_ATT_DB) * DB_LN).exp();
                    (m, m)
                };
                mask[k] = mask[k].max(m);
                emask[k] = emask[k].max(me);
            }
        }
        let mut gmax = 0.0;
        for k in 0..nb {
            let v = snr[k] * self.floor[k];
            if v > gmax {
                gmax = v;
            }
        }
        // local order statistic across frequency (CFAR)
        let mut loc = vec![0.0; nb];
        let ww = self.fe.hz_bin(CFAR_HZ) as i64;
        let gg = self.fe.hz_bin(CFAR_GUARD_HZ) as i64;
        let mut buf: Vec<f64> = Vec::with_capacity(2 * ww as usize + 2);
        for k in 0..nb as i64 {
            buf.clear();
            for j in k - ww..=k + ww {
                if j < 0 || j >= nb as i64 || (j >= k - gg && j <= k + gg) {
                    continue;
                }
                buf.push(self.smooth[j as usize] * self.floor[j as usize]);
            }
            let mid = buf.len() / 2;
            loc[k as usize] = *buf.select_nth_unstable_by(mid, f64::total_cmp).1 / EXP_MEDIAN;
        }
        let mut esnr = vec![0.0; nb];
        for k in 0..nb {
            let eff = self.floor[k]
                .max(BIRTH_RANGE * gmax.max(self.gmax))
                .max(loc[k]);
            let v = self.smooth[k] * self.floor[k];
            snr[k] = v / eff.max(mask[k]);
            esnr[k] = v / eff.max(emask[k]);
        }
        if gmax > self.gmax {
            self.gmax = gmax;
        } else {
            self.gmax *= GMAX_DECAY;
        }
        let mut seen: Vec<i64> = Vec::new();
        let mut rs: Vec<(f64, f64)> = Vec::new();
        for k in 2..nb.saturating_sub(2) {
            if snr[k] < th
                || snr[k] < snr[k - 1]
                || snr[k] < snr[k + 1]
                || snr[k] < snr[k - 2]
                || snr[k] < snr[k + 2]
            {
                continue;
            }
            let mut key = k as i64;
            for kk in [k as i64 - 1, k as i64 + 1] {
                if self.cand.contains_key(&kk) {
                    key = kk;
                }
            }
            seen.push(key);
            let fno = self.frame_no;
            let cnt = self.cand.entry(key).or_insert((0, fno));
            if esnr[k] < th {
                // frozen, but not for longer than the passage of an edge
                if fno - cnt.1 > FREEZE_FRAMES {
                    *cnt = (0, fno);
                }
                continue;
            }
            *cnt = (cnt.0 + 1, fno);
            if cnt.0 < self.cfg.birth_run {
                continue;
            }
            // position from the residual power: the masks fall with the distance from a
            // strong track and would push the peak away from it
            let pw = |j: usize| self.smooth[j] * self.floor[j];
            let mut k = k;
            for j in [k - 2, k - 1, k + 1, k + 2] {
                if j >= 1 && j + 1 < nb && pw(j) > pw(k) {
                    k = j;
                }
            }
            let (l, c, r) = (pw(k - 1), pw(k), pw(k + 1));
            let mut nu = (k + self.fe.lo) as f64;
            let den = l - 2.0 * c + r;
            if den != 0.0 {
                nu += 0.5 * (l - r) / den;
            }
            rs.push((nu, snr[k]));
        }
        rs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut born: Vec<f64> = Vec::new();
        for &(nu, _) in &rs {
            if self.near(nu) {
                continue;
            }
            let nu = self.joint_position(nu);
            let mut ok = !self.near(nu);
            for &b in &born {
                if self.hz(b - nu).abs() < BIRTH_SPACING_HZ {
                    ok = false;
                }
            }
            if !ok {
                continue;
            }
            born.push(nu);
            let k = nu.round() as i64 - self.fe.lo as i64;
            self.cand.remove(&k);
            self.cand.remove(&(k - 1));
            self.cand.remove(&(k + 1));
            // candidates counted beside the new line before it was fitted start again
            let clear = self.fe.hz_bin(EDGE_GATE_HZ);
            let lo = self.fe.lo as f64;
            self.cand
                .retain(|&c, _| ((c as f64 + lo) - nu).abs() > clear);
            self.spawn(nu, dt);
        }
        self.cand.retain(|k, _| seen.contains(k));
    }

    /// Noise power in a line coefficient at frequency nu (bins), from the residual floor.
    fn line_noise(&self, nu: f64) -> f64 {
        let k = (nu.round() as i64 - self.fe.lo as i64).clamp(0, self.floor.len() as i64 - 1);
        self.floor[k as usize] / self.fe.kernel.norm
    }

    /// Position (bins) near nu of a new line that best explains the last spectra together
    /// with the line and ramp of every track within reach: the residual peak of a weak
    /// line beside a strong one is pushed away from the strong line.
    fn joint_position(&self, nu: f64) -> f64 {
        let k = &self.fe.kernel;
        let lo = self.fe.lo as f64;
        let near: Vec<f64> = self
            .tracks
            .iter()
            .filter(|t| (t.nu - nu).abs() < 2.0 * k.r)
            .map(|t| t.nu)
            .collect();
        if near.is_empty() || self.specs.is_empty() {
            return nu;
        }
        let mut best = (f64::INFINITY, nu);
        for j in 0..=JOINT_STEPS {
            let x = nu + JOINT_SPAN * (2.0 * j as f64 / JOINT_STEPS as f64 - 1.0);
            let mut lines: Vec<(f64, bool)> =
                near.iter().flat_map(|&v| [(v, false), (v, true)]).collect();
            lines.push((x, false));
            let n = lines.len();
            let a = near.iter().fold(x, |m, &v| m.min(v));
            let b = near.iter().fold(x, |m, &v| m.max(v));
            let b0 = (a - k.r - lo).floor().max(0.0) as usize;
            let b1 = (b + k.r - lo).ceil().min((self.floor.len() - 1) as f64) as usize;
            let w = b1 - b0 + 1;
            let mut cols = vec![C64::ZERO; n * w];
            for (i, &(v, ramp)) in lines.iter().enumerate() {
                for q in 0..w {
                    let d = (b0 + q) as f64 + lo - v;
                    cols[i * w + q] = if ramp { k.ramp(d) } else { k.at(d) };
                }
            }
            let mut g0 = vec![C64::ZERO; n * n];
            let mut reg = 0.0;
            for i in 0..n {
                for jj in 0..n {
                    let mut acc = C64::ZERO;
                    for q in 0..w {
                        acc += cols[i * w + q].conj() * cols[jj * w + q];
                    }
                    g0[i * n + jj] = acc;
                }
                reg += g0[i * n + i].re;
            }
            reg *= FIT_REG / n as f64;
            for i in 0..n {
                g0[i * n + i] += C64::new(reg, 0.0);
            }
            let mut cost = 0.0;
            for sp in &self.specs {
                let mut h = vec![C64::ZERO; n];
                for (i, hi) in h.iter_mut().enumerate() {
                    for q in 0..w {
                        *hi += cols[i * w + q].conj() * sp[b0 + q];
                    }
                }
                let c = solve(&mut g0.clone(), &mut h, n);
                for q in 0..w {
                    let mut r = sp[b0 + q];
                    for (i, &ci) in c.iter().enumerate() {
                        r -= ci * cols[i * w + q];
                    }
                    cost += r.re * r.re + r.im * r.im;
                }
            }
            if cost < best.0 {
                best = (cost, x);
            }
        }
        best.1
    }

    fn near(&self, nu: f64) -> bool {
        self.tracks
            .iter()
            .any(|t| self.hz(t.nu - nu).abs() < self.cfg.min_sep_hz)
    }

    /// Creates a track and replays the look-back buffer through it.
    fn spawn(&mut self, nu: f64, dt: f64) {
        let mut t = Track {
            id: self.next_id,
            nu,
            c: C64::ZERO,
            prev_c: C64::ZERO,
            key: Keyer::new(dt),
            tim: Timing::new(),
            confirmed: false,
            born: self.frame_no,
            level: 0.0,
            edges: VecDeque::new(),
            peak_e: 0.0,
            edge_m: 0.0,
            e_sm: 0.0,
            quiet: 0,
            quiet_on: 0,
            pump: [0; 4],
            quad: 0.0,
            e_held: 0.0,
            leak_bits: 0,
            soft_mark: self.frame_no,
            durs: VecDeque::new(),
            key_unit: 0.0,
            fk: FastKeyer::new(self.fe.rate as f64),
            alt: None,
            fast: false,
            dormant: false,
            dormant_n: 0,
            crowded_birth: false,
            sent: Vec::new(),
            sk: None,
            nst: NnState::default(),
        };
        self.next_id += 1;
        let (a, b) = self.raw.as_slices();
        for x in [a, b] {
            t.fk.feed(x, self.fe.bin_hz(nu), self.fe.rate as f64);
        }
        let k = &self.fe.kernel;
        let lo = self.fe.lo as f64;
        let mut norm = 0.0;
        let b0 = (nu - k.r - lo).floor().max(0.0) as usize;
        let b1 = (nu + k.r - lo).ceil().min((self.floor.len() - 1) as f64) as usize;
        let cols: Vec<C64> = (b0..=b1).map(|kk| k.at(kk as f64 + lo - nu)).collect();
        for cv in &cols {
            norm += cv.re * cv.re + cv.im * cv.im;
        }
        if norm <= 0.0 {
            // no analysis bin within the kernel's reach (a candidate beyond the band edge)
            return;
        }
        let mut cs = Vec::with_capacity(self.ring.len());
        let mut es = Vec::with_capacity(self.ring.len());
        for (f, _) in &self.ring {
            let mut s = C64::ZERO;
            for kk in b0..=b1 {
                s += cols[kk - b0].conj() * f[kk];
            }
            let c = s / C64::new(norm, 0.0);
            cs.push(c);
            es.push(c.re * c.re + c.im * c.im);
        }
        let mut sorted = es.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = sorted.len();
        let q = sorted[n / 3];
        t.key.peak = sorted[n - 1];
        t.key.noise = (q / P33_LOW).max(t.key.peak * NOISE_MIN_REL);
        if q >= sorted[n - 1] / HIGH_DUTY {
            let nz = (q / P33).min(sorted[n / 10] / P10);
            t.key.noise = nz.max(t.key.peak * NOISE_MIN_REL);
        }
        t.key.peak = t.key.peak.max(t.key.noise * PEAK_MIN_REL);
        let first = self.frame_no - cs.len() as i64 + 1;
        t.tim.c.at_start = first <= 1;
        let level = t.key.peak;
        t.crowded_birth = self.tracks.iter().any(|o| {
            self.fe.bin_hz(o.nu - nu).abs() <= OUTSHONE_HZ
                && o.key.peak > level
                && o.key.peak < CROWD_RATIO * level
        });
        let ctx = Ctx {
            cfg: &self.cfg,
            rate: self.fe.rate as f64,
            m: self.fe.m as f64,
            floor: &self.floor,
            lo: self.fe.lo,
            knorm: self.fe.kernel.norm,
            crashes: &self.crashes,
            band: self.band,
            noisy: self.frame_no - self.noisy_fr < NOISY_FRAMES,
        };
        for (i, &c) in cs.iter().enumerate() {
            t.prev_c = t.c;
            t.c = c;
            let nb = &self.ring[i].1;
            advance(
                &ctx,
                &mut t,
                (&self.tracks, &[]),
                c,
                dt,
                first + i as i64,
                false,
                nb,
                &mut self.out,
                &mut self.elements,
            );
        }
        self.tracks.push(t);
    }

    fn prune(&mut self, dt: f64) {
        let (rate, m) = (self.fe.rate as f64, self.fe.m as f64);
        let tracks = std::mem::take(&mut self.tracks);
        for mut t in tracks {
            let silent = t.key.last_on as f64 * dt;
            let mut dead = silent > self.cfg.dead_sec;
            // a candidate is silent when neither keyer has seen a mark
            let soft_silent = (self.frame_no - t.soft_mark) as f64 * dt;
            if !t.confirmed && silent > CANDIDATE_DEAD_SEC && soft_silent > CANDIDATE_DEAD_SEC {
                dead = true;
            }
            if dead {
                let ev = t.sk.as_ref().map_or(0.0, |k| k.evidence(self.cfg.ev_win));
                if t.confirmed && !t.dormant && ev >= FLUSH_EV {
                    t.tim.tick(t.gap(), true, true);
                    t.tim.finish();
                    let f = t.nu * rate / m;
                    deliver(&mut self.out, &mut t, f);
                }
                continue;
            }
            self.tracks.push(t);
        }
        self.tracks
            .sort_by(|a, b| a.nu.partial_cmp(&b.nu).unwrap_or(std::cmp::Ordering::Equal));
        for i in 1..self.tracks.len() {
            let (a, b) = (&self.tracks[i - 1], &self.tracks[i]);
            if self.hz(b.nu - a.nu).abs() < self.cfg.min_sep_hz {
                let drop = if b.born < a.born { i - 1 } else { i };
                self.tracks[drop].key.last_on = KILL;
                self.tracks[drop].confirmed = false;
            }
        }
    }
}

/// Joint fit of a cluster with each ramp split into an in-phase part (an amplitude ramp,
/// free) and a quadrature part (a frequency error, held near the track's slow average by
/// a ridge): a frequency error is steady, while a close neighbour's beat rotates. The
/// phase reference is this frame's line coefficient from the unconstrained fit. `g` and
/// `h` are the complex normal equations over the columns line, ramp per track (already
/// regularised) and `quads` the tracks' quadrature averages; returns the complex
/// coefficients in the order of the columns.
fn solve_ramp(g: &[C64], h: &[C64], quads: &[f64]) -> Vec<C64> {
    let n = 2 * quads.len();
    let r = 2 * n;
    let cur = solve(&mut g.to_vec(), &mut h.to_vec(), n);
    // real unknowns per track: line re, line im, ramp in-phase, ramp quadrature
    let mut ph = vec![C64::ZERO; r];
    for a in 0..quads.len() {
        let m = cur[2 * a].abs();
        let u = if m > 0.0 {
            cur[2 * a] / C64::new(m, 0.0)
        } else {
            C64::new(1.0, 0.0)
        };
        ph[4 * a] = C64::new(1.0, 0.0);
        ph[4 * a + 1] = C64::new(0.0, 1.0);
        ph[4 * a + 2] = u;
        ph[4 * a + 3] = u * C64::new(0.0, 1.0);
    }
    let base = |i: usize| 2 * (i / 4) + usize::from(i % 4 >= 2);
    let mut gr = vec![0.0; r * r];
    let mut hr = vec![0.0; r];
    for i in 0..r {
        for j in 0..r {
            gr[i * r + j] = (ph[i].conj() * ph[j] * g[base(i) * n + base(j)]).re;
        }
        hr[i] = (ph[i].conj() * h[base(i)]).re;
    }
    for (a, &quad) in quads.iter().enumerate() {
        let i = 4 * a + 3;
        let lam = QUAD_RIDGE * gr[i * r + i];
        gr[i * r + i] += lam;
        hr[i] += lam * quad * cur[2 * a].abs();
    }
    let x = solve_real(&mut gr, &mut hr, r);
    let mut c = vec![C64::ZERO; n];
    for a in 0..quads.len() {
        c[2 * a] = C64::new(x[4 * a], x[4 * a + 1]);
        c[2 * a + 1] = ph[4 * a + 2] * C64::new(x[4 * a + 2], x[4 * a + 3]);
    }
    c
}

/// Gaussian elimination with partial pivoting for a small real system.
fn solve_real(a: &mut [f64], b: &mut [f64], n: usize) -> Vec<f64> {
    for col in 0..n {
        let mut p = col;
        for r in col + 1..n {
            if a[r * n + col].abs() > a[p * n + col].abs() {
                p = r;
            }
        }
        if p != col {
            for k in 0..n {
                a.swap(col * n + k, p * n + k);
            }
            b.swap(col, p);
        }
        let piv = a[col * n + col];
        if piv == 0.0 {
            continue;
        }
        for r in col + 1..n {
            let f = a[r * n + col] / piv;
            for k in col..n {
                a[r * n + k] -= f * a[col * n + k];
            }
            b[r] -= f * b[col];
        }
    }
    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        let mut s = b[r];
        for k in r + 1..n {
            s -= a[r * n + k] * x[k];
        }
        if a[r * n + r] != 0.0 {
            x[r] = s / a[r * n + r];
        }
    }
    x
}

/// Gaussian elimination with partial pivoting for a small complex system.
fn solve(a: &mut [C64], b: &mut [C64], n: usize) -> Vec<C64> {
    for col in 0..n {
        let mut p = col;
        for r in col + 1..n {
            if a[r * n + col].abs() > a[p * n + col].abs() {
                p = r;
            }
        }
        if p != col {
            for k in 0..n {
                a.swap(col * n + k, p * n + k);
            }
            b.swap(col, p);
        }
        let piv = a[col * n + col];
        if piv.is_zero() {
            continue;
        }
        for r in col + 1..n {
            let f = a[r * n + col] / piv;
            for k in col..n {
                let v = f * a[col * n + k];
                a[r * n + k] -= v;
            }
            let v = f * b[col];
            b[r] -= v;
        }
    }
    let mut x = vec![C64::ZERO; n];
    for r in (0..n).rev() {
        let mut s = b[r];
        for k in r + 1..n {
            s -= a[r * n + k] * x[k];
        }
        if !a[r * n + r].is_zero() {
            x[r] = s / a[r * n + r];
        }
    }
    x
}

fn deliver(out: &mut Vec<Out>, t: &mut Track, freq: f64) {
    if t.tim.out().is_empty() {
        return;
    }
    let (text, meta) = t.tim.take_out();
    t.sent.extend(text.chars().filter(|&c| c != ' '));
    if t.sent.len() > SENT_CHARS {
        t.sent.drain(..t.sent.len() - SENT_CHARS);
    }
    out.push(Out {
        track: t.id,
        freq,
        text,
        meta,
    });
}

type Others<'a> = (&'a [Track], &'a [Track]);

fn others_iter<'a>(o: Others<'a>) -> impl Iterator<Item = &'a Track> {
    o.0.iter().chain(o.1.iter())
}

/// Feeds one frame of a track's amplitude through keyer and timing.
/// `live` is false while replaying the look-back buffer at birth.
#[allow(clippy::too_many_arguments)]
fn advance(
    ctx: &Ctx,
    t: &mut Track,
    others: Others,
    c: C64,
    dt: f64,
    fr: i64,
    live: bool,
    nb: &[NbRec],
    out: &mut Vec<Out>,
    elements: &mut Option<Vec<(u32, i64, Element)>>,
) {
    let cfg = ctx.cfg;
    let e = c.re * c.re + c.im * c.im;
    let leak = edge_leak_in(ctx, t, nb);
    let eh = held_power(t, e, leak);
    let feats = features(ctx, t, c, eh, dt, nb);
    // phase-advance frequency refinement on strong frames
    if live
        && t.key.on
        && !t.prev_c.is_zero()
        && e > REFINE_SNR * t.key.noise
        && !dominated(ctx, t, others, e)
    {
        let want = 2.0 * std::f64::consts::PI * ctx.hz(t.nu) * dt;
        let dphi = remainder(
            (c * t.prev_c.conj()).arg() - want,
            2.0 * std::f64::consts::PI,
        );
        t.nu += REFINE_GAIN * ctx.hz_bin(dphi / (2.0 * std::f64::consts::PI * dt));
    }
    let mut unit = t.tim.best().u;
    if t.tim.ambig() <= AMBIG_LOW {
        unit = 0.0;
    }
    let was = t.key.on;
    t.key.step(e, unit, |_| {});
    let sk =
        t.sk.get_or_insert_with(|| SoftKeyer::new(dt, cfg.kappa, cfg.k_frac));
    let best: Hyp = t.tim.best();
    sk.build(best, t.tim.ambig() <= AMBIG_LOW);
    let s = (t.key.peak / t.key.noise - 1.0).max(SOFT_SNR_MIN);
    let x = e / t.key.noise;
    // key-down log-likelihood ratio from the network, blended from the Rician model
    // over the first frames of a track (the network has no history of a new station)
    let lg = nn::step(&mut t.nst, &feats);
    if live || !t.crowded_birth {
        t.nst.age += 1;
    }
    let mut llr = (NN_TAU * (lg - nn::prior_logit())).clamp(-NN_CLIP, NN_CLIP);
    if t.nst.age < NN_BLEND {
        let w = t.nst.age as f64 / NN_BLEND as f64;
        llr = w * llr + (1.0 - w) * sk.rician_llr(x, s);
    }
    // frames under a louder neighbour's keying edges carry little evidence either way
    llr /= 1.0 + leak / t.level.max(t.key.noise);
    if e < BAND_GATE * ctx.band {
        llr = llr.min(0.0);
    }
    t.tim.c.snr_db = 10.0 * (t.key.peak / t.key.noise).max(1e-3).log10();
    t.tim.c.fade_db = 10.0 * (t.level / t.peak_e.max(1e-30)).clamp(1e-3, 1.0).log10();
    // a weak station next to a clearly stronger one holds doubtful characters back
    let lvl = t.level * 10f64.powf(OUTSHONE_DB / 10.0);
    t.tim.c.outshone = others_iter(others)
        .any(|o| o.confirmed && ctx.hz(o.nu - t.nu).abs() <= OUTSHONE_HZ && o.level > lvl);
    let glvl = t.level * 10f64.powf(GUARD_DB / 10.0);
    t.tim.c.guarded = others_iter(others)
        .any(|o| o.confirmed && ctx.hz(o.nu - t.nu).abs() <= GUARD_HZ && o.level > glvl);
    common_fate(ctx, t, nb, llr);
    pump_fate(t, nb, llr);
    t.leak_bits = (t.leak_bits << 1) | u128::from(leak > UNSURE_LEAK * t.level.max(t.key.noise));
    let bits = t.leak_bits;
    let soft_mark = &mut t.soft_mark;
    let sk = t.sk.as_mut().unwrap();
    let (tim, ftim) = match (t.fast, t.alt.as_mut()) {
        (true, Some(a)) => (a, Some(&mut t.tim)),
        (_, a) => (&mut t.tim, a),
    };
    if let Some(f) = ftim {
        for el in t.fk.out.drain(..) {
            f.add_element(el);
        }
    }
    let id = t.id;
    let durs = &mut t.durs;
    sk.step(llr, |el| {
        durs.push_back(el.d);
        if durs.len() > KEY_ELEMENTS {
            durs.pop_front();
        }
        if let Some(log) = elements.as_mut() {
            log.push((id, fr, el));
        }
        if el.mark {
            *soft_mark = fr;
        }
        tim.c.next_unsure = boundary_unsure(bits, el, dt);
        tim.add_element(el);
    });
    t.key_unit = keying_unit(&t.durs);
    let u = t.fk.unit();
    let gain = t.alt.as_ref().map_or(0.0, |a| a.c.llr - t.tim.c.llr);
    let clear = !others_iter(others)
        .any(|o| ctx.hz(o.nu - t.nu).abs() < FAST_CLEAR_HZ && o.level > t.level);
    let to_fast = !t.fast
        && clear
        && u > 0.0
        && u < FAST_ON
        && gain > FAST_LLR
        && t.alt.as_ref().is_some_and(|a| {
            a.nchars() >= cfg.conf_chars && a.best().u < FAST_ON && a.best().u > FAST_MIN
        });
    let to_slow = t.fast && (u > FAST_OFF || gain > FAST_LLR);
    if to_fast || to_slow {
        std::mem::swap(&mut t.tim, t.alt.as_mut().unwrap());
        t.fast = !t.fast;
        if t.confirmed {
            t.tim.align(&t.sent);
        }
    }
    if t.key.on != was {
        t.edges.push_back(Edge {
            fr: fr - t.key.min_run,
            on: t.key.on,
        });
        if t.edges.len() > EDGE_MEMORY {
            t.edges.pop_front();
        }
    }
    if t.key.on {
        t.level = LEVEL_KEEP * t.level + LEVEL_NEW * e;
    }
    t.peak_e = e.max(t.peak_e * PEAK_DECAY);
    let sk = t.sk.as_ref().unwrap();
    let gap_now = if t.fast { t.fk.gap() } else { sk.gap() };
    let ev = sk.evidence(cfg.ev_win);
    if live && t.confirmed && !t.dormant && t.tim.c.ended {
        t.dormant = true;
        t.dormant_n = t.tim.nchars();
    }
    if ev < cfg.ev_alive {
        if !t.confirmed || t.dormant {
            t.tim.discard_held();
            t.dormant_n = 0;
        }
        return;
    }
    let ev_conf = if ctx.noisy {
        cfg.ev_conf.max(EV_CONF_NOISY)
    } else {
        cfg.ev_conf
    };
    if live && t.dormant {
        if ev >= ev_conf && t.tim.nchars() - t.dormant_n >= cfg.conf_chars && !crash_driven(ctx, t)
        {
            t.dormant = false;
            t.tim.release();
        } else {
            t.tim.tick(gap_now, false, false);
            return;
        }
    }
    if live && !t.confirmed && ev < ev_conf {
        t.tim.tick(gap_now, false, false);
        t.tim.open_gap(gap_now, false);
        return;
    }
    if live && !t.confirmed && t.tim.nchars() >= cfg.conf_chars {
        if ghost_of(ctx, t, others) || crash_driven(ctx, t) || pumped(t) {
            t.key.last_on = KILL; // its keying is a stronger neighbour's: a skirt, not a station
            return;
        }
        let f = confirm::features(
            ev,
            t.tim.c.snr_db,
            t.tim.c.fade_db,
            t.tim.c.r,
            t.tim.c.b,
            t.tim.c.scale,
            t.tim.c.llr,
            t.tim.nchars(),
            (fr - t.born) as f64 * dt,
            crate::model::PARIS / t.tim.best().u,
            ctx.noisy,
            t.tim.c.outshone,
        );
        let logged = CONF_LOG.with(|l| {
            l.borrow_mut()
                .as_mut()
                .map(|v| v.push((t.id, fr, ctx.hz(t.nu), f.to_vec())))
                .is_some()
        });
        if !logged && confirm::prob(&f) < confirm::P_CONFIRM {
            t.tim.tick(gap_now, false, false);
            t.tim.open_gap(gap_now, false);
            return;
        }
        t.confirmed = true;
        t.tim.release();
    }
    t.tim.tick(gap_now, t.confirmed, false);
    t.tim.open_gap(gap_now, t.confirmed);
    if live {
        let f = ctx.hz(t.nu);
        deliver(out, t, f);
    }
}

/// Whether most of the track's onsets coincide with edges of a clearly stronger nearby
/// track, clearly more often than the density of those edges explains by chance, and the
/// track was never keyed on its own while its louder neighbours were silent.
fn ghost_of(ctx: &Ctx, t: &Track, others: Others) -> bool {
    let ons: Vec<i64> = t.edges.iter().filter(|e| e.on).map(|e| e.fr).collect();
    if ons.len() < 3 {
        return false;
    }
    if t.quiet_on >= FATE_ON && t.quiet_on as f64 >= FATE_SHARE * t.quiet as f64 {
        return false;
    }
    for o in others_iter(others) {
        let far = ctx.hz(o.nu - t.nu).abs() > GHOST_HZ;
        if o.level < GHOST_LEVEL * t.level || (far && o.level < GHOST_FAR_LEVEL * t.level) {
            continue;
        }
        let mut hit = 0;
        for &f in &ons {
            if o.edges
                .iter()
                .any(|e| f - e.fr <= GHOST_FRAMES && e.fr - f <= GHOST_FRAMES)
            {
                hit += 1;
            }
        }
        if (hit as f64) < GHOST_FRAC * ons.len() as f64 {
            continue;
        }
        // share of the frames spanned by the onsets that lie near an edge of the
        // neighbour: random onsets hit that often by chance
        let (a, b) = (ons[0] - GHOST_FRAMES, ons[ons.len() - 1] + GHOST_FRAMES);
        let mut covered = 0;
        let mut until = a;
        for e in o.edges.iter() {
            let (lo, hi) = (
                (e.fr - GHOST_FRAMES).max(until),
                (e.fr + GHOST_FRAMES + 1).min(b),
            );
            if hi > lo {
                covered += hi - lo;
                until = hi;
            }
        }
        let p0 = covered as f64 / (b - a).max(1) as f64;
        let miss = (ons.len() - hit) as f64;
        if miss <= GHOST_MISS * (1.0 - p0) * ons.len() as f64
            && binomial_tail(ons.len(), hit, p0) < GHOST_CHANCE
        {
            return true;
        }
    }
    false
}

/// Probability of at least k successes in n trials of probability p.
fn binomial_tail(n: usize, k: usize, p: f64) -> f64 {
    let p = p.clamp(1e-9, 1.0 - 1e-9);
    let mut c = 1.0;
    let mut sum = 0.0;
    for i in 0..=n {
        if i > 0 {
            c *= (n - i + 1) as f64 / i as f64;
        }
        if i >= k {
            sum += c * p.powi(i as i32) * (1.0 - p).powi((n - i) as i32);
        }
    }
    sum
}

/// Whether most of the track's onsets follow static crashes.
fn crash_driven(ctx: &Ctx, t: &Track) -> bool {
    let ons: Vec<i64> = t.edges.iter().filter(|e| e.on).map(|e| e.fr).collect();
    if ons.is_empty() {
        return false;
    }
    let hit = ons
        .iter()
        .filter(|&&f| {
            ctx.crashes
                .iter()
                .any(|&c| f - c >= -GHOST_FRAMES && f - c <= CRASH_HOLD)
        })
        .count();
    hit as f64 >= GHOST_FRAC * ons.len() as f64
}

/// Whether a stronger track within 80 Hz is louder in this frame.
fn dominated(ctx: &Ctx, t: &Track, others: Others, e: f64) -> bool {
    others_iter(others)
        .any(|o| ctx.hz(o.nu - t.nu).abs() <= DOMINANCE_HZ && o.c.re * o.c.re + o.c.im * o.c.im > e)
}

/// Counts the frames in which every clearly louder confirmed neighbour is silent with no
/// edge in the window, and those in which this track is key-down: a skirt or leakage
/// ghost follows its neighbour's keying and is never key-down while the neighbour is
/// silent.
fn common_fate(ctx: &Ctx, t: &mut Track, nb: &[NbRec], llr: f64) {
    let mut any = false;
    for o in nb {
        if o.id == t.id
            || !o.confirmed
            || o.level < GHOST_LEVEL * t.level
            || ctx.hz(o.nu - t.nu).abs() > GHOST_HZ
        {
            continue;
        }
        any = true;
        if o.edge > 0.0 || o.e > QUIET_FRAC * o.level {
            return;
        }
    }
    if any {
        t.quiet += 1;
        if llr > 0.0 {
            t.quiet_on += 1;
        }
    }
}

/// Counts this track's key-down frames while the loudest much stronger confirmed track is
/// key-down and while it is silent.
fn pump_fate(t: &mut Track, nb: &[NbRec], llr: f64) {
    let Some(o) = nb
        .iter()
        .filter(|o| o.id != t.id && o.confirmed && o.level > GHOST_FAR_LEVEL * t.level)
        .max_by(|a, b| a.level.total_cmp(&b.level))
    else {
        return;
    };
    let on = usize::from(o.e > PUMP_ON * o.level);
    if o.edge > 0.0 || (on == 0 && o.e > QUIET_FRAC * o.level) {
        return;
    }
    t.pump[2 * on] += 1;
    if llr > 0.0 {
        t.pump[2 * on + 1] += 1;
    }
}

/// Whether the track is key-down mostly while a much stronger station is silent, as noise
/// lifted by a receiver AGC between that station's elements.
fn pumped(t: &Track) -> bool {
    let [off, off_on, on, on_on] = t.pump.map(f64::from);
    off >= PUMP_FRAMES && on >= PUMP_FRAMES && on_on / on < PUMP_RATIO * off_on / off
}

/// Keying-edge leakage of louder tracks expected in the track's line in this frame (in
/// units of its power).
fn edge_leak_in(ctx: &Ctx, t: &Track, nb: &[NbRec]) -> f64 {
    let mut leak = 0.0;
    for o in nb {
        if o.id == t.id || o.edge <= 0.0 || o.level <= t.level || !o.source {
            continue;
        }
        leak += o.level * o.edge * edge_leak(ctx.hz(o.nu - t.nu).abs());
    }
    leak
}

/// Line power given to the key-state network: a frame dominated by a louder track's edge
/// leakage repeats the last clean frame, so that the burst does not enter its memory.
fn held_power(t: &mut Track, e: f64, leak: f64) -> f64 {
    if leak > HOLD_LEAK * t.level.max(t.key.noise) && t.e_held > 0.0 {
        return t.e_held.min(e);
    }
    t.e_held = e;
    e
}

/// Whether an element just decided by the soft keyer (with its fixed lag) starts or ends
/// within `UNSURE_FRAMES` of a frame dominated by a louder track's edge leakage, or is a
/// space holding such a frame (which may hide a mark the leakage erased).
fn boundary_unsure(bits: u128, el: Element, dt: f64) -> bool {
    if bits == 0 {
        return false;
    }
    let flagged = |j: i64| (0..128).contains(&j) && bits >> j & 1 == 1;
    let run = (el.d / dt).round() as i64;
    let near = [SOFT_LAG, SOFT_LAG + run]
        .iter()
        .any(|&b| (b - UNSURE_FRAMES..=b + UNSURE_FRAMES).any(flagged));
    near || (!el.mark && (SOFT_LAG..SOFT_LAG + run).any(flagged))
}

/// Worst-case residual of a keying edge of a fitted line at offset df (Hz), relative to
/// the line's peak bin power (40 ms Hann window, line and ramp subtracted; the larger of
/// 5 ms raised-cosine and 0.3 ms edges).
const EDGE_LEAK: [(f64, f64); 9] = [
    (0.0, -20.0),
    (25.0, -23.0),
    (37.5, -16.5),
    (50.0, -16.0),
    (62.5, -16.0),
    (80.0, -19.0),
    (100.0, -21.5),
    (125.0, -24.0),
    (150.0, -25.0),
];

/// Worst-case edge leakage at offset df (Hz) as a power ratio (none beyond the table).
fn edge_leak(df: f64) -> f64 {
    if df > EDGE_LEAK[EDGE_LEAK.len() - 1].0 {
        return 0.0;
    }
    let i = EDGE_LEAK.iter().position(|p| p.0 >= df).unwrap_or(1).max(1);
    let (a, b) = (EDGE_LEAK[i - 1], EDGE_LEAK[i]);
    let db = a.1 + (b.1 - a.1) * (df - a.0) / (b.0 - a.0);
    10f64.powf(db / 10.0)
}

/// Lower quartile (s) of the recent element durations.
fn keying_unit(durs: &VecDeque<f64>) -> f64 {
    if durs.len() < 4 {
        return 0.0;
    }
    let mut d: Vec<f64> = durs.iter().copied().collect();
    d.sort_by(f64::total_cmp);
    d[d.len() / 4]
}

/// Share (0 to 1) of the worst-case edge leakage of a track in one frame: an edge inside
/// the window leaves a partial share u of the key-down amplitude and moves the amplitude
/// from frame to frame; noise-level frames, full key-down frames and slow fading leave
/// none.
fn edge_fraction(e: f64, prev: C64, level: f64, noise: f64) -> f64 {
    if level <= 0.0 {
        return 0.0;
    }
    let u2 = ((e - EDGE_NOISE * noise).max(0.0) / level).min(1.0);
    let u = u2.sqrt();
    if u < EDGE_U_MIN {
        return 0.0;
    }
    let g = (u / EDGE_U_KNEE).powf(1.3).min(4.0 * (1.0 - u2)).min(1.0);
    let step = (e.sqrt() - prev.abs()).abs() / level.sqrt();
    g * (step / EDGE_STEP).min(1.0)
}

/// Cosine of the deviation of a frame's phase advance from the advance expected at the
/// track frequency (1 for a steady carrier, 0 when undefined).
fn phase_cons(c: C64, prev: C64, hz: f64, dt: f64) -> f64 {
    if prev.is_zero() || c.is_zero() {
        return 0.0;
    }
    let want = 2.0 * std::f64::consts::PI * hz * dt;
    remainder((c * prev.conj()).arg() - want, 2.0 * std::f64::consts::PI).cos()
}

#[inline]
fn clamp(x: f64, b: (f64, f64)) -> f64 {
    x.min(b.1).max(b.0)
}

impl Core {
    /// Records all tracks of the current frame (after the fit) for neighbour features.
    fn snapshot(&self, dt: f64) -> Vec<NbRec> {
        self.tracks
            .iter()
            .map(|t| {
                let e = t.c.re * t.c.re + t.c.im * t.c.im;
                let mut la = t.nst.la;
                if t.nst.init {
                    la = clamp(e.max(POWER_MIN).ln() - t.nst.nf, NN_LA);
                }
                let evidence = t.sk.as_ref().map_or(0.0, |k| k.evidence(self.cfg.ev_win));
                NbRec {
                    id: t.id,
                    nu: t.nu,
                    la,
                    pc: phase_cons(t.c, t.prev_c, self.hz(t.nu), dt),
                    level: t.level,
                    e,
                    edge: edge_fraction(e, t.prev_c, t.level, self.line_noise(t.nu)),
                    confirmed: t.confirmed,
                    source: t.confirmed
                        || (t.level > SOURCE_SNR * self.line_noise(t.nu)
                            && evidence >= self.cfg.ev_conf),
                }
            })
            .collect()
    }
}

/// Input features of the key-state network for track t in one frame.
/// Confirmation features of a candidate: track, frame, Hz, features.
pub type ConfRecord = (u32, i64, f64, Vec<f32>);

thread_local! {
    pub static CONF_LOG: std::cell::RefCell<Option<Vec<ConfRecord>>> = const { std::cell::RefCell::new(None) };
}

fn features(ctx: &Ctx, t: &mut Track, c: C64, e: f64, dt: f64, nb: &[NbRec]) -> [f32; INPUTS] {
    let st = &mut t.nst;
    let l = e.max(POWER_MIN).ln();
    if !st.init {
        // start from the birth estimate of the noise, which does not depend on the keyer
        st.nf = t.key.noise.max(POWER_MIN).ln();
        st.init = true;
        st.pk = l;
    }
    st.pk = l.max(st.pk - NN_PEAK_RELEASE);
    st.nf = st.nf.max(st.pk - NN_RANGE);
    let la = clamp(l - st.nf, NN_LA);
    if l > st.nf {
        st.nf += NN_UP;
    } else {
        st.nf -= NN_DOWN;
    }
    st.la = la;
    let floor = (ctx.noise_at(t.nu).max(POWER_MIN) / ctx.knorm).ln();
    let lb = clamp(l - floor.max(st.pk - NN_RANGE), NN_LB);
    let mut f = [0f32; INPUTS];
    f[0] = la as f32;
    f[1] = lb as f32;
    f[2] = phase_cons(c, t.prev_c, ctx.hz(t.nu), dt) as f32;
    f[3] = clamp(l - st.pk, NN_LP) as f32;
    f[4] = 0.0;
    f[5] = 1.0;
    f[6] = 0.0;
    // within the reach of keying-edge leakage the joint fit, the leakage model and the
    // common-fate checks deal with neighbours: there the network's neighbour inputs made
    // it follow a strong neighbour's keying
    let mut best = -1.0;
    for o in nb {
        if o.id == t.id {
            continue;
        }
        let df = ctx.hz(o.nu - t.nu).abs();
        if df > NN_NEIGHBOUR_HZ || df <= EDGE_GATE_HZ || o.level <= best {
            continue;
        }
        best = o.level;
        f[4] = o.la as f32;
        f[5] = (df / NN_NEIGHBOUR_HZ) as f32;
        f[6] = o.pc as f32;
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_leakage_table() {
        // strongest just outside the main lobe, falling beyond, none past 150 Hz
        assert!(edge_leak(50.0) > edge_leak(25.0));
        assert!(edge_leak(50.0) > edge_leak(100.0));
        assert!(edge_leak(100.0) > edge_leak(150.0));
        assert!((10.0 * edge_leak(50.0).log10() + 16.0).abs() < 1e-9);
        assert_eq!(edge_leak(151.0), 0.0);
    }

    #[test]
    fn edge_share_needs_a_partial_and_moving_amplitude() {
        let level = 100.0;
        let noise = 0.01;
        let amp = |a: f64| C64::new(a, 0.0);
        // steady key-down, silence and slow fading show no edge
        assert_eq!(edge_fraction(level, amp(10.0), level, noise), 0.0);
        assert_eq!(edge_fraction(0.02, amp(0.1), level, noise), 0.0);
        assert!(edge_fraction(50.0, amp(7.0), level, noise) < 0.1);
        // half the amplitude, moving by a third of the key-down amplitude per frame
        assert_eq!(edge_fraction(25.0, amp(8.3), level, noise), 1.0);
    }

    #[test]
    fn element_boundaries_under_edges_are_unsure() {
        let el = Element {
            mark: true,
            d: 0.05,
        };
        let dt = 0.005;
        assert!(!boundary_unsure(0, el, dt));
        // the element ended SOFT_LAG frames ago and started 10 frames before that
        assert!(boundary_unsure(1 << SOFT_LAG, el, dt));
        assert!(boundary_unsure(1 << (SOFT_LAG + 10 + 2), el, dt));
        assert!(!boundary_unsure(1 << (SOFT_LAG + 5), el, dt));
        // a space with such a frame inside
        let gap = Element {
            mark: false,
            d: 0.05,
        };
        assert!(boundary_unsure(1 << (SOFT_LAG + 5), gap, dt));
    }

    /// Residual power of a weak tone df Hz above a strong line after fitting the strong
    /// line's columns alone, unconstrained and with the quadrature ridge.
    fn absorbed(df: f64, ridge: bool) -> f64 {
        let fe = Frontend::new(8000, 0.040, 250.0, 1300.0);
        let k = &fe.kernel;
        let nu = fe.hz_bin(700.0);
        let nw = fe.hz_bin(700.0 + df);
        let (b0, b1) = ((nu - k.r).floor() as usize, (nu + k.r).ceil() as usize);
        let cols: Vec<[C64; 2]> = (b0..=b1)
            .map(|b| [k.at(b as f64 - nu), k.ramp(b as f64 - nu)])
            .collect();
        let spec: Vec<C64> = (b0..=b1)
            .map(|b| k.at(b as f64 - nw) * C64::cis(0.7))
            .collect();
        let mut g = vec![C64::ZERO; 4];
        let mut h = vec![C64::ZERO; 2];
        for (c, &y) in cols.iter().zip(&spec) {
            for a in 0..2 {
                for b in 0..2 {
                    g[a * 2 + b] += c[a].conj() * c[b];
                }
                h[a] += c[a].conj() * y;
            }
        }
        let reg = FIT_REG * (g[0].re + g[3].re) / 2.0;
        g[0] += C64::new(reg, 0.0);
        g[3] += C64::new(reg, 0.0);
        let x = if ridge {
            solve_ramp(&g, &h, &[0.0])
        } else {
            solve(&mut g.clone(), &mut h.clone(), 2)
        };
        let mut left = 0.0;
        let mut all = 0.0;
        for (c, &y) in cols.iter().zip(&spec) {
            let r = y - x[0] * c[0] - x[1] * c[1];
            left += r.re * r.re + r.im * r.im;
            all += y.re * y.re + y.im * y.im;
        }
        left / all
    }

    #[test]
    fn quadrature_ridge_leaves_a_close_neighbour_in_the_residual() {
        // a free complex ramp takes up most of a tone 25 Hz away (frequency shift)
        assert!(absorbed(25.0, false) < 0.25, "{}", absorbed(25.0, false));
        assert!(absorbed(25.0, true) > 0.3, "{}", absorbed(25.0, true));
        assert!(absorbed(50.0, true) > absorbed(50.0, false));
    }
}
