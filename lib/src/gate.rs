//! Station-presence gate: a small causal convolutional network reads the whole band and
//! scores, per 15.625 Hz strip and 16 ms frame, whether a station is centred there.
//! Committed text of a track is released only where this score confirms a station.
//!
//! Front end: resampling to 8 kHz, Hann 256 / hop 32 / FFT 512 log power of bins 10..90,
//! normalised by a per-bin floor (fast down, slow up) and by the smoothed band median.
//! Strips are bins 16..84, each seeing +-6 neighbours under both normalisations (26 inputs).
//! Network per strip (shared weights): causal convolutions 26-32 (k5), 32-32 (k3, stride 2),
//! 32-48 (k3, stride 2), a 1x1 layer, seven residual blocks (depthwise k3 with dilations
//! 1, 2, 4, 8, 16, 32, 1, channel RMS normalisation, 48-96-48 pointwise with GELU) and a
//! logistic centre output, smoothed over 150 ms.
//! Gate: the score of a commit is the largest smoothed centre value within +-2 strips of the
//! commit frequency over the commit's span. Commits of a new track are held until one of
//! them scores `OPEN` and then released together (a track that never does is dropped);
//! after that a commit following more than 2 s without commits is checked against `KEEP`,
//! and commits between checks follow the last check.

use crate::complex::C64;
use crate::fft::Fft;
use std::collections::{HashMap, VecDeque};

const WEIGHTS: &[u8] = include_bytes!("gate_weights.bin");
const FS: f64 = 8000.0;
const NWIN: usize = 256;
const HOP: usize = 32;
const NFFT: usize = 512;
const B0: usize = 10;
const NB: usize = 80;
const S0: usize = 16;
const NS: usize = 68;
const CTX: usize = 6;
const NIN: usize = 2 * (2 * CTX + 1);
const C1: usize = 32;
const C3: usize = 48;
const H: usize = 48;
const E: usize = 96;
const DIL: [usize; 7] = [1, 2, 4, 8, 16, 32, 1];
const INIT_FRAMES: usize = 25;
/// Peak-to-peak amplitude of the uniform dither relative to the input's slowly decaying
/// peak (rms about 55 dB below the peak).
const DITHER: f64 = 6e-3;
const OPEN: f32 = 0.6;
const KEEP: f32 = 0.5;
const HOLD_SEC: f64 = 2.0;

struct Net {
    lut: Vec<f32>,
    c1w: Vec<f32>,
    c1b: Vec<f32>,
    c2w: Vec<f32>,
    c2b: Vec<f32>,
    c3w: Vec<f32>,
    c3b: Vec<f32>,
    iw: Vec<f32>,
    ib: Vec<f32>,
    blocks: Vec<[Vec<f32>; 6]>,
    cw: Vec<f32>,
    cb: f32,
}

impl Net {
    fn load() -> Net {
        let all: Vec<f32> = WEIGHTS
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect();
        let mut at = 0;
        let mut take = |n: usize| {
            let v = all[at..at + n].to_vec();
            at += n;
            v
        };
        let (c1w, c1b) = (okc(&take(C1 * NIN * 5), C1, NIN, 5), take(C1));
        let (c2w, c2b) = (okc(&take(C1 * C1 * 3), C1, C1, 3), take(C1));
        let (c3w, c3b) = (okc(&take(C3 * C1 * 3), C3, C1, 3), take(C3));
        let (iw, ib) = (take(H * C3), take(H));
        let blocks = DIL
            .iter()
            .map(|_| {
                [
                    take(H * 3),
                    take(H),
                    take(E * H),
                    take(E),
                    take(H * E),
                    take(H),
                ]
            })
            .collect();
        let cw = take(H);
        let cb = take(1)[0];
        assert_eq!(at, all.len());
        Net {
            lut: gelu_table(),
            c1w,
            c1b,
            c2w,
            c2b,
            c3w,
            c3b,
            iw,
            ib,
            blocks,
            cw,
            cb,
        }
    }
}

