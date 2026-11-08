//! Confirmation classifier: a small network over the state of a candidate track that
//! tells a station from noise keyed like Morse. Trained offline on simulated scenes
//! (tools/train_confirm); the weights are in `confirm_weights.rs`.

use crate::confirm_weights::{B1, B2, MEAN, SD, W1, W2};

pub const NF: usize = 12;
pub const NH: usize = 8;

/// Probability at or above which a candidate is confirmed.
pub const P_CONFIRM: f32 = 0.5;

#[allow(clippy::too_many_arguments)]
pub fn features(
    ev: f64,
    snr_db: f64,
    fade_db: f64,
    ratio: f64,
    bias: f64,
    scale: f64,
    llr: f64,
    nchars: i64,
    age_sec: f64,
    wpm: f64,
    noisy: bool,
    outshone: bool,
) -> [f32; NF] {
    [
        ev.max(0.0).ln_1p() as f32,
        snr_db as f32,
        fade_db as f32,
        ratio as f32,
        bias as f32,
        scale as f32,
        llr as f32,
        (nchars as f64).ln_1p() as f32,
        age_sec.ln_1p() as f32,
        wpm.max(1.0).ln() as f32,
        f32::from(u8::from(noisy)),
        f32::from(u8::from(outshone)),
    ]
}

pub fn prob(x: &[f32; NF]) -> f32 {
    let mut out = B2;
    for j in 0..NH {
        let mut a = B1[j];
        for i in 0..NF {
            a += W1[j][i] * (x[i] - MEAN[i]) / SD[i];
        }
        out += W2[j] * a.tanh();
    }
    1.0 / (1.0 + (-out).exp())
}
