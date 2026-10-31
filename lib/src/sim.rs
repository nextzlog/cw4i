//! Deterministic CW signal simulator for self-tests and benchmarks.
//!
//! A [`Scene`] renders [`Station`]s keyed by a human or machine fist, with speed drift,
//! weight, dah ratio, rise time, fading and frequency drift, and adds AWGN, QRM tones,
//! impulsive and machine noise and a sample-rate error. [`score`] runs the decoder over a
//! scene and reports the character error rate of every station.

use crate::{Config, Decoder, Event};
use std::f64::consts::PI;

const WANDER_KEEP: f64 = 0.95;
const WANDER_STEP: f64 = 0.05;
const PATH_TONES: usize = 16;
const CHIRP_SEC: f64 = 0.01;
const CRASH_SEC: f64 = 0.03;
const WANDER_HZ: f64 = 0.05;
const AGC_ATTACK: f64 = 0.001;
const AGC_TARGET: f64 = 0.3;
const AGC_FLOOR: f64 = 0.01;

/// xorshift64* generator.
#[derive(Clone, Debug)]
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
        (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos()
    }
}

/// Dot and dash pattern of a character, empty if unknown.
pub fn code(c: char) -> &'static str {
    match c.to_ascii_uppercase() {
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

/// How the operator keys.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fist {
    /// Every element and space varies independently by `jitter`.
    Even,
    /// Paddle and electronic keyer: exact elements, hand-made character and word spaces.
    Paddle,
    /// Semi-automatic bug: machine dots, hand dashes with a wandering ratio.
    Bug,
    /// Straight key: hand timing whose speed, weight and dah ratio drift.
    Straight,
}

/// Two-path Watterson channel (ITU-R F.1487): Doppler spread (2 sigma) and differential
/// delay of two equal Rayleigh paths, plus a Doppler shift.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Channel {
    pub spread: f64,
    pub delay: f64,
    pub shift: f64,
}

impl Channel {
    pub const NONE: Channel = Channel::new(0.0, 0.0);
    pub const MID_QUIET: Channel = Channel::new(0.1, 0.0005);
    pub const MID_MODERATE: Channel = Channel::new(0.5, 0.001);
    pub const MID_DISTURBED: Channel = Channel::new(1.0, 0.002);
    pub const LOW_DISTURBED: Channel = Channel::new(10.0, 0.006);
    pub const HIGH_MODERATE: Channel = Channel::new(10.0, 0.003);
    pub const HIGH_DISTURBED: Channel = Channel::new(30.0, 0.007);

    pub const fn new(spread: f64, delay: f64) -> Channel {
        Channel {
            spread,
            delay,
            shift: 0.0,
        }
    }

    /// The presets with their names.
    pub fn presets() -> [(&'static str, Channel); 7] {
        [
            ("none", Channel::NONE),
            ("mid-quiet", Channel::MID_QUIET),
            ("mid-moderate", Channel::MID_MODERATE),
            ("mid-disturbed", Channel::MID_DISTURBED),
            ("low-disturbed", Channel::LOW_DISTURBED),
            ("high-moderate", Channel::HIGH_MODERATE),
            ("high-disturbed", Channel::HIGH_DISTURBED),
        ]
    }
}

/// One transmitting station.
#[derive(Clone, Debug)]
pub struct Station {
    pub text: String,
    /// Carrier frequency in Hz.
    pub freq: f64,
    /// Speed at the start and at the end of the message (linear in time).
    pub wpm: f64,
    pub wpm_end: f64,
    /// Share of a dit period keyed down (0.5 is standard).
    pub weight: f64,
    /// Dah length in dits.
    pub ratio: f64,
    /// Relative sd of each hand-made element's duration.
    pub jitter: f64,
    pub fist: Fist,
    /// Start time in seconds.
    pub start: f64,
    /// Linear carrier amplitude.
    pub amp: f64,
    /// 10-90 % rise time of the raised-cosine edge in seconds (0 = hard keying with clicks).
    pub rise: f64,
    /// Frequency drift in Hz per second.
    pub drift: f64,
    /// Chirp: frequency offset in Hz at key-down, decaying over 10 ms.
    pub chirp: f64,
    /// Periodic fading (QSB): depth (0..1) and rate in Hz.
    pub qsb_depth: f64,
    pub qsb_hz: f64,
    /// Single-path Rayleigh fading bandwidth in Hz (0 = off).
    pub fade_hz: f64,
    pub channel: Channel,
    pub seed: u64,
}

impl Station {
    pub fn new(text: &str, freq: f64, wpm: f64) -> Station {
        Station {
            text: text.to_string(),
            freq,
            wpm,
            wpm_end: wpm,
            weight: 0.5,
            ratio: 3.0,
            jitter: 0.0,
            fist: Fist::Even,
            start: 0.5,
            amp: 0.3,
            rise: 0.004,
            drift: 0.0,
            chirp: 0.0,
            qsb_depth: 0.0,
            qsb_hz: 0.0,
            fade_hz: 0.0,
            channel: Channel::NONE,
            seed: 1,
        }
    }