/// GELU with erf from Abramowitz and Stegun 7.1.26 (absolute error below 2e-7).
fn gelu_exact(x: f32) -> f32 {
    let z = x * std::f32::consts::FRAC_1_SQRT_2;
    let a = z.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * a);
    let p = t
        * (0.254_829_6
            + t * (-0.284_496_74 + t * (1.421_413_7 + t * (-1.453_152_1 + t * 1.061_405_4))));
    let erf = 1.0 - p * (-a * a).exp();
    0.5 * x * (1.0 + erf.copysign(z))
}

const LUT_MAX: f32 = 8.0;
const LUT_STEP: usize = 512;

/// Tabulated GELU on [-8, 8] with linear interpolation (absolute error below 1e-6).
fn gelu_table() -> Vec<f32> {
    let n = 2 * LUT_MAX as usize * LUT_STEP + 2;
    (0..n)
        .map(|i| gelu_exact(i as f32 / LUT_STEP as f32 - LUT_MAX))
        .collect()
}

fn gelu_lut(t: &[f32], x: f32) -> f32 {
    if x <= -LUT_MAX {
        return 0.0;
    }
    if x >= LUT_MAX {
        return x;
    }
    let u = (x + LUT_MAX) * LUT_STEP as f32;
    let i = u as usize;
    let f = u - i as f32;
    t[i] + f * (t[i + 1] - t[i])
}

/// Dot product with eight independent accumulators (vectorises).
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = [0f32; 8];
    let (ca, ra) = a.as_chunks::<8>();
    let (cb, rb) = b.as_chunks::<8>();
    for (x, y) in ca.iter().zip(cb) {
        for i in 0..8 {
            acc[i] += x[i] * y[i];
        }
    }
    let mut s: f32 = acc.iter().sum();
    for (x, y) in ra.iter().zip(rb) {
        s += x * y;
    }
    s
}

/// Reorders PyTorch convolution weights [out][in][k] to [out][k][in].
fn okc(w: &[f32], cout: usize, cin: usize, k: usize) -> Vec<f32> {
    let mut r = vec![0f32; w.len()];
    for o in 0..cout {
        for c in 0..cin {
            for i in 0..k {
                r[(o * k + i) * cin + c] = w[(o * cin + c) * k + i];
            }
        }
    }
    r
}

/// Causal convolution output: w is [out][k][in], hist holds the last k inputs (oldest first).
fn conv(w: &[f32], b: &[f32], hist: &[Vec<f32>], cin: usize, k: usize, out: &mut [f32]) {
    for (o, y) in out.iter_mut().enumerate() {
        let mut s = b[o];
        for (i, h) in hist.iter().enumerate().take(k) {
            s += dot(&w[(o * k + i) * cin..(o * k + i + 1) * cin], h);
        }
        *y = s;
    }
}

/// Polyphase resampler matching scipy.signal.resample_poly (Kaiser window, beta 5).
struct Resampler {
    up: usize,
    down: usize,
    half: usize,
    h: Vec<f64>,
    x: VecDeque<f64>,
    base: usize,
    n_in: usize,
    n_out: usize,
}

fn bessel_i0(x: f64) -> f64 {
    let (mut s, mut t, mut k) = (1.0, 1.0, 1.0);
    while t > 1e-12 * s {
        t *= (x / (2.0 * k)) * (x / (2.0 * k));
        s += t;
        k += 1.0;
    }
    s
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

impl Resampler {
    fn new(rate: usize) -> Resampler {
        let g = gcd(rate, FS as usize);
        let (up, down) = (FS as usize / g, rate / g);
        let m = up.max(down);
        let half = 10 * m;
        let n = 2 * half + 1;
        let fc = 1.0 / m as f64;
        let beta = 5.0;
        let mut h: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 - half as f64;
                let sinc = if t == 0.0 {
                    1.0
                } else {
                    (std::f64::consts::PI * fc * t).sin() / (std::f64::consts::PI * fc * t)
                };
                let r = 2.0 * i as f64 / (n - 1) as f64 - 1.0;
                fc * sinc * bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / bessel_i0(beta)
            })
            .collect();
        let s: f64 = h.iter().sum();
        for v in &mut h {
            *v *= up as f64 / s;
        }
        Resampler {
            up,
            down,
            half,
            h,
            x: VecDeque::new(),
            base: 0,
            n_in: 0,
            n_out: 0,
        }
    }

    fn push(&mut self, v: f64, out: &mut Vec<f64>) {
        self.x.push_back(v);
        self.n_in += 1;
        loop {
            let top = self.n_out * self.down + self.half;
            if top / self.up >= self.n_in {
                break;
            }
            let mut s = 0.0;
            let lo = top.saturating_sub(self.h.len() - 1);
            let first = lo.div_ceil(self.up);
            for m in first..=top / self.up {
                if m >= self.base {
                    s += self.h[top - m * self.up] * self.x[m - self.base];
                }
            }
            out.push(s);
            self.n_out += 1;
            let keep = (self.n_out * self.down + self.half).saturating_sub(self.h.len()) / self.up;
            while self.base < keep {
                self.x.pop_front();
                self.base += 1;
            }
        }
    }
}

