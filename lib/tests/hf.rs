//! HF propagation and human fists on the simulator.

use cw4i::sim::{score, Channel, Fist, Scene};
use cw4i::Config;

const TEXTS: [&str; 3] = [
    "CQ TEST DE JA1ZLO JA1ZLO K",
    "5NN 1317M TU",
    "JH3WNZ/3 R 599 0123 BK",
];

fn mean_cer(wpm: f64, snr: f64, fist: Fist, ch: Channel, seeds: u64, f: fn(&mut Scene)) -> f64 {
    let (mut c, mut n) = (0.0, 0.0);
    for seed in 1..=seeds {
        for text in TEXTS {
            let mut sc = Scene::single(text, wpm, snr, seed);
            sc.stations[0].fist = fist;
            sc.stations[0].jitter = 0.08;
            sc.stations[0].channel = ch;
            f(&mut sc);
            c += score(&sc, Config::default())[0].cer;
            n += 1.0;
        }
    }
    c / n
}

fn check(wpm: f64, snr: f64, fist: Fist, ch: Channel, max: f64) {
    let c = mean_cer(wpm, snr, fist, ch, 1, |_| {});
    assert!(c <= max, "{wpm} WPM {snr} dB {fist:?} {ch:?}: CER {c}");
}

#[test]
fn fists_without_fading() {
    check(25.0, 10.0, Fist::Straight, Channel::NONE, 0.05);
    check(25.0, 10.0, Fist::Bug, Channel::NONE, 0.05);
    check(40.0, 10.0, Fist::Paddle, Channel::NONE, 0.05);
}

#[test]
fn mid_latitude_channels() {
    check(25.0, 20.0, Fist::Straight, Channel::MID_QUIET, 0.05);
    check(25.0, 20.0, Fist::Paddle, Channel::MID_MODERATE, 0.1);
    check(25.0, 20.0, Fist::Bug, Channel::MID_DISTURBED, 0.1);
}

#[test]
fn static_crashes() {
    let c = mean_cer(25.0, 10.0, Fist::Straight, Channel::NONE, 1, |sc| {
        sc.crash_hz = 2.0;
        sc.crash_amp = 1.0;
    });
    assert!(c <= 0.1, "CER {c}");
}

#[test]
fn doppler_and_drift() {
    let c = mean_cer(25.0, 10.0, Fist::Straight, Channel::NONE, 1, |sc| {
        sc.stations[0].channel.shift = 3.0;
        sc.stations[0].drift = 2.0;
    });
    assert!(c <= 0.05, "CER {c}");
}

#[test]
fn pileup_under_moderate_fading() {
    let mut c = 0.0;
    for seed in 1..=2 {
        let mut sc = Scene::pileup(3, 250.0, (18.0, 35.0), 20.0, seed);
        for st in &mut sc.stations {
            st.channel = Channel::MID_MODERATE;
        }
        let s = score(&sc, Config::default());
        c += s.iter().map(|s| s.cer).sum::<f64>() / s.len() as f64;
    }
    assert!(c / 2.0 <= 0.3, "CER {}", c / 2.0);
}

#[test]
#[ignore]
fn sweep_channels_and_fists() {
    for (name, ch) in Channel::presets() {
        for fist in [Fist::Even, Fist::Paddle, Fist::Bug, Fist::Straight] {
            for snr in [20.0, 10.0, 5.0] {
                let c = mean_cer(25.0, snr, fist, ch, 2, |_| {});
                println!("{name} {fist:?} {snr} dB: CER {:.1} %", 100.0 * c);
                assert!(c.is_finite());
            }
        }
    }
}