    /// Key-down intervals (start, end) in seconds.
    pub fn intervals(&self) -> Vec<(f64, f64)> {
        let mut rng = Rng::new(self.seed ^ 0x5151);
        let words: Vec<&str> = self.text.split_whitespace().collect();
        let units: f64 = words
            .iter()
            .map(|w| {
                w.chars()
                    .map(|c| {
                        code(c)
                            .chars()
                            .map(|e| if e == '.' { 2.0 } else { 4.0 })
                            .sum::<f64>()
                            + 2.0
                    })
                    .sum::<f64>()
                    + 4.0
            })
            .sum::<f64>()
            .max(1.0);
        let straight = self.fist == Fist::Straight;
        let wander = self.fist == Fist::Straight || self.fist == Fist::Bug;
        let (mut ls, mut dw, mut dr) = (0.0, 0.0, 0.0);
        let mut done = 0.0;
        let mut t = self.start;
        let mut out = Vec::new();
        let mut step = |rng: &mut Rng, t: &mut f64, n: f64, hand: bool, ls: f64| {
            let wpm = self.wpm + (self.wpm_end - self.wpm) * (done / units).min(1.0);
            done += n;
            let j = if hand {
                self.jitter * rng.normal()
            } else {
                0.0
            };
            *t += n * 1.2 / wpm.max(1.0) * ls.exp() * (1.0 + j).clamp(0.3, 3.0);
        };
        for (wi, word) in words.iter().enumerate() {
            for (ci, c) in word.chars().enumerate() {
                if straight {
                    ls = WANDER_KEEP * ls + WANDER_STEP * rng.normal();
                    dw = WANDER_KEEP * dw + 0.5 * WANDER_STEP * rng.normal();
                }
                if wander {
                    dr = WANDER_KEEP * dr + 3.0 * WANDER_STEP * rng.normal();
                }
                let w = 2.0 * (self.weight + dw).clamp(0.3, 0.7);
                let ratio = (self.ratio + dr).clamp(2.0, 5.0);
                if ci > 0 {
                    step(&mut rng, &mut t, 4.0 - w, true, ls);
                } else if wi > 0 {
                    step(&mut rng, &mut t, 8.0 - w, true, ls);
                }
                for (ei, e) in code(c).chars().enumerate() {
                    let dah = e == '-';
                    let machine = match self.fist {
                        Fist::Even | Fist::Straight => false,
                        Fist::Paddle => true,
                        Fist::Bug => !dah,
                    };
                    if ei > 0 {
                        step(&mut rng, &mut t, 2.0 - w, !machine, ls);
                    }
                    let a = t;
                    let n = if dah { ratio } else { 1.0 };
                    step(&mut rng, &mut t, n - 1.0 + w, !machine, ls);
                    out.push((a, t));
                }
            }
        }
        out
    }

    /// End of the last mark in seconds.
    pub fn end(&self) -> f64 {
        self.intervals().last().map(|x| x.1).unwrap_or(self.start)
    }