struct Strip {
    xin: VecDeque<Vec<f32>>,
    h1: VecDeque<Vec<f32>>,
    h2: VecDeque<Vec<f32>>,
    blk: Vec<VecDeque<Vec<f32>>>,
    cs: f32,
}

impl Strip {
    fn new() -> Strip {
        let z = |n: usize, d: usize| (0..n).map(|_| vec![0.0; d]).collect::<VecDeque<_>>();
        Strip {
            xin: z(5, NIN),
            h1: z(3, C1),
            h2: z(3, C1),
            blk: DIL.iter().map(|&d| z(2 * d + 1, H)).collect(),
            cs: f32::NAN,
        }
    }
}

/// Runs the network from a cold start on the given strips over feature frames
/// `first..first + feats.len()` (`first` a multiple of 4) and returns, per strip, the
/// smoothed centre score of every network frame k (feature frame 4k) in that range.
fn run(net: &Net, feats: &[&Vec<f32>], strips: &[usize]) -> Vec<Vec<f32>> {
    let n = strips.len();
    let mut st: Vec<Strip> = (0..n).map(|_| Strip::new()).collect();
    let mut out = vec![Vec::new(); n];
    let a = 1.0 - (-1.0f32 / (0.15 * 62.5)).exp();
    let mut y = vec![vec![0f32; H]; n];
    let mut z = vec![vec![0f32; H]; n];
    let mut e = vec![vec![0f32; E]; n];
    for (i, f) in feats.iter().enumerate() {
        for (j, &s) in strips.iter().enumerate() {
            let c = S0 + s - B0;
            let t = &mut st[j];
            let mut v = t.xin.pop_front().unwrap_or_default();
            v.clear();
            v.extend_from_slice(&f[c - CTX..=c + CTX]);
            v.extend_from_slice(&f[NB + c - CTX..=NB + c + CTX]);
            t.xin.push_back(v);
            let mut h = t.h1.pop_front().unwrap_or_default();
            h.resize(C1, 0.0);
            conv(&net.c1w, &net.c1b, t.xin.make_contiguous(), NIN, 5, &mut h);
            h.iter_mut().for_each(|v| *v = gelu_lut(&net.lut, *v));
            t.h1.push_back(h);
            if i % 2 != 0 {
                continue;
            }
            let mut h2 = t.h2.pop_front().unwrap_or_default();
            h2.resize(C1, 0.0);
            conv(&net.c2w, &net.c2b, t.h1.make_contiguous(), C1, 3, &mut h2);
            h2.iter_mut().for_each(|v| *v = gelu_lut(&net.lut, *v));
            t.h2.push_back(h2);
            if i % 4 != 0 {
                continue;
            }
            let mut h3 = vec![0f32; C3];
            conv(&net.c3w, &net.c3b, t.h2.make_contiguous(), C1, 3, &mut h3);
            h3.iter_mut().for_each(|v| *v = gelu_lut(&net.lut, *v));
            for o in 0..H {
                y[j][o] = net.ib[o] + dot(&net.iw[o * C3..(o + 1) * C3], &h3);
            }
        }
        if i % 4 != 0 {
            continue;
        }
        for (k, (&d, w)) in DIL.iter().zip(&net.blocks).enumerate() {
            for j in 0..n {
                let q = &mut st[j].blk[k];
                let mut old = q.pop_front().unwrap_or_default();
                old.clear();
                old.extend_from_slice(&y[j]);
                q.push_back(old);
                let zs = &mut z[j];
                for c in 0..H {
                    zs[c] = w[1][c]
                        + w[0][c * 3] * q[0][c]
                        + w[0][c * 3 + 1] * q[d][c]
                        + w[0][c * 3 + 2] * q[2 * d][c];
                }
                let rms = (zs.iter().map(|v| v * v).sum::<f32>() / H as f32 + 1e-5).sqrt();
                zs.iter_mut().for_each(|v| *v /= rms);
            }
            for o in 0..E {
                let row = &w[2][o * H..(o + 1) * H];
                for j in 0..n {
                    e[j][o] = gelu_lut(&net.lut, w[3][o] + dot(row, &z[j]));
                }
            }
            for o in 0..H {
                let row = &w[4][o * E..(o + 1) * E];
                for j in 0..n {
                    y[j][o] += w[5][o] + dot(row, &e[j]);
                }
            }
        }
        for j in 0..n {
            let p = 1.0 / (1.0 + (-(net.cb + dot(&net.cw, &y[j]))).exp());
            let t = &mut st[j];
            t.cs = if t.cs.is_nan() {
                p
            } else {
                t.cs + a * (p - t.cs)
            };
            out[j].push(t.cs);
        }
    }
    out
}

