//! Silence, weak signals, strong neighbours and conditions of real recordings.

use cw4i::sim::{decode, score, texts, Scene, Station, Tone};
use cw4i::{Config, Event};

const TEXTS: [&str; 2] = ["CQ TEST DE JA1ZLO JA1ZLO K", "5NN 1317M TU"];

fn false_chars(sc: &Scene) -> usize {
    texts(&decode(&sc.render(), sc.rate, Config::default()))
        .iter()
        .map(|t| t.2.replace(' ', "").len())
        .sum()
}

#[test]
fn silence_has_no_text() {
    let cases: [fn(&mut Scene); 8] = [
        |_| {},
        |sc| {
            sc.crash_hz = 1.0;
            sc.crash_amp = 1.0;
        },
        |sc| {
            sc.impulse_hz = 10.0;
            sc.impulse_amp = 1.0;
        },
        |sc| {
            sc.buzz_hz = 100.0;
            sc.buzz_amp = 0.1;
        },
        |sc| {
            sc.qrm.push(Tone {
                freq: 700.0,
                amp: 0.3,
            })
        },
        |sc| {
            sc.crash_hz = 1.0;
            sc.crash_amp = 1.0;
            sc.agc = 0.3;
        },
        |sc| sc.noise_wander = 10.0,
        |sc| sc.noise_step = 10.0,
    ];
    for (i, f) in cases.iter().enumerate() {
        let mut sc = Scene::silence(30.0, 7 + i as u64);
        f(&mut sc);
        assert_eq!(false_chars(&sc), 0, "background {i}");
    }
}

#[test]
fn gaps_between_transmissions_stay_quiet() {
    let mut sts = Vec::new();
    let mut start = 1.0;
    for k in 0..2 {
        let mut st = Station::new("CQ TEST DE JA1ZLO K", 700.0, 25.0);
        st.start = start;
        st.seed = k + 1;
        start = st.end() + 15.0;
        sts.push(st);
    }
    let ends: Vec<(f64, f64)> = sts.iter().map(|s| (s.start, s.end())).collect();
    let mut sc = Scene::new(8000, sts, 10.0, 3);
    sc.tail = 15.0;
    for e in decode(&sc.render(), sc.rate, Config::default()) {
        if let Event::Commit { sample, text, .. } = e {
            let t = sample as f64 / 8000.0;
            let (_, end) = ends.iter().rev().find(|w| w.0 <= t).copied().unwrap();
            assert!(
                text.trim().is_empty() || t < end + 2.0,
                "{text} at {t:.1} s"
            );
        }
    }
}

fn weak_cer(snr: f64) -> f64 {
    let mut c = 0.0;
    for text in TEXTS {
        c += score(&Scene::single(text, 25.0, snr, 1), Config::default())[0].cer;
    }
    c / TEXTS.len() as f64
}

#[test]
fn weak_station_at_minus_3_db() {
    let c = weak_cer(-3.0);
    assert!(c <= 0.05, "CER {c}");
}

#[test]
fn weak_station_at_minus_6_db() {
    let c = weak_cer(-6.0);
    assert!(c <= 0.2, "CER {c}");
}

fn below_cer(below: f64, off: f64) -> f64 {
    let mut c = 0.0;
    for text in TEXTS {
        let mut sc = Scene::single(text, 25.0, 10.0, 1);
        let mut st = Station::new("TEST DE JA1ZLO JA1ZLO TEST TEST CQ TEST", 700.0 + off, 27.0);
        st.amp = sc.stations[0].amp * 10f64.powf(below / 20.0);
        st.start = 0.2;
        st.seed = 0x33;
        sc.stations.push(st);
        c += score(&sc, Config::default())[0].cer;
    }
    c / TEXTS.len() as f64
}

#[test]
fn weak_station_40_db_below_200_hz_away() {
    let c = below_cer(40.0, 200.0);
    assert!(c <= 0.15, "CER {c}");
}

#[test]
fn weak_station_20_db_below_100_hz_away() {
    let c = below_cer(20.0, 100.0);
    assert!(c <= 0.15, "CER {c}");
}

#[test]
fn conditions_of_the_recordings() {
    let mut c = 0.0;
    let mut n = 0.0;
    for seed in 1..=4 {
        for nb in [false, true] {
            c += score(&Scene::realistic(TEXTS[0], nb, seed), Config::default())[0].cer;
            n += 1.0;
        }
    }
    assert!(c / n <= 0.3, "CER {}", c / n);
}