    /// Adds the keyed tone, with time t of sample i at i / rate.
    pub fn render(&self, x: &mut [f64], rate: f64) {
        let mut rng = Rng::new(self.seed ^ 0xFADE);
        let comps: Vec<(f64, f64, f64)> = (0..8)
            .map(|_| {
                (
                    self.fade_hz * (2.0 * rng.uniform() - 1.0),
                    2.0 * PI * rng.uniform(),
                    rng.normal(),
                )
            })
            .collect();
        let phase0 = 2.0 * PI * rng.uniform();
        let ch = self.channel;
        let paths: Vec<(f64, Vec<(f64, f64)>)> = if ch.spread > 0.0 {
            [0.0, ch.delay]
                .iter()
                .map(|&d| {
                    let taps = (0..PATH_TONES)
                        .map(|_| (0.5 * ch.spread * rng.normal(), 2.0 * PI * rng.uniform()))
                        .collect();
                    (d, taps)
                })
                .collect()
        } else {
            vec![(0.0, Vec::new())]
        };
        let edge = self.rise / 0.6;
        let env = |t: f64, a: f64, b: f64| -> f64 {
            if edge <= 0.0 {
                return f64::from(t >= a && t < b);
            }
            let up = ((t - a) / edge + 0.5).clamp(0.0, 1.0);
            let down = ((b - t) / edge + 0.5).clamp(0.0, 1.0);
            (0.5 - 0.5 * (PI * up).cos()) * (0.5 - 0.5 * (PI * down).cos())
        };
        let norm = (paths.len() as f64).sqrt();
        for (a, b) in self.intervals() {
            let i0 = ((a - edge) * rate).max(0.0) as usize;
            let i1 = (((b + edge + ch.delay) * rate) as usize + 1).min(x.len());
            for (i, xi) in x.iter_mut().enumerate().take(i1).skip(i0) {
                let t = i as f64 / rate;
                let mut g = 1.0 - self.qsb_depth * (0.5 + 0.5 * (2.0 * PI * self.qsb_hz * t).sin());
                let mut ph = 0.0;
                if self.fade_hz > 0.0 {
                    let (mut re, mut im) = (0.0, 0.0);
                    for &(f, p, w) in &comps {
                        let th = 2.0 * PI * f * t + p;
                        re += w * th.cos();
                        im += w * th.sin();
                    }
                    g *= (re * re + im * im).sqrt() / (comps.len() as f64).sqrt();
                    ph = im.atan2(re);
                }
                let mut v = 0.0;
                for (d, taps) in &paths {
                    let td = t - d;
                    let e = env(td, a, b);
                    if e == 0.0 {
                        continue;
                    }
                    let (mut re, mut im) = (1.0, 0.0);
                    if !taps.is_empty() {
                        (re, im) = (0.0, 0.0);
                        for &(f, p) in taps {
                            let th = 2.0 * PI * f * t + p;
                            re += th.cos();
                            im += th.sin();
                        }
                        re /= (taps.len() as f64).sqrt() * norm;
                        im /= (taps.len() as f64).sqrt() * norm;
                    }
                    let chirp = 2.0
                        * PI
                        * self.chirp
                        * CHIRP_SEC
                        * (1.0 - (-(td - a).max(0.0) / CHIRP_SEC).exp());
                    let th = 2.0 * PI * ((self.freq + ch.shift) * td + 0.5 * self.drift * td * td)
                        + phase0
                        + ph
                        + chirp;
                    v += e * (re * th.sin() + im * th.cos());
                }
                *xi += self.amp * g * v;
            }
        }
    }
}

/// A steady interfering carrier.
#[derive(Clone, Debug)]
pub struct Tone {
    pub freq: f64,
    pub amp: f64,
}

/// Audio of several stations plus noise.
#[derive(Clone, Debug)]
pub struct Scene {
    /// Nominal sample rate given to the decoder.
    pub rate: u32,
    /// Sample-rate error of the sound card in parts per million.
    pub rate_ppm: f64,
    pub stations: Vec<Station>,
    /// SNR in 2500 Hz of the first station's carrier (infinite = no AWGN).
    pub snr_db: f64,
    pub qrm: Vec<Tone>,
    /// Impulsive noise: mean impulses per second and peak amplitude.
    pub impulse_hz: f64,
    pub impulse_amp: f64,
    /// Machine noise: buzz fundamental in Hz and amplitude (odd harmonics up to 3 kHz).
    pub buzz_hz: f64,
    pub buzz_amp: f64,
    /// Static crashes (QRN): mean crashes per second and peak amplitude, 30 ms decay.
    pub crash_hz: f64,
    pub crash_amp: f64,
    /// Receiver AGC release in seconds (0 = off), 1 ms attack, applied after the noise.
    pub agc: f64,
    /// Swing (dB peak to peak) of the noise level at 0.05 Hz, and its step (dB) halfway.
    pub noise_wander: f64,
    pub noise_step: f64,
    /// Silence after the last mark in seconds.
    pub tail: f64,
    pub seed: u64,
}

impl Scene {
    pub fn new(rate: u32, stations: Vec<Station>, snr_db: f64, seed: u64) -> Scene {
        Scene {
            rate,
            rate_ppm: 0.0,
            stations,
            snr_db,
            qrm: Vec::new(),
            impulse_hz: 0.0,
            impulse_amp: 0.0,
            buzz_hz: 0.0,
            buzz_amp: 0.0,
            crash_hz: 0.0,
            crash_amp: 0.0,
            agc: 0.0,
            noise_wander: 0.0,
            noise_step: 0.0,
            tail: 2.0,
            seed,
        }
    }

