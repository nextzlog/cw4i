//! Regression tests on the built-in simulator.

use cw4i::sim::{cer, score, Scene, Station};
use cw4i::Config;

const TEXT: &str = "CQ TEST DE JA1ZLO JA1ZLO K";

fn mean_cer(wpm: f64, snr: f64, seeds: u64) -> f64 {
    let mut c = 0.0;
    for seed in 1..=seeds {
        let mut sc = Scene::single(TEXT, wpm, snr, seed);
        sc.stations[0].jitter = 0.05;
        c += score(&sc, Config::default())[0].cer;
    }
    c / seeds as f64
}

#[test]
fn renders_deterministically() {
    let sc = Scene::pileup(3, 200.0, (20.0, 40.0), 10.0, 5);
    assert_eq!(sc.render(), sc.render());
    assert_ne!(
        sc.render(),
        Scene::pileup(3, 200.0, (20.0, 40.0), 10.0, 6).render()
    );
}

#[test]
fn weight_and_ratio_set_the_marks() {
    let mut st = Station::new("ET", 700.0, 20.0);
    st.weight = 0.6;
    st.ratio = 4.0;
    let iv = st.intervals();
    assert!((iv[0].1 - iv[0].0 - 0.072).abs() < 1e-9);
    assert!((iv[1].1 - iv[1].0 - 0.252).abs() < 1e-9);
    assert!((iv[1].0 - iv[0].1 - 0.168).abs() < 1e-9);
    assert_eq!(cer("JA1ZL0", "JA1ZLO"), 1.0 / 6.0);
}

#[test]
fn wpm_40_at_10_db() {
    let c = mean_cer(40.0, 10.0, 2);
    assert!(c <= 0.02, "CER {c}");
}

#[test]
fn wpm_50_at_10_db() {
    let c = mean_cer(50.0, 10.0, 2);
    assert!(c <= 0.02, "CER {c}");
}

#[test]
fn wpm_60_at_10_db() {
    let c = mean_cer(60.0, 10.0, 2);
    assert!(c <= 0.05, "CER {c}");
}

#[test]
fn wpm_80_at_20_db() {
    let c = mean_cer(80.0, 20.0, 2);
    assert!(c <= 0.1, "CER {c}");
}

#[test]
fn wpm_100_at_20_db() {
    let c = mean_cer(100.0, 20.0, 2);
    assert!(c <= 0.05, "CER {c}");
}

#[test]
fn wpm_120_at_20_db() {
    let c = mean_cer(120.0, 20.0, 2);
    assert!(c <= 0.15, "CER {c}");
}

#[test]
fn fast_station_has_no_sideband_tracks() {
    let sc = Scene::single(TEXT, 80.0, 20.0, 3);
    let ev = cw4i::sim::decode(&sc.render(), sc.rate, Config::default());
    let born = ev
        .iter()
        .filter(|e| matches!(e, cw4i::Event::TrackBorn { .. }))
        .count();
    assert_eq!(born, 1);
}

#[test]
fn fast_pileup() {
    let mut c = 0.0;
    for seed in 1..=2 {
        let s = score(
            &Scene::pileup(3, 250.0, (30.0, 45.0), 20.0, seed),
            Config::default(),
        );
        c += s.iter().map(|s| s.cer).sum::<f64>() / s.len() as f64;
    }
    assert!(c / 2.0 <= 0.15, "CER {}", c / 2.0);
}

#[test]
fn impairments_at_40_wpm() {
    let mut sc = Scene::single(TEXT, 40.0, 15.0, 4);
    sc.stations[0].qsb_depth = 0.6;
    sc.stations[0].qsb_hz = 0.3;
    sc.stations[0].drift = 1.0;
    sc.stations[0].wpm_end = 48.0;
    sc.impulse_hz = 10.0;
    sc.impulse_amp = 0.5;
    sc.rate_ppm = 300.0;
    let c = score(&sc, Config::default())[0].cer;
    assert!(c <= 0.1, "CER {c}");
}
