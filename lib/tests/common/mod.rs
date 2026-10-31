//! Test-only signal synthesiser: keyed tones with raised-cosine edges, AWGN at a given
//! SNR, fading, timing jitter and several stations, with a deterministic PRNG.
#![allow(dead_code)]

use cw4i::{Config, Decoder, Event};

pub const RATE: u32 = 8000;

/// xorshift64* generator.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniform in [0, 1).
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Standard normal (Box-Muller).
    pub fn normal(&mut self) -> f64 {
        let u1 = self.uniform().max(1e-300);
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

pub fn code(c: char) -> &'static str {
    match c {
        'A' => ".-",
        'B' => "-...",
        'C' => "-.-.",
        'D' => "-..",
        'E' => ".",
        'F' => "..-.",
        'G' => "--.",
        'H' => "....",
        'I' => "..",
        'J' => ".---",
        'K' => "-.-",
        'L' => ".-..",
        'M' => "--",
        'N' => "-.",
        'O' => "---",
        'P' => ".--.",
        'Q' => "--.-",
        'R' => ".-.",
        'S' => "...",
        'T' => "-",
        'U' => "..-",
        'V' => "...-",
        'W' => ".--",
        'X' => "-..-",
        'Y' => "-.--",
        'Z' => "--..",
        '0' => "-----",
        '1' => ".----",
        '2' => "..---",
        '3' => "...--",
        '4' => "....-",
        '5' => ".....",
        '6' => "-....",
        '7' => "--...",
        '8' => "---..",
        '9' => "----.",
        '/' => "-..-.",
        '?' => "..--..",
        '=' => "-...-",
        '.' => ".-.-.-",
        ',' => "--..--",
        _ => "",
    }
}

/// One transmitting station.
#[derive(Clone)]
pub struct Station {
    pub text: String,
    pub freq: f64,
    pub wpm: f64,
    /// Start time in seconds.
    pub start: f64,
    /// Linear amplitude of the carrier.
    pub amp: f64,
    /// Relative sd of each element's duration (timing jitter).
    pub jitter: f64,
    /// Periodic fading: depth (0..1) and rate in Hz.
    pub qsb_depth: f64,
    pub qsb_hz: f64,
    /// Rayleigh-like fading bandwidth in Hz (0 = off).
    pub fade_hz: f64,
    pub seed: u64,
}

impl Station {
    pub fn new(text: &str, freq: f64, wpm: f64) -> Station {
        Station {
            text: text.to_string(),
            freq,
            wpm,
            start: 0.5,
            amp: 0.3,
            jitter: 0.0,
            qsb_depth: 0.0,
            qsb_hz: 0.0,
            fade_hz: 0.0,
            seed: 1,
        }
    }

    /// Key-down intervals (start, end) in seconds.
    pub fn intervals(&self) -> Vec<(f64, f64)> {
        let mut rng = Rng::new(self.seed ^ 0x5151);
        let dit = 1.2 / self.wpm;
        let mut jit = |d: f64| d * (1.0 + self.jitter * rng.normal()).clamp(0.3, 3.0);
        let mut t = self.start;
        let mut out = Vec::new();
        let words: Vec<&str> = self.text.split(' ').filter(|w| !w.is_empty()).collect();
        for (wi, w) in words.iter().enumerate() {
            if wi > 0 {
                t += jit(7.0 * dit);
            }
            for (ci, c) in w.chars().enumerate() {
                if ci > 0 {
                    t += jit(3.0 * dit);
                }
                for (ei, e) in code(c).chars().enumerate() {
                    if ei > 0 {
                        t += jit(dit);
                    }
                    let d = jit(if e == '.' { dit } else { 3.0 * dit });
                    out.push((t, t + d));
                    t += d;
                }
            }
        }
        out
    }

    /// End of the last mark in seconds.
    pub fn end(&self) -> f64 {
        self.intervals().last().map(|x| x.1).unwrap_or(self.start)
    }