    /// One station sending `text` at 700 Hz.
    pub fn single(text: &str, wpm: f64, snr_db: f64, seed: u64) -> Scene {
        let mut st = Station::new(text, 700.0, wpm);
        st.seed = seed;
        Scene::new(8000, vec![st], snr_db, seed)
    }

    /// `n` stations at random pitches within `spread` Hz of 700 Hz, random speeds in
    /// `wpm`, amplitudes within 10 dB and staggered starts, each sending its own callsign.
    pub fn pileup(n: usize, spread: f64, wpm: (f64, f64), snr_db: f64, seed: u64) -> Scene {
        let mut rng = Rng::new(seed ^ 0x9111);
        let mut stations: Vec<Station> = Vec::new();
        while stations.len() < n {
            let f = 700.0 + spread * (2.0 * rng.uniform() - 1.0);
            if stations.iter().any(|s| (s.freq - f).abs() < 30.0) {
                continue;
            }
            let call = plain_callsign(&mut rng);
            let w = wpm.0 + (wpm.1 - wpm.0) * rng.uniform();
            let mut st = Station::new(&format!("{call} {call}"), f, w);
            st.amp = 0.3 * 10f64.powf(-rng.uniform() / 2.0);
            st.start = 0.5 + 1.5 * rng.uniform();
            st.jitter = 0.05 * rng.uniform();
            st.seed = rng.next_u64();
            stations.push(st);
        }
        stations[0].amp = 0.3;
        Scene::new(8000, stations, snr_db, seed)
    }

    /// `n` contest callers within `spread` Hz of 700 Hz (at least 30 Hz apart unless
    /// `spread` is 0), speeds in `wpm`, amplitudes within 10 dB, paddle fists, mild QSB and
    /// starts within a second, each sending its callsign and an exchange.
    pub fn contest(n: usize, spread: f64, wpm: (f64, f64), snr_db: f64, seed: u64) -> Scene {
        let mut rng = Rng::new(seed ^ 0xC0DE);
        let mut stations: Vec<Station> = Vec::new();
        while stations.len() < n {
            let f = 700.0 + spread * (2.0 * rng.uniform() - 1.0);
            if spread > 0.0 && stations.iter().any(|s| (s.freq - f).abs() < 30.0) {
                continue;
            }
            let call = callsign(&mut rng);
            let text = format!("{call} {}", exchange(&mut rng));
            let w = wpm.0 + (wpm.1 - wpm.0) * rng.uniform();
            let mut st = Station::new(&text, f, w);
            st.fist = Fist::Paddle;
            st.jitter = 0.05;
            st.amp = 0.3 * 10f64.powf(-rng.uniform() / 2.0);
            st.start = 0.5 + rng.uniform();
            st.qsb_depth = 0.3 * rng.uniform();
            st.qsb_hz = 0.1 + 0.2 * rng.uniform();
            st.seed = rng.next_u64();
            stations.push(st);
        }
        stations[0].amp = 0.3;
        Scene::new(8000, stations, snr_db, seed)
    }

