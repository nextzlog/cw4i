//! A streaming multi-signal Morse (CW) decoder.
//!
//! Feed mono audio of any sample rate in blocks of any size with [`Decoder::push`];
//! the decoder returns [`Event`]s that do not depend on how the audio is split.
//! The core is pure computation (no I/O, no threads) and builds for WebAssembly.

#![allow(clippy::needless_range_loop)]

mod calib;
mod complex;
mod confirm;
mod confirm_weights;
mod decoder;
mod fast;
mod fft;
mod frontend;
mod hsmm;
mod keyer;
mod model;
mod nn;
mod nn_weights;
pub mod sim;
mod special;
mod syntax;
mod timing;

pub use model::{Alt, Guess};
pub use syntax::{TokenKind, Verdict};

use decoder::Core;
use std::collections::BTreeMap;

/// One keyed mark or space with its duration in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Element {
    pub mark: bool,
    pub d: f64,
}

/// Decoder parameters.
#[derive(Clone, Debug)]
pub struct Config {
    /// Analysis window length in seconds.
    pub win_sec: f64,
    /// Lowest decoded audio frequency in Hz.
    pub lo_hz: f64,
    /// Highest decoded audio frequency in Hz.
    pub hi_hz: f64,
    /// Smoothed residual SNR (dB) that starts a track candidate.
    pub birth_db: f64,
    /// Frames a candidate must persist.
    pub birth_run: i64,
    /// Frames decoded retroactively at birth.
    pub lookback: usize,
    /// Silence in seconds before a confirmed track is dropped.
    pub dead_sec: f64,
    /// Tracks closer than this (Hz) are merged.
    pub min_sep_hz: f64,
    /// Informative characters required before a track may be confirmed.
    pub conf_chars: i64,
    /// Windowed log-likelihood ratio (Morse versus noise) required to confirm.
    pub ev_conf: f64,
    /// Windowed log-likelihood ratio below which held text is discarded.
    pub ev_alive: f64,
    /// Evidence window in frames.
    pub ev_win: usize,
    /// Fraction of an element's mean duration spent in forced stages of the soft keyer.
    pub k_frac: f64,
    /// Likelihood tempering of the soft keyer for overlapping frames.
    pub kappa: f64,
    /// Emit Tentative events (costs some CPU per track and frame).
    pub tentative: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            win_sec: 0.040,
            lo_hz: 250.0,
            hi_hz: 1300.0,
            birth_db: 9.0,
            birth_run: 4,
            lookback: 160,
            dead_sec: 6.0,
            min_sep_hz: 12.0,
            conf_chars: 2,
            ev_conf: 8.0,
            ev_alive: 0.0,
            ev_win: 300,
            k_frac: 0.6,
            kappa: 0.35,
            tentative: true,
        }
    }
}

/// One committed character.
#[derive(Clone, Debug, PartialEq)]
pub struct Char {
    pub ch: char,
    /// Calibrated probability that the character is right.
    pub p: f64,
    /// Forced best guess (the posterior stayed below the staged commit threshold).
    pub doubtful: bool,
    /// Up to three alternatives (including the chosen character).
    pub alts: Vec<Alt>,
}

/// Decoder event; `sample` is the absolute sample index at the end of the analysis frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A new track; `candidate` is true until it is confirmed as a station.
    TrackBorn {
        track: u32,
        sample: u64,
        freq: f64,
        candidate: bool,
    },
    /// The candidate became a station; its look-back text is committed in a Commit of the same frame.
    TrackConfirmed { track: u32, sample: u64, freq: f64 },
    /// Confirmed characters (including spaces); never revised.
    Commit {
        track: u32,
        sample: u64,
        freq: f64,
        text: String,
        chars: Vec<Char>,
    },
    /// Uncommitted state of a track, emitted when it changes.
    Tentative {
        track: u32,
        sample: u64,
        /// Text a candidate has decoded but holds until confirmation.
        held: String,
        /// Uncommitted text of the most probable hypothesis group.
        text: String,
        /// Open dot/dash pattern.
        pattern: String,
        /// Character of the open pattern if it ended now.
        guess: String,
        /// Calibrated probability of the most probable group.
        p: f64,
        /// Next hypothesis groups.
        alts: Vec<Guess>,
        candidate: bool,
    },
    /// Retraction of displayed characters (reserved; the current core never revises).
    Correction {
        track: u32,
        sample: u64,
        retract: u32,
        text: String,
    },
    /// A token of a track's committed text closed by a space or by the end of the track,
    /// with its class and verdict; `start` indexes the track's committed text.
    Token {
        track: u32,
        sample: u64,
        start: u32,
        text: String,
        kind: TokenKind,
        verdict: Verdict,
        /// Calibrated probability that the token is right (callsigns; for other kinds the
        /// product of its character probabilities).
        p: f64,
    },
    /// The track ended; `candidate` true means a rejected candidate.
    TrackLost {
        track: u32,
        sample: u64,
        candidate: bool,
    },
}

