use std::collections::BTreeMap;

use cw4i::{Event, TokenKind, Verdict};

const CHARS_KEPT: usize = 400;

const WORDS_KEPT: usize = 16;

#[derive(Clone, Debug, PartialEq)]
pub struct Letter {
    pub ch: char,
    pub doubtful: bool,
    pub suspect: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordKind {
    Callsign,
    Exchange,
    Procedure,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub kind: WordKind,
    pub sure: bool,
    pub p: f64,
}

#[derive(Clone, Debug)]
pub struct Station {
    pub id: u32,
    pub freq: f64,
    pub confirmed: bool,
    pub alive: bool,
    pub letters: Vec<Letter>,
    base: usize,
    pub pending: String,
    pub words: Vec<Word>,
    pub wpm: f64,
    pub snr: f64,
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

    pub fn text(&self) -> String {
        self.letters.iter().map(|l| l.ch).collect()
    }

    pub fn callsign(&self) -> Option<&str> {
        self.words
            .iter()
            .rev()
            .find(|w| w.kind == WordKind::Callsign && w.sure)
            .map(|w| w.text.as_str())
    }
}

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

    pub fn refresh(&mut self, decoder: &cw4i::Decoder) {
        for t in decoder.snapshot() {
            if let Some(s) = self.map.get_mut(&t.track) {
                s.wpm = t.wpm;
                s.snr = t.snr;
                s.freq = t.freq;
            }
        }
    }

    pub fn sorted(&self) -> Vec<&Station> {
        let mut v: Vec<&Station> = self.map.values().collect();
        v.sort_by(|a, b| a.freq.total_cmp(&b.freq));
        v
    }

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
