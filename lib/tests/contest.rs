//! Contest callers and pile-ups read as a beginner would use them.

use cw4i::sim::{read_contest, Scene};
use cw4i::Config;

/// Key-down SNR in 2500 Hz of an average power of 5 dB in 500 Hz.
const SNR: f64 = 1.0;

#[test]
fn single_callers_25_to_45_wpm_are_exact_and_quick() {
    for wpm in [25.0, 35.0, 45.0] {
        for seed in 1..=2 {
            let sc = Scene::contest(1, 0.0, (wpm, wpm), SNR, seed);
            let r = &read_contest(&sc, Config::default())[0];
            assert!(
                r.call_ok && r.exchange_ok,
                "{wpm} WPM: {} | {}",
                r.want,
                r.got
            );
            assert_eq!(r.cer, 0.0, "{} | {}", r.want, r.got);
            assert_eq!(r.te_inserted, 0);
            let char_time = 12.0 / wpm;
            assert!(
                r.latency.is_some_and(|l| l < 1.2 * char_time),
                "{:?}",
                r.latency
            );
        }
    }
}

#[test]
fn pileup_callsigns() {
    let (mut ok, mut n) = (0, 0);
    for seed in 1..=3 {
        let sc = Scene::contest(4, 300.0, (30.0, 40.0), SNR + 5.0, seed);
        for r in read_contest(&sc, Config::default()) {
            ok += usize::from(r.call_ok);
            n += 1;
        }
    }
    assert!(ok as f64 >= 0.7 * n as f64, "{ok} of {n}");
}

#[test]
fn two_callers_both_read() {
    for seed in 1..=4 {
        let sc = Scene::contest(2, 300.0, (30.0, 40.0), SNR + 5.0, seed);
        for r in read_contest(&sc, Config::default()) {
            assert!(r.call_ok, "{} | {}", r.want, r.got);
        }
    }
}

#[test]
fn operating_scene_has_no_garbage_in_gaps() {
    let mut sc = Scene::operating(40.0, 0.5, SNR + 10.0, 2);
    sc.agc = 0.0;
    sc.crash_hz = 0.0;
    let busy: Vec<(f64, f64)> = sc.stations.iter().map(|s| (s.start, s.end())).collect();
    for e in cw4i::sim::decode(&sc.render(), sc.rate, Config::default()) {
        if let cw4i::Event::Commit { sample, text, .. } = e {
            let t = sample as f64 / sc.rate as f64;
            let inside = busy.iter().any(|b| t > b.0 - 0.5 && t < b.1 + 2.0);
            assert!(inside || text.trim().is_empty(), "{text} at {t:.1} s");
        }
    }
}
