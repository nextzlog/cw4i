//! Stations heard, built from the events of the rumo decoder.

use std::collections::BTreeMap;

use rumo::{Event, TokenKind, Verdict};

/// How many committed characters a station keeps.
const CHARS_KEPT: usize = 400;

/// How many recognised words a station keeps.
const WORDS_KEPT: usize = 16;

/// One committed character and how sure the decoder is of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Letter {
    pub ch: char,
    /// The character itself is a forced guess.
    pub doubtful: bool,
    /// The character belongs to a word that failed its callsign or exchange check.
    pub suspect: bool,
}

/// Kind of a recognised word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordKind {
    Callsign,
    Exchange,
    Procedure,
    Other,
}

/// A word of committed text with its class and verdict.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub kind: WordKind,
    /// The word passed its checks and may be answered.
    pub sure: bool,
    /// Calibrated probability that the word is right.
    pub p: f64,
}

/// One station heard.
#[derive(Clone, Debug)]
pub struct Station {
    pub id: u32,
    pub freq: f64,
    /// The track was shown to be a real station.
    pub confirmed: bool,
    /// The station is still on the air.
    pub alive: bool,
    pub letters: Vec<Letter>,
    /// Index in the decoder's committed text of `letters[0]`.
    base: usize,
    /// Uncommitted best guess, shown grey.
    pub pending: String,
    pub words: Vec<Word>,
    pub wpm: f64,
    pub snr: f64,
    /// Sample index of the last event.
    pub last: u64,
}

impl Station {
    fn new(id: u32, freq: f64, confirmed: bool) -> Self {
        Station {
            id,
            freq,
            confirmed,
            alive: true,
            letters: Vec::new(),
            base: 0,
            pending: String::new(),
            words: Vec::new(),
            wpm: 0.0,
            snr: 0.0,
            last: 0,
        }
    }

    /// Committed text.
    pub fn text(&self) -> String {
        self.letters.iter().map(|l| l.ch).collect()
    }

    /// The last callsign that passed its checks.
    pub fn callsign(&self) -> Option<&str> {
        self.words
            .iter()
            .rev()
            .find(|w| w.kind == WordKind::Callsign && w.sure)
            .map(|w| w.text.as_str())
    }
}

/// Every station heard, keyed by the decoder's track number.
#[derive(Default)]
pub struct Stations {
    map: BTreeMap<u32, Station>,
}

impl Stations {
    pub fn apply(&mut self, event: Event) {
        match event {
            Event::TrackBorn {
                track,
                sample,
                freq,
                candidate,
            } => {
                let s = self
                    .map
                    .entry(track)
                    .or_insert_with(|| Station::new(track, freq, !candidate));
                s.freq = freq;
                s.last = sample;
            }
            Event::TrackConfirmed {
                track,
                sample,
                freq,
            } => {
                let s = self
                    .map
                    .entry(track)
                    .or_insert_with(|| Station::new(track, freq, true));
                s.confirmed = true;
                s.freq = freq;
                s.last = sample;
            }
            Event::Commit {
                track,
                sample,
                freq,
                chars,
                ..
            } => {
                let s = self
                    .map
                    .entry(track)
                    .or_insert_with(|| Station::new(track, freq, true));
                s.letters.extend(chars.iter().map(|c| Letter {
                    ch: c.ch,
                    doubtful: c.doubtful,
                    suspect: false,
                }));
                if s.letters.len() > CHARS_KEPT {
                    let n = s.letters.len() - CHARS_KEPT;
                    s.letters.drain(..n);
                    s.base += n;
                }
                s.freq = freq;
                s.last = sample;
                s.pending.clear();
            }
            Event::Tentative {
                track,
                sample,
                held,
                text,
                guess,
                ..
            } => {
                if let Some(s) = self.map.get_mut(&track) {
                    s.pending = format!("{held}{text}{guess}");
                    s.last = sample;
                }
            }
            Event::Token {
                track,
                start,
                text,
                kind,
                verdict,
                p,
                ..
            } => {
                if let Some(s) = self.map.get_mut(&track) {
                    let sure = matches!(verdict, Verdict::Confirmed | Verdict::Repeated);
                    let n = text.chars().count();
                    let from = (start as usize).saturating_sub(s.base);
                    for l in s.letters.iter_mut().skip(from).take(n) {
                        l.suspect = !sure;
                    }
                    let kind = match kind {
                        TokenKind::Callsign => WordKind::Callsign,
                        TokenKind::Exchange => WordKind::Exchange,
                        TokenKind::Procedure => WordKind::Procedure,
                        _ => WordKind::Other,
                    };
                    s.words.retain(|w| w.text != text || w.kind != kind);
                    s.words.push(Word {
                        text,
                        kind,
                        sure,
                        p,
                    });
                    if s.words.len() > WORDS_KEPT {
                        let n = s.words.len() - WORDS_KEPT;
                        s.words.drain(..n);
                    }
                }
            }
            Event::TrackLost {
                track, candidate, ..
            } => {
                if candidate {
                    self.map.remove(&track);
                } else if let Some(s) = self.map.get_mut(&track) {
                    s.alive = false;
                    s.pending.clear();
                }
            }
            _ => {}
        }
    }

    /// Update speed and SNR from the decoder's track states.
    pub fn refresh(&mut self, decoder: &rumo::Decoder) {
        for t in decoder.snapshot() {
            if let Some(s) = self.map.get_mut(&t.track) {
                s.wpm = t.wpm;
                s.snr = t.snr;
                s.freq = t.freq;
            }
        }
    }

    /// Stations sorted by frequency.
    pub fn sorted(&self) -> Vec<&Station> {
        let mut v: Vec<&Station> = self.map.values().collect();
        v.sort_by(|a, b| a.freq.total_cmp(&b.freq));
        v
    }

    /// Drop stations off the air since before `sample`.
    pub fn forget_before(&mut self, sample: u64) {
        self.map.retain(|_, s| s.alive || s.last >= sample);
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}