struct TrackGate {
    last: f64,
    validated: bool,
    open: bool,
}

/// Feature frames run before the queried span (3.2 s; the score then matches a run from
/// the start of the stream to within 4e-6).
const WARM: usize = 800;
/// Feature frames kept (longest span 6 s plus warm-up and slack).
const KEEP_FRAMES: usize = 6 * 250 + WARM + 64;

pub struct Gate {
    net: Net,
    rs: Resampler,
    x8: VecDeque<f64>,
    fft: Fft,
    win: Vec<f64>,
    init: Vec<Vec<f64>>,
    floor: Vec<f64>,
    med: f64,
    started: bool,
    feats: VecDeque<Vec<f32>>,
    nfeat: usize,
    tracks: HashMap<u32, TrackGate>,
    scratch: Vec<f64>,
    seed: u64,
    peak: f64,
}

impl Gate {
    pub fn new(rate: usize) -> Gate {
        Gate {
            net: Net::load(),
            rs: Resampler::new(rate),
            x8: VecDeque::new(),
            fft: Fft::new(NFFT),
            win: (0..NWIN)
                .map(|i| {
                    0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (NWIN - 1) as f64).cos()
                })
                .collect(),
            init: Vec::new(),
            floor: vec![0.0; NB],
            med: 0.0,
            started: false,
            feats: VecDeque::new(),
            nfeat: 0,
            tracks: HashMap::new(),
            scratch: Vec::new(),
            seed: 1,
            peak: 0.0,
        }
    }

    /// Consumes audio at the decoder's rate.
    pub fn feed(&mut self, x: &[f64]) {
        let mut y = std::mem::take(&mut self.scratch);
        for &v in x {
            self.rs.push(v, &mut y);
        }
        // dither about 55 dB below the signal peak: the network was trained on audio that
        // always carries noise, so noiseless input must not reach it
        for v in y.drain(..) {
            self.peak = (self.peak * 0.99999).max(v.abs());
            self.seed = self
                .seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let u = (self.seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5;
            self.x8.push_back(v + DITHER * self.peak * u);
        }
        self.scratch = y;
        while self.x8.len() >= NWIN {
            let mut buf: Vec<C64> = (0..NFFT)
                .map(|i| {
                    if i < NWIN {
                        C64::new(self.x8[i] * self.win[i], 0.0)
                    } else {
                        C64::ZERO
                    }
                })
                .collect();
            self.fft.transform(&mut buf);
            let l: Vec<f64> = (B0..B0 + NB)
                .map(|b| 10.0 * (buf[b].re * buf[b].re + buf[b].im * buf[b].im + 1e-10).log10())
                .collect();
            self.x8.drain(..HOP);
            if self.started {
                self.frame(&l);
            } else {
                self.init.push(l);
                if self.init.len() == INIT_FRAMES {
                    self.start();
                }
            }
        }
    }

    fn start(&mut self) {
        self.started = true;
        let init = std::mem::take(&mut self.init);
        for b in 0..NB {
            self.floor[b] = init.iter().map(|l| l[b]).sum::<f64>() / init.len() as f64;
        }
        self.med = median(&init[0]);
        for l in &init {
            self.frame(l);
        }
    }

    fn frame(&mut self, l: &[f64]) {
        let rate = 250.0f64;
        let am = 1.0 - (-1.0 / (0.3 * rate)).exp();
        let ad = 1.0 - (-1.0 / (0.04 * rate)).exp();
        let au = 1.0 - (-1.0 / (1.5 * rate)).exp();
        self.med += am * (median(l) - self.med);
        let mut f = if self.feats.len() >= KEEP_FRAMES {
            self.feats.pop_front().unwrap_or_default()
        } else {
            Vec::new()
        };
        f.resize(2 * NB, 0.0);
        for b in 0..NB {
            let d = l[b] - self.floor[b];
            self.floor[b] += if d > 0.0 { au } else { ad } * d;
            f[b] = ((l[b] - self.floor[b]) / 10.0).clamp(-3.0, 6.0) as f32;
            f[NB + b] = ((l[b] - self.med) / 10.0).clamp(-3.0, 6.0) as f32;
        }
        self.feats.push_back(f);
        self.nfeat += 1;
    }

    /// Smoothed centre scores of `strips` for network frames `k0..=k1` (cold start WARM
    /// feature frames earlier, or at the first kept frame).
    fn centre(&self, strips: &[usize], k0: usize, k1: usize) -> Vec<Vec<f32>> {
        let kept0 = self.nfeat - self.feats.len();
        let first = ((4 * k0).saturating_sub(WARM).max(kept0)).div_ceil(4) * 4;
        let last = (4 * k1).min(self.nfeat.saturating_sub(1));
        if first > last {
            return vec![Vec::new(); strips.len()];
        }
        let fs: Vec<&Vec<f32>> = (first..=last).map(|i| &self.feats[i - kept0]).collect();
        let skip = k0.saturating_sub(first / 4);
        run(&self.net, &fs, strips)
            .into_iter()
            .map(|v| v.into_iter().skip(skip).collect())
            .collect()
    }

    fn presence(&self, t: f64, hz: f64, chars: usize) -> f32 {
        let span = (0.6 + 0.3 * chars as f64).min(6.0);
        let s = (hz / (FS / NFFT as f64)).round() as i64 - S0 as i64;
        let (lo, hi) = ((s - 2).max(0), (s + 3).min(NS as i64));
        if hi <= lo || self.nfeat == 0 {
            return 0.0;
        }
        let frame = |sec: f64| ((sec - 0.044) / 0.016).floor().max(0.0) as usize;
        let (k0, k1) = (frame(t - span), frame(t));
        let strips: Vec<usize> = (lo as usize..hi as usize).collect();
        self.centre(&strips, k0, k1)
            .iter()
            .flatten()
            .fold(0f32, |m, &v| m.max(v))
    }

    /// Verdict on a commit of `text` on `track` at `t` seconds and `hz`.
    /// A track is validated by the first commit whose score reaches `OPEN` (commits before
    /// it are held and then released); after that it is checked at commits that follow
    /// more than `HOLD_SEC` without commits, against `KEEP`.
    pub fn verdict(&mut self, track: u32, t: f64, hz: f64, text: &str) -> Verdict {
        let n = text.chars().filter(|&c| c != ' ').count();
        let g = self.tracks.entry(track).or_insert(TrackGate {
            last: f64::NEG_INFINITY,
            validated: false,
            open: false,
        });
        let (last, validated) = (g.last, g.validated);
        g.last = t;
        let v = if !validated {
            if self.presence(t, hz, n) >= OPEN {
                Verdict::Release
            } else {
                Verdict::Hold
            }
        } else if t - last > HOLD_SEC {
            if self.presence(t, hz, n) >= KEEP {
                Verdict::Release
            } else {
                Verdict::Drop
            }
        } else if self.tracks[&track].open {
            Verdict::Release
        } else {
            Verdict::Drop
        };
        let g = self.tracks.get_mut(&track).expect("track");
        match v {
            Verdict::Release => {
                g.validated = true;
                g.open = true;
            }
            Verdict::Drop => g.open = false,
            Verdict::Hold => {}
        }
        v
    }

    /// Forgets a lost track.
    pub fn lost(&mut self, track: u32) {
        self.tracks.remove(&track);
    }
}