    /// `secs` seconds of a contest band: a running station at 700 Hz calling CQ and
    /// callers that arrive at `rate` per second for 3 s after each CQ within 300 Hz, with
    /// mixed fists, 25 to 40 WPM, levels from 0 to -12 dB, QSB and mid-latitude fading,
    /// static crashes and a receiver AGC.
    pub fn operating(secs: f64, rate: f64, snr_db: f64, seed: u64) -> Scene {
        let mut rng = Rng::new(seed ^ 0x0BE7);
        let mut stations = Vec::new();
        let mut t = 0.5;
        let mut k = 0;
        while t < secs {
            let mut cq = Station::new("CQ TEST JA1ZLO", 700.0, 32.0);
            cq.fist = Fist::Paddle;
            cq.start = t;
            cq.seed = rng.next_u64();
            t = cq.end() + 1.0;
            stations.push(cq);
            let mut wait = 0.0;
            while wait < 3.0 {
                wait += -(1.0 - rng.uniform()).ln() / rate;
                if wait > 3.0 {
                    break;
                }
                let call = callsign(&mut rng);
                let mut st = Station::new(
                    &format!("{call} {}", exchange(&mut rng)),
                    700.0 + 300.0 * (2.0 * rng.uniform() - 1.0),
                    25.0 + 15.0 * rng.uniform(),
                );
                st.wpm_end = st.wpm * (0.95 + 0.1 * rng.uniform());
                st.fist = [Fist::Paddle, Fist::Paddle, Fist::Bug, Fist::Straight][k % 4];
                k += 1;
                st.jitter = 0.03 + 0.05 * rng.uniform();
                st.amp = 0.3 * 10f64.powf(-0.6 * rng.uniform());
                st.start = t + wait;
                st.qsb_depth = 0.5 * rng.uniform();
                st.qsb_hz = 0.05 + 0.3 * rng.uniform();
                st.channel = Channel::MID_QUIET;
                st.seed = rng.next_u64();
                stations.push(st);
            }
            let busy = stations.iter().map(|s| s.end()).fold(t, f64::max);
            t = busy + 0.5 + 1.5 * rng.uniform();
        }
        let mut sc = Scene::new(8000, stations, snr_db, seed);
        sc.crash_hz = 0.3;
        sc.crash_amp = 0.6;
        sc.agc = 0.3;
        sc
    }

    /// Conditions measured on real recordings: SNR from -5 to 10 dB in 2500 Hz, slow QSB
    /// of up to 12 dB at 0.05 to 0.5 Hz, mild impulsive noise, a straight key or bug and,
    /// when `neighbour` is set, a station up to 5 dB stronger 60 to 300 Hz away.
    pub fn realistic(text: &str, neighbour: bool, seed: u64) -> Scene {
        let mut rng = Rng::new(seed ^ 0x4EA1);
        let wpm = 18.0 + 12.0 * rng.uniform();
        let mut st = Station::new(text, 600.0 + 200.0 * rng.uniform(), wpm);
        st.seed = seed;
        st.fist = if rng.uniform() < 0.5 {
            Fist::Straight
        } else {
            Fist::Bug
        };
        st.jitter = 0.05 + 0.05 * rng.uniform();
        st.qsb_depth = 0.75 * rng.uniform();
        st.qsb_hz = 0.05 + 0.45 * rng.uniform();
        st.channel = Channel::MID_QUIET;
        let snr = -5.0 + 15.0 * rng.uniform();
        let mut stations = vec![st];
        if neighbour {
            let s0 = &stations[0];
            let side = if rng.uniform() < 0.5 { -1.0 } else { 1.0 };
            let mut nb = Station::new(
                "CQ TEST CQ TEST DE JA1ZLO JA1ZLO TEST",
                s0.freq + side * (60.0 + 240.0 * rng.uniform()),
                22.0 + 10.0 * rng.uniform(),
            );
            nb.amp = s0.amp * 10f64.powf(5.0 * rng.uniform() / 20.0);
            nb.start = 0.2;
            nb.seed = seed ^ 0x77;
            stations.push(nb);
        }
        let mut sc = Scene::new(8000, stations, snr, seed);
        sc.impulse_hz = 5.0 * rng.uniform();
        sc.impulse_amp = 0.3;
        sc
    }

    /// Background without stations for `secs` seconds, at the noise level of a 0 dB
    /// station of amplitude 0.3.
    pub fn silence(secs: f64, seed: u64) -> Scene {
        let mut sc = Scene::new(8000, Vec::new(), 0.0, seed);
        sc.tail = secs;
        sc
    }

    /// Length in seconds.
    pub fn secs(&self) -> f64 {
        self.stations.iter().map(|s| s.end()).fold(0.0, f64::max) + self.tail
    }

