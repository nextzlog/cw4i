//! Event invariants and public API smoke tests.

mod common;
use common::*;
use cw4i::{Config, Decoder, Event};
use std::collections::HashMap;

fn busy_scene() -> Vec<f64> {
    let mut a = Station::new("CQ TEST DE JA1ZLO K", 700.0, 25.0);
    a.jitter = 0.05;
    let mut b = Station::new("JH1ABC 5NN 1317M TU", 820.0, 22.0);
    b.start = 1.1;
    b.seed = 5;
    let mut c = Station::new("TU", 1000.0, 20.0);
    c.start = 6.0;
    c.seed = 6;
    c.amp = 0.1;
    let secs = a.end().max(b.end()) + 9.0;
    scene(&[a, b, c], secs, 10.0, 23)
}

fn track_and_sample(e: &Event) -> (u32, u64) {
    match e {
        Event::TrackBorn { track, sample, .. }
        | Event::TrackConfirmed { track, sample, .. }
        | Event::Commit { track, sample, .. }
        | Event::Tentative { track, sample, .. }
        | Event::Correction { track, sample, .. }
        | Event::Token { track, sample, .. }
        | Event::TrackLost { track, sample, .. } => (*track, *sample),
    }
}

#[test]
fn event_invariants() {
    let x = busy_scene();
    let ev = decode_flush(&x, 256, Config::default());
    assert!(n_committed(&ev) > 10);
    #[derive(Default)]
    struct St {
        born: bool,
        confirmed: bool,
        lost: bool,
        candidate: bool,
        text: Vec<char>,
    }
    let mut st: HashMap<u32, St> = HashMap::new();
    let mut last = 0u64;
    let frame = 40u64; // 5 ms at 8 kHz
    for e in &ev {
        let (id, sample) = track_and_sample(e);
        assert!(sample >= last, "samples must not decrease: {e:?}");
        assert!(sample <= x.len() as u64);
        last = sample;
        let s = st.entry(id).or_default();
        assert!(!s.lost, "event after TrackLost: {e:?}");
        match e {
            Event::TrackBorn {
                candidate, freq, ..
            } => {
                assert!(!s.born, "born twice");
                s.born = true;
                s.candidate = *candidate;
                assert!((250.0..=1300.0).contains(freq));
            }
            Event::TrackConfirmed { .. } => {
                assert!(s.born && !s.confirmed);
                s.confirmed = true;
            }
            Event::Commit { text, chars, .. } => {
                assert!(s.born, "commit before TrackBorn");
                assert_eq!(text.chars().count(), chars.len());
                assert_eq!(&chars.iter().map(|c| c.ch).collect::<String>(), text);
                s.text.extend(text.chars());
                for c in chars {
                    assert!((0.0..=1.0).contains(&c.p), "{c:?}");
                    if !c.doubtful && c.ch != ' ' {
                        // the first alternative is the character itself with its calibrated p
                        assert!(
                            c.alts.is_empty()
                                || (c.alts[0].ch == c.ch && (c.alts[0].p - c.p).abs() < 1e-9),
                            "{c:?}"
                        );
                    }
                }
            }
            Event::Tentative { p, alts, .. } => {
                assert!(s.born);
                assert!((0.0..=1.0).contains(p));
                assert!(alts.len() <= 3);
            }
            Event::Correction { .. } => panic!("the core never revises"),
            Event::Token { start, text, .. } => {
                // a token is a closed stretch of the track's committed text
                let a = *start as usize;
                let got: String = s.text[a..a + text.chars().count()].iter().collect();
                assert_eq!(got.to_ascii_uppercase(), *text);
                assert!(!text.contains(' '));
            }
            Event::TrackLost { candidate, .. } => {
                assert!(s.born);
                assert_eq!(*candidate, !s.confirmed);
                s.lost = true;
            }
        }
        let _ = frame;
    }
    // after the flush every track is lost
    assert!(st.values().all(|s| s.lost));
    // commits of confirmed tracks only (the final flush also releases held text)
    let flush_at = last;
    for e in &ev {
        if let Event::Commit { track, sample, .. } = e {
            if *sample < flush_at {
                assert!(st[track].confirmed, "commit on an unconfirmed track");
            }
        }
    }
}