/// What happens to a commit.
pub enum Verdict {
    /// Released now (with any commits held for the track).
    Release,
    /// Held until the track is validated.
    Hold,
    /// Dropped.
    Drop,
}

fn median(l: &[f64]) -> f64 {
    let mut v = l.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_cover_the_network() {
        let n = Net::load();
        assert_eq!(n.cw.len(), H);
    }

    #[test]
    fn silence_opens_no_track() {
        let mut g = Gate::new(8000);
        g.feed(&vec![0.0; 8000 * 3]);
        assert!(matches!(g.verdict(0, 2.5, 700.0, "TEST"), Verdict::Hold));
    }
}

#[cfg(test)]
mod reference {
    use super::*;

    /// Compares smoothed centre scores with the PyTorch reference when CW4I_GATE_REF names
    /// a directory holding ref_x.f64 (8 kHz audio) and ref_cen.f32 (68 x T raw scores).
    #[test]
    fn matches_pytorch_reference() {
        let Some(dir) = std::env::var_os("CW4I_GATE_REF") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        let x: Vec<f64> = std::fs::read(dir.join("ref_x.f64"))
            .unwrap()
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| f64::from_le_bytes(*c))
            .collect();
        let cen: Vec<f32> = std::fs::read(dir.join("ref_cen.f32"))
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect();
        let t = cen.len() / NS;
        let mut g = Gate::new(8000);
        g.feed(&x);
        let a = 1.0 - (-1.0f32 / (0.15 * 62.5)).exp();
        let mut worst = 0f32;
        let all: Vec<usize> = (0..NS).collect();
        let k1 = (g.nfeat - 1) / 4;
        // full history and a cold start 2.24 s back (as used by the gate)
        for k0 in [0, k1 - 20] {
            let got = g.centre(&all, k0, k1);
            for s in 0..NS {
                let mut v = cen[s * t];
                for k in 0..=k1.min(t - 1) {
                    v += a * (cen[s * t + k] - v);
                    if k >= k0 {
                        worst = worst.max((got[s][k - k0] - v).abs());
                    }
                }
            }
            eprintln!("k0 {k0}: worst so far {worst}");
        }
        eprintln!("frames {} of {t}, worst abs difference {worst}", k1 + 1);
        assert!(k1 + 3 >= t && worst < 1e-3);
    }
}

#[cfg(test)]
mod timing {
    use super::*;

    #[test]
    fn cost_per_audio_second() {
        if std::env::var_os("CW4I_GATE_TIME").is_none() {
            return;
        }
        for rate in [8000usize, 48000] {
            let x: Vec<f64> = (0..rate * 20)
                .map(|i| ((i * 7919) % 1000) as f64 / 1000.0 - 0.5)
                .collect();
            let mut r = Resampler::new(rate);
            let t = std::time::Instant::now();
            let mut y = Vec::new();
            for &v in &x {
                r.push(v, &mut y);
            }
            let tr = t.elapsed().as_secs_f64() / 20.0;
            let mut g = Gate::new(rate);
            let t = std::time::Instant::now();
            g.feed(&x);
            let tf = t.elapsed().as_secs_f64() / 20.0;
            let t = std::time::Instant::now();
            let p = g.presence(19.0, 700.0, 20);
            eprintln!("rate {rate}: resampler {tr:.4} s, front end {tf:.4} s CPU per audio second; one check of a 6 s span {:.4} s (p {p:.2})", t.elapsed().as_secs_f64());
        }
    }
}