    /// Adds the keyed tone to x.
    pub fn render(&self, x: &mut [f64]) {
        let rate = RATE as f64;
        let edge = 0.005;
        let mut rng = Rng::new(self.seed ^ 0xFADE);
        // fading: a few random sinusoids form a slowly varying complex gain
        let comps: Vec<(f64, f64, f64)> = (0..8)
            .map(|_| {
                (
                    self.fade_hz * (2.0 * rng.uniform() - 1.0),
                    2.0 * std::f64::consts::PI * rng.uniform(),
                    rng.normal(),
                )
            })
            .collect();
        let phase0 = 2.0 * std::f64::consts::PI * rng.uniform();
        for (a, b) in self.intervals() {
            let i0 = ((a - edge) * rate).max(0.0) as usize;
            let i1 = (((b + edge) * rate) as usize).min(x.len());
            for (i, xi) in x.iter_mut().enumerate().take(i1).skip(i0) {
                let t = i as f64 / rate;
                let env = if t < a - edge / 2.0 {
                    0.0
                } else if t < a + edge / 2.0 {
                    0.5 - 0.5 * (std::f64::consts::PI * (t - a + edge / 2.0) / edge).cos()
                } else if t < b - edge / 2.0 {
                    1.0
                } else if t < b + edge / 2.0 {
                    0.5 + 0.5 * (std::f64::consts::PI * (t - b + edge / 2.0) / edge).cos()
                } else {
                    0.0
                };
                let mut g = 1.0;
                if self.qsb_depth > 0.0 {
                    g *= 1.0
                        - self.qsb_depth
                            * (0.5 + 0.5 * (2.0 * std::f64::consts::PI * self.qsb_hz * t).sin());
                }
                let mut ph = 0.0;
                if self.fade_hz > 0.0 {
                    let (mut re, mut im) = (0.0, 0.0);
                    for &(f, p, w) in &comps {
                        let th = 2.0 * std::f64::consts::PI * f * t + p;
                        re += w * th.cos();
                        im += w * th.sin();
                    }
                    let n = (comps.len() as f64).sqrt();
                    g *= (re * re + im * im).sqrt() / n;
                    ph = im.atan2(re);
                }
                *xi += self.amp
                    * g
                    * env
                    * (2.0 * std::f64::consts::PI * self.freq * t + phase0 + ph).sin();
            }
        }
    }
}

/// White Gaussian noise whose power in 2500 Hz gives the stated SNR for a carrier of amplitude `amp`.
pub fn add_noise(x: &mut [f64], amp: f64, snr_db: f64, seed: u64) {
    if !snr_db.is_finite() {
        return;
    }
    let ps = amp * amp / 2.0;
    let pn_band = ps / 10f64.powf(snr_db / 10.0);
    let sigma = (pn_band * (RATE as f64 / 2.0) / 2500.0).sqrt();
    let mut rng = Rng::new(seed);
    for v in x.iter_mut() {
        *v += sigma * rng.normal();
    }
}

/// Renders stations plus noise over `secs` seconds.
pub fn scene(stations: &[Station], secs: f64, snr_db: f64, seed: u64) -> Vec<f64> {
    let mut x = vec![0.0; (secs * RATE as f64) as usize];
    for s in stations {
        s.render(&mut x);
    }
    let amp = stations.first().map(|s| s.amp).unwrap_or(0.3);
    add_noise(&mut x, amp, snr_db, seed);
    x
}

/// Decodes x in blocks of `block` samples (streaming, no flush).
pub fn decode(x: &[f64], block: usize, cfg: Config) -> Vec<Event> {
    let mut d = Decoder::with_config(RATE, cfg);
    let mut ev = Vec::new();
    for c in x.chunks(block) {
        ev.extend(d.push_f64(c));
    }
    ev
}

/// As [`decode`], followed by the end-of-stream flush.
pub fn decode_flush(x: &[f64], block: usize, cfg: Config) -> Vec<Event> {
    let mut d = Decoder::with_config(RATE, cfg);
    let mut ev = Vec::new();
    for c in x.chunks(block) {
        ev.extend(d.push_f64(c));
    }
    ev.extend(d.flush());
    ev
}

/// Committed text per track: (track, freq, text, sample of first commit).
pub fn texts(ev: &[Event]) -> Vec<(u32, f64, String)> {
    let mut v: Vec<(u32, f64, String)> = Vec::new();
    for e in ev {
        if let Event::Commit {
            track, freq, text, ..
        } = e
        {
            match v.iter_mut().find(|t| t.0 == *track) {
                Some(t) => {
                    t.1 = *freq;
                    t.2.push_str(text);
                }
                None => v.push((*track, *freq, text.clone())),
            }
        }
    }
    for t in &mut v {
        t.2 = t.2.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    v.retain(|t| !t.2.is_empty());
    v
}

/// Edit distance between a and b (characters, spaces removed), over the length of b.
pub fn cer(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().filter(|c| *c != ' ').collect();
    let b: Vec<char> = b.chars().filter(|c| *c != ' ').collect();
    let mut d: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = d[0];
        d[0] = i;
        for j in 1..=b.len() {
            let cur = d[j];
            let c = usize::from(a[i - 1] != b[j - 1]);
            d[j] = (d[j] + 1).min(d[j - 1] + 1).min(prev + c);
            prev = cur;
        }
    }
    d[b.len()] as f64 / b.len().max(1) as f64
}

/// Number of committed non-space characters.
pub fn n_committed(ev: &[Event]) -> usize {
    texts(ev)
        .iter()
        .map(|t| t.2.chars().filter(|c| *c != ' ').count())
        .sum()
}