#[test]
fn commits_are_never_retracted() {
    let ev = decode(&busy_scene(), 512, Config::default());
    assert!(!ev.iter().any(|e| matches!(e, Event::Correction { .. })));
}

#[test]
fn tentative_can_be_disabled() {
    let x = busy_scene();
    let with = decode(&x, 512, Config::default());
    let without = decode(
        &x,
        512,
        Config {
            tentative: false,
            ..Config::default()
        },
    );
    assert!(with.iter().any(|e| matches!(e, Event::Tentative { .. })));
    assert!(!without.iter().any(|e| matches!(e, Event::Tentative { .. })));
    let strip = |v: &[Event]| -> Vec<Event> {
        v.iter()
            .filter(|e| !matches!(e, Event::Tentative { .. }))
            .cloned()
            .collect()
    };
    assert_eq!(strip(&with), without);
}

#[test]
fn snapshot() {
    let x = busy_scene();
    let mut d = Decoder::new(RATE);
    let mut saw = false;
    let mut ids_seen = std::collections::HashSet::new();
    for c in x.chunks(800) {
        for e in d.push_f64(c) {
            if let Event::TrackBorn { track, .. } = e {
                ids_seen.insert(track);
            }
        }
        let snap = d.snapshot();
        for w in snap.windows(2) {
            assert!(w[0].freq <= w[1].freq, "snapshot sorted by frequency");
        }
        for s in &snap {
            assert!(
                ids_seen.contains(&s.track),
                "snapshot track announced by TrackBorn"
            );
            assert!(s.guesses.len() <= 4);
            if s.candidate {
                assert!(s.held.iter().all(|c| (0.0..=1.0).contains(&c.p)));
            }
            if !s.candidate && s.wpm > 0.0 {
                saw = true;
                assert!(s.wpm > 5.0 && s.wpm < 110.0, "{s:?}");
                assert!(s.snr > 0.0);
                assert!(s.ratio >= 2.2 && s.ratio <= 6.5, "{s:?}");
            }
        }
    }
    assert!(saw, "a confirmed track with a speed estimate");
}

#[test]
fn api_smoke() {
    let mut d = Decoder::new(48000);
    assert!((d.frame_seconds() - 0.005).abs() < 1e-12);
    assert!(d.push(&[]).is_empty());
    assert!(d.push(&vec![0.0f32; 48000]).is_empty());
    assert_eq!(d.frame_count(), (48000 - 1920) / 240 + 1);
    assert!(d.snapshot().is_empty());
    assert!(d.flush().is_empty());
    let cfg = Config::default();
    assert_eq!(cfg.lookback, 160);
    assert!(cfg.tentative);
}

#[test]
fn element_recording() {
    let st = Station::new("TEST", 700.0, 25.0);
    let x = scene(std::slice::from_ref(&st), st.end() + 2.0, 20.0, 2);
    let mut d = Decoder::new(RATE);
    d.record_elements(true);
    for c in x.chunks(400) {
        d.push_f64(c);
    }
    let els = d.take_elements();
    let marks: Vec<f64> = els.iter().filter(|e| e.2.mark).map(|e| e.2.d).collect();
    // T E S T: 1 + 1 + 3 + 1 marks; dah 144 ms, dit 48 ms at 25 WPM
    assert!(marks.len() >= 6, "{marks:?}");
    assert!(marks.iter().any(|&d| (d - 0.144).abs() < 0.03));
    assert!(marks.iter().any(|&d| (d - 0.048).abs() < 0.02));
    assert!(d.take_elements().is_empty());
}

#[test]
fn silence_and_dc_have_no_tracks() {
    let mut d = Decoder::new(RATE);
    let mut ev = d.push_f64(&vec![0.0; 8000 * 5]);
    ev.extend(d.push_f64(&vec![0.25; 8000 * 5]));
    assert!(
        !ev.iter().any(|e| matches!(e, Event::Commit { .. })),
        "{ev:?}"
    );
}

#[test]
fn constant_carrier_is_not_decoded() {
    let x: Vec<f64> = (0..8000 * 15)
        .map(|i| 0.3 * (2.0 * std::f64::consts::PI * 700.0 * i as f64 / 8000.0).sin())
        .collect();
    let mut y = x.clone();
    add_noise(&mut y, 0.3, 20.0, 3);
    assert_eq!(n_committed(&decode(&y, 400, Config::default())), 0);
}