/// State of one live track.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackState {
    pub track: u32,
    pub freq: f64,
    pub candidate: bool,
    /// Key-down level over the noise level (dB).
    pub snr: f64,
    /// Smoothed key-down power (dB, arbitrary reference).
    pub level: f64,
    pub wpm: f64,
    /// Learned dah/dit ratio.
    pub ratio: f64,
    /// Learned mark lengthening in units.
    pub bias: f64,
    pub key_on: bool,
    /// Windowed log-likelihood ratio of Morse versus noise.
    pub evidence: f64,
    /// Held text of a candidate.
    pub held: Vec<Char>,
    /// Calibrated probability that the track is a station.
    pub p_station: f64,
    /// Top hypothesis groups of the uncommitted text.
    pub guesses: Vec<Guess>,
}

struct Known {
    confirmed: bool,
    tent: String,
    reader: syntax::Reader,
    /// Station probability at the last commit.
    p_station: f64,
}

/// The streaming decoder.
pub struct Decoder {
    core: Core,
    sample: u64,
    tracks: BTreeMap<u32, Known>,
    ev: Vec<Event>,
}

/// Station features of a track.
fn station_features(c: &Core, t: &decoder::Track) -> [f32; calib::NS] {
    let ev =
        t.sk.as_ref()
            .map(|s| s.evidence(c.cfg.ev_win))
            .unwrap_or(0.0);
    calib::station_features(
        ev,
        t.tim.c.snr_db,
        t.tim.c.llr,
        t.tim.c.nemit,
        t.tim.nchars(),
        t.confirmed,
        t.tim.c.outshone,
        t.tim.c.guarded,
        (c.frame_no - t.born) as f64 * c.frame_seconds(),
    )
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}
fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}
fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

impl Decoder {
    /// Creates a decoder with the default configuration.
    pub fn new(rate: u32) -> Decoder {
        Decoder::with_config(rate, Config::default())
    }

    pub fn with_config(rate: u32, cfg: Config) -> Decoder {
        assert!(rate >= 4000, "sample rate must be at least 4000 Hz");
        Decoder {
            core: Core::new(rate as usize, cfg),
            sample: 0,
            tracks: BTreeMap::new(),
            ev: Vec::new(),
        }
    }

    /// Consumes audio and returns the events of all frames completed by it.
    pub fn push(&mut self, x: &[f32]) -> Vec<Event> {
        self.push_iter(x.iter().map(|&v| v as f64))
    }

    /// As [`Decoder::push`] for f64 samples.
    pub fn push_f64(&mut self, x: &[f64]) -> Vec<Event> {
        self.push_iter(x.iter().copied())
    }

    fn push_iter(&mut self, x: impl Iterator<Item = f64>) -> Vec<Event> {
        self.core.feed(x);
        while let Some(end) = self.core.next_frame() {
            self.on_frame(end as u64);
        }
        std::mem::take(&mut self.ev)
    }

    /// Finalises all tracks (end of a recording).
    pub fn flush(&mut self) -> Vec<Event> {
        self.core.out.clear();
        self.core.flush();
        let outs = std::mem::take(&mut self.core.out);
        for o in outs {
            self.commit(o, self.sample);
        }
        let tracks = std::mem::take(&mut self.tracks);
        for (id, mut k) in tracks {
            let toks = k.reader.finish();
            self.push_tokens(id, self.sample, toks);
            self.ev.push(Event::TrackLost {
                track: id,
                sample: self.sample,
                candidate: !k.confirmed,
            });
        }
        std::mem::take(&mut self.ev)
    }