    pub fn render(&self) -> Vec<f64> {
        let true_rate = self.rate as f64 * (1.0 + self.rate_ppm * 1e-6);
        let n = (self.secs() * true_rate) as usize;
        let mut x = vec![0.0; n];
        for s in &self.stations {
            s.render(&mut x, true_rate);
        }
        let mut rng = Rng::new(self.seed);
        for q in &self.qrm {
            let ph = 2.0 * PI * rng.uniform();
            for (i, v) in x.iter_mut().enumerate() {
                *v += q.amp * (2.0 * PI * q.freq * i as f64 / true_rate + ph).sin();
            }
        }
        if self.buzz_hz > 0.0 {
            for (i, v) in x.iter_mut().enumerate() {
                let t = i as f64 / true_rate;
                let mut k = 1.0;
                while k * self.buzz_hz < 3000.0 {
                    *v += self.buzz_amp / k * (2.0 * PI * k * self.buzz_hz * t).sin();
                    k += 2.0;
                }
            }
        }
        if self.impulse_hz > 0.0 {
            let p = self.impulse_hz / true_rate;
            let decay = (-1.0 / (0.0005 * true_rate)).exp();
            let mut s = 0.0;
            for v in x.iter_mut() {
                if rng.uniform() < p {
                    s = self.impulse_amp * if rng.uniform() < 0.5 { -1.0 } else { 1.0 };
                }
                *v += s * rng.normal();
                s *= decay;
            }
        }
        if self.crash_hz > 0.0 {
            let p = self.crash_hz / true_rate;
            let decay = (-1.0 / (CRASH_SEC * true_rate)).exp();
            let mut s = 0.0;
            for v in x.iter_mut() {
                if rng.uniform() < p {
                    s = self.crash_amp * (0.3 + rng.uniform());
                }
                *v += s * rng.normal();
                s *= decay;
            }
        }
        if self.snr_db.is_finite() {
            let amp = self.stations.first().map(|s| s.amp).unwrap_or(0.3);
            let pn = amp * amp / 2.0 / 10f64.powf(self.snr_db / 10.0);
            let sigma = (pn * true_rate / 2.0 / 2500.0).sqrt();
            let half = x.len() / 2;
            for (i, v) in x.iter_mut().enumerate() {
                let t = i as f64 / true_rate;
                let mut db = 0.5 * self.noise_wander * (2.0 * PI * WANDER_HZ * t).sin();
                if i >= half {
                    db += self.noise_step;
                }
                *v += sigma * 10f64.powf(db / 20.0) * rng.normal();
            }
        }
        if self.agc > 0.0 {
            let up = (-1.0 / (AGC_ATTACK * true_rate)).exp();
            let down = (-1.0 / (self.agc * true_rate)).exp();
            let mut e: f64 = 0.0;
            for v in x.iter_mut() {
                let a = v.abs();
                e = if a > e {
                    up * e + (1.0 - up) * a
                } else {
                    down * e
                };
                *v *= AGC_TARGET / e.max(AGC_FLOOR);
            }
        }
        x
    }
}

fn plain_callsign(rng: &mut Rng) -> String {
    let pre = [
        "JA", "JH", "JR", "JE", "JF", "JG", "JK", "JL", "JM", "JN", "JO", "JP",
    ];
    let mut s = pre[(rng.uniform() * pre.len() as f64) as usize].to_string();
    s.push(char::from(b'0' + (rng.uniform() * 10.0) as u8));
    for _ in 0..3 {
        s.push(char::from(b'A' + (rng.uniform() * 26.0) as u8));
    }
    s
}

/// A Japanese callsign such as JA1ZLO, sometimes portable (JA1ZLO/3), or a DX one.
pub fn callsign(rng: &mut Rng) -> String {
    let pick = |rng: &mut Rng, v: &[&'static str]| v[(rng.uniform() * v.len() as f64) as usize];
    let letter = |rng: &mut Rng| char::from(b'A' + (rng.uniform() * 26.0) as u8);
    let digit = |rng: &mut Rng| char::from(b'0' + (rng.uniform() * 10.0) as u8);
    let r = rng.uniform();
    let mut s = String::new();
    if r < 0.8 {
        s.push_str(pick(
            rng,
            &[
                "JA", "JH", "JR", "JE", "JF", "JG", "JK", "JL", "JM", "JN", "JO", "JP", "JQ", "JS",
            ],
        ));
        s.push(digit(rng));
        for _ in 0..if rng.uniform() < 0.8 { 3 } else { 2 } {
            s.push(letter(rng));
        }
        if rng.uniform() < 0.1 {
            s.push('/');
            s.push(digit(rng));
        }
    } else {
        s.push_str(pick(
            rng,
            &[
                "K", "W", "N", "DL", "G", "F", "UA", "VK", "BY", "HL", "BV", "YB",
            ],
        ));
        s.push(digit(rng));
        for _ in 0..if rng.uniform() < 0.5 { 2 } else { 3 } {
            s.push(letter(rng));
        }
    }
    s
}

/// A contest exchange: a report and a serial, prefecture, power or zone.
pub fn exchange(rng: &mut Rng) -> String {
    let rst = if rng.uniform() < 0.8 { "5NN" } else { "599" };
    let n = (rng.uniform() * 4.0) as u8;
    let rest = match n {
        0 => format!("{:02}M", 10 + (rng.uniform() * 40.0) as u32),
        1 => format!("{}H", 1000 + (rng.uniform() * 9000.0) as u32),
        2 => format!("{:02}", 1 + (rng.uniform() * 40.0) as u32),
        _ => format!("{}", 1 + (rng.uniform() * 999.0) as u32),
    };
    format!("{rst} {rest}")
}

/// Committed text per track: (track, freq, text), whitespace collapsed.
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

/// Edit distance of `got` from `want` (spaces ignored), over the length of `want`.
pub fn cer(got: &str, want: &str) -> f64 {
    let a: Vec<char> = got.chars().filter(|c| *c != ' ').collect();
    let b: Vec<char> = want.chars().filter(|c| *c != ' ').collect();
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

/// As [`cer`], against the best-matching stretch of `got` (text before and after it is free).
pub fn cer_within(got: &str, want: &str) -> f64 {
    let a: Vec<char> = got.chars().filter(|c| *c != ' ').collect();
    let b: Vec<char> = want.chars().filter(|c| *c != ' ').collect();
    let mut d: Vec<usize> = (0..=b.len()).collect();
    let mut best = d[b.len()];
    for i in 1..=a.len() {
        let mut prev = d[0];
        d[0] = 0;
        for j in 1..=b.len() {
            let cur = d[j];
            let c = usize::from(a[i - 1] != b[j - 1]);
            d[j] = (d[j] + 1).min(d[j - 1] + 1).min(prev + c);
            prev = cur;
        }
        best = best.min(d[b.len()]);
    }
    best as f64 / b.len().max(1) as f64
}

/// Decodes the whole of `x` with the end-of-stream flush.
pub fn decode(x: &[f64], rate: u32, cfg: Config) -> Vec<Event> {
    let mut d = Decoder::with_config(rate, cfg);
    let mut ev = Vec::new();
    for c in x.chunks(512) {
        ev.extend(d.push_f64(c));
    }
    ev.extend(d.flush());
    ev
}

/// Result of decoding one station of a scene.
#[derive(Clone, Debug)]
pub struct Score {
    pub want: String,
    pub got: String,
    pub cer: f64,
}

/// Decodes a scene and scores each station against the track nearest its pitch
/// (within 25 Hz plus its drift); a station without a track scores 1.
pub fn score(scene: &Scene, cfg: Config) -> Vec<Score> {
    let x = scene.render();
    let tr = texts(&decode(&x, scene.rate, cfg));
    scene
        .stations
        .iter()
        .map(|s| {
            let reach = 25.0 + (s.drift * s.end()).abs();
            let got = tr
                .iter()
                .filter(|t| (t.1 - s.freq).abs() < reach)
                .map(|t| t.2.clone())
                .min_by(|a, b| cer(a, &s.text).total_cmp(&cer(b, &s.text)))
                .unwrap_or_default();
            Score {
                cer: cer(&got, &s.text).min(1.0),
                want: s.text.clone(),
                got,
            }
        })
        .collect()
}

/// Contest reading of one station: callsign and exchange shown correctly, character
/// error rate, inserted T and E, and seconds from the end of the callsign to its display.
#[derive(Clone, Debug)]
pub struct Reading {
    pub want: String,
    pub got: String,
    pub call_ok: bool,
    pub exchange_ok: bool,
    pub cer: f64,
    pub te_inserted: usize,
    pub latency: Option<f64>,
}

/// Characters of `got` that an alignment with `want` inserts (spaces ignored).
pub fn insertions(got: &str, want: &str) -> Vec<char> {
    let a: Vec<char> = got.chars().filter(|c| *c != ' ').collect();
    let b: Vec<char> = want.chars().filter(|c| *c != ' ').collect();
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for i in 0..=n {
        d[i][0] = i;
    }
    for j in 0..=m {
        d[0][j] = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let c = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + c);
        }
    }
    let (mut i, mut j, mut out) = (n, m, Vec::new());
    while i > 0 {
        if j > 0 && d[i][j] == d[i - 1][j - 1] + usize::from(a[i - 1] != b[j - 1]) {
            i -= 1;
            j -= 1;
        } else if d[i][j] == d[i - 1][j] + 1 {
            out.push(a[i - 1]);
            i -= 1;
        } else {
            j -= 1;
        }
    }
    out
}

/// Seconds after a transmission within which its text counts as read.
const READ_AFTER: f64 = 3.0;

/// Track number, pitch and timed commits.
type TrackText = (u32, f64, Vec<(f64, String)>);

/// Decodes a contest scene and reads every station from the track nearest its pitch, using
/// the text committed from its start until `READ_AFTER` after its end.
pub fn read_contest(scene: &Scene, cfg: Config) -> Vec<Reading> {
    let ev = decode(&scene.render(), scene.rate, cfg);
    let mut tracks: Vec<TrackText> = Vec::new();
    let mut tokens: Vec<(u32, f64, String)> = Vec::new();
    for e in &ev {
        if let Event::Token {
            track,
            sample,
            text,
            ..
        } = e
        {
            tokens.push((*track, *sample as f64 / scene.rate as f64, text.clone()));
        }
        if let Event::Commit {
            track,
            sample,
            freq,
            text,
            ..
        } = e
        {
            let t = *sample as f64 / scene.rate as f64;
            match tracks.iter_mut().find(|x| x.0 == *track) {
                Some(x) => {
                    x.1 = *freq;
                    x.2.push((t, text.clone()));
                }
                None => tracks.push((*track, *freq, vec![(t, text.clone())])),
            }
        }
    }
    let squash = |s: &str| s.chars().filter(|c| *c != ' ').collect::<String>();
    scene
        .stations
        .iter()
        .map(|s| {
            let (t0, t1) = (s.start, s.end() + READ_AFTER);
            let full = |x: &TrackText| {
                let t: String =
                    x.2.iter()
                        .filter(|c| c.0 >= t0 && c.0 <= t1)
                        .map(|c| c.1.as_str())
                        .collect();
                t.split_whitespace().collect::<Vec<_>>().join(" ")
            };
            let best = tracks
                .iter()
                .filter(|x| (x.1 - s.freq).abs() < 25.0 + (s.drift * s.end()).abs())
                .min_by(|a, b| cer(&full(a), &s.text).total_cmp(&cer(&full(b), &s.text)));
            let got = best.map(full).unwrap_or_default();
            let mut words = s.text.split_whitespace();
            let call = words.next().unwrap_or("").to_string();
            let exch = squash(&words.collect::<Vec<_>>().join(""));
            let marks: usize = call.chars().map(|c| code(c).len()).sum();
            let call_end = s.intervals().get(marks.max(1) - 1).map_or(0.0, |x| x.1);
            let latency = best.and_then(|x| {
                let mut acc = String::new();
                x.2.iter().filter(|c| c.0 >= t0).find_map(|(t, txt)| {
                    acc.push_str(txt);
                    acc.split_whitespace()
                        .any(|w| w == call)
                        .then_some(t - call_end)
                })
            });
            let te_inserted = insertions(&got, &s.text)
                .iter()
                .filter(|c| matches!(c, 'T' | 'E'))
                .count();
            let token_call = best.is_some_and(|x| {
                tokens
                    .iter()
                    .any(|k| k.0 == x.0 && k.1 >= t0 && k.1 <= t1 + READ_AFTER && k.2 == call)
            });
            Reading {
                call_ok: token_call || got.split_whitespace().any(|w| w == call),
                exchange_ok: !exch.is_empty() && squash(&got).contains(&exch),
                cer: cer(&got, &s.text).min(1.0),
                te_inserted,
                latency,
                want: s.text.clone(),
                got,
            }
        })
        .collect()
}