    fn on_frame(&mut self, end: u64) {
        self.sample = end;
        let core = &mut self.core;
        let mut order: Vec<usize> = (0..core.tracks.len()).collect();
        order.sort_by(|&a, &b| {
            core.hz(core.tracks[a].nu)
                .partial_cmp(&core.hz(core.tracks[b].nu))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for &i in &order {
            let t = &core.tracks[i];
            if let std::collections::btree_map::Entry::Vacant(v) = self.tracks.entry(t.id) {
                v.insert(Known {
                    confirmed: t.confirmed,
                    tent: String::new(),
                    reader: syntax::Reader::default(),
                    p_station: 0.5,
                });
                self.ev.push(Event::TrackBorn {
                    track: t.id,
                    sample: end,
                    freq: round1(core.hz(t.nu)),
                    candidate: !t.confirmed,
                });
            }
        }
        let outs = std::mem::take(&mut core.out);
        for o in outs {
            self.commit(o, end);
        }
        let core = &self.core;
        let tentative = core.cfg.tentative;
        for &i in &order {
            let t = &core.tracks[i];
            let freq = core.hz(t.nu);
            let k = self.tracks.get_mut(&t.id).unwrap();
            if t.confirmed && !k.confirmed {
                k.confirmed = true;
                self.ev.push(Event::TrackConfirmed {
                    track: t.id,
                    sample: end,
                    freq: round1(freq),
                });
            }
            if !tentative {
                continue;
            }
            let held: String = t.tim.held().iter().collect();
            let gs = calib::guesses(&t.tim.guesses(4), |p| t.tim.c.guess_features(p));
            let (mut text, mut pattern, mut guess, mut p, mut alts) =
                (String::new(), String::new(), String::new(), 0.0, Vec::new());
            if let Some(g) = gs.first() {
                text = g.text.clone();
                pattern = g.pattern.clone();
                guess = g.ch.clone();
                p = round3(g.p);
                alts = gs[1..].to_vec();
            }
            let key = format!("{held}|{text}|{pattern}|{guess}");
            if key != k.tent {
                k.tent = key;
                self.ev.push(Event::Tentative {
                    track: t.id,
                    sample: end,
                    held,
                    text,
                    pattern,
                    guess,
                    p,
                    alts,
                    candidate: !t.confirmed,
                });
            }
        }
        let live: Vec<u32> = core.tracks.iter().map(|t| t.id).collect();
        let lost: Vec<u32> = self
            .tracks
            .keys()
            .copied()
            .filter(|id| !live.contains(id))
            .collect();
        for id in lost {
            let mut k = self.tracks.remove(&id).unwrap();
            let toks = k.reader.finish();
            self.push_tokens(id, end, toks);
            self.ev.push(Event::TrackLost {
                track: id,
                sample: end,
                candidate: !k.confirmed,
            });
        }
    }

    fn commit(&mut self, o: decoder::Out, sample: u64) {
        let chars = o
            .text
            .chars()
            .enumerate()
            .map(|(i, ch)| match o.meta.get(i) {
                Some(m) => calib::character(m, ch),
                None => Char {
                    ch,
                    p: 1.0,
                    doubtful: false,
                    alts: Vec::new(),
                },
            })
            .collect::<Vec<Char>>();
        let core_track = self.core.tracks.iter().find(|t| t.id == o.track);
        let dropped = core_track.is_some_and(|t| t.tim.c.first_dropped);
        let ps = core_track.map(|t| calib::station_p(&station_features(&self.core, t)));
        let toks = match self.tracks.get_mut(&o.track) {
            Some(k) => {
                if dropped {
                    k.reader.truncate_first();
                }
                if let Some(p) = ps {
                    k.p_station = p;
                }
                k.reader
                    .feed_p(chars.iter().map(|c| (c.ch, c.doubtful, c.p)))
            }
            None => Vec::new(),
        };
        self.ev.push(Event::Commit {
            track: o.track,
            sample,
            freq: round1(o.freq),
            text: o.text,
            chars,
        });
        self.push_tokens(o.track, sample, toks);
    }

    fn push_tokens(&mut self, track: u32, sample: u64, toks: Vec<syntax::Token>) {
        let ps = self.tracks.get(&track).map(|k| k.p_station).unwrap_or(0.5);
        for t in toks {
            let p = if t.kind == TokenKind::Callsign {
                let valid = matches!(
                    t.verdict,
                    Verdict::Confirmed | Verdict::Repeated | Verdict::DoubtfulCharacter
                );
                let f = calib::token_features(t.min_lp, t.sum_lp, valid, t.repeat, ps);
                calib::token_p(&f)
            } else {
                t.sum_lp.exp()
            };
            self.ev.push(Event::Token {
                track,
                sample,
                start: t.start as u32,
                text: t.text,
                kind: t.kind,
                verdict: t.verdict,
                p: round3(p),
            });
        }
    }

    /// Live tracks sorted by frequency.
    pub fn snapshot(&self) -> Vec<TrackState> {
        let c = &self.core;
        let mut v: Vec<TrackState> = c
            .tracks
            .iter()
            .map(|t| {
                let mut s = TrackState {
                    track: t.id,
                    freq: c.hz(t.nu),
                    candidate: !t.confirmed,
                    snr: 0.0,
                    level: 0.0,
                    wpm: 0.0,
                    ratio: 0.0,
                    bias: 0.0,
                    key_on: t.key.on,
                    evidence: 0.0,
                    held: Vec::new(),
                    guesses: calib::guesses(&t.tim.guesses(4), |p| t.tim.c.guess_features(p)),
                    p_station: round3(calib::station_p(&station_features(c, t))),
                };
                if t.key.noise > 0.0 && t.key.peak > 0.0 {
                    s.snr = round1(10.0 * (t.key.peak / t.key.noise).log10());
                }
                if t.level > 0.0 {
                    s.level = round1(10.0 * t.level.log10());
                }
                let best = t.tim.best();
                if best.u > 0.0 {
                    s.wpm = round1(model::PARIS / best.u);
                }
                if t.tim.initialized() {
                    let m = t.tim.means();
                    let b = t.tim.learned_b();
                    s.ratio = round2(m.dah - b);
                    s.bias = round2(b);
                }
                if let Some(sk) = &t.sk {
                    s.evidence = round1(sk.evidence(c.cfg.ev_win));
                }
                let meta = t.tim.held_meta();
                s.held = t
                    .tim
                    .held()
                    .iter()
                    .enumerate()
                    .map(|(i, &ch)| match meta.get(i) {
                        Some(m) => Char {
                            alts: Vec::new(),
                            ..calib::character(m, ch)
                        },
                        None => Char {
                            ch,
                            p: 1.0,
                            doubtful: false,
                            alts: Vec::new(),
                        },
                    })
                    .collect();
                s
            })
            .collect();
        v.sort_by(|a, b| {
            a.freq
                .partial_cmp(&b.freq)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for s in &mut v {
            s.freq = round1(s.freq);
        }
        v
    }

    /// Number of analysis frames processed so far.
    pub fn frame_count(&self) -> i64 {
        self.core.frame_no
    }

    /// Records the confirmation features of candidate tracks (track, sample index, Hz,
    /// features); while recording, candidates are confirmed without the classifier (for
    /// training; per thread).
    #[doc(hidden)]
    pub fn record_confirmations(&mut self, on: bool) {
        decoder::CONF_LOG.with(|l| *l.borrow_mut() = if on { Some(Vec::new()) } else { None });
    }
    #[doc(hidden)]
    pub fn take_confirmations(&mut self) -> Vec<(u32, i64, f64, Vec<f32>)> {
        decoder::CONF_LOG.with(|l| {
            l.borrow_mut()
                .as_mut()
                .map(std::mem::take)
                .unwrap_or_default()
        })
    }

    /// Starts recording the keyer output of all tracks (for diagnostics and feedback traces).
    pub fn record_elements(&mut self, on: bool) {
        self.core.elements = if on { Some(Vec::new()) } else { None };
    }

    /// Takes the recorded keyer output: (track, frame, element), in decoding order.
    pub fn take_elements(&mut self) -> Vec<(u32, i64, Element)> {
        self.core
            .elements
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// Frame hop in seconds (5 ms).
    pub fn frame_seconds(&self) -> f64 {
        self.core.frame_seconds()
    }
}
