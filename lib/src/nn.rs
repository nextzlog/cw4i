//! Key-state network: a small causal GRU that reads seven features of a track per frame
//! and returns the calibrated log-odds that the key is down.
//!
//! Architecture: inputs are normalised by fixed means and deviations, a single GRU layer
//! of 24 units (PyTorch gate convention: reset, update, candidate) updates its state every
//! 5 ms frame, and a linear output divided by a temperature gives the logit.
//! The features are the track's log power over its own noise reference, its log power
//! over the residual floor of its bin, its phase-advance consistency, its log power below
//! its recent peak, and the log power, frequency offset and phase consistency of the
//! strongest neighbour within 300 Hz.

use crate::nn_weights as w;

/// Number of input features.
pub const INPUTS: usize = 7;
/// Number of GRU units.
pub const HIDDEN: usize = 24;

/// Recurrent state and feature trackers of one track.
#[derive(Clone, Debug)]
pub struct NnState {
    h: [f32; HIDDEN],
    /// Peak follower of the log power.
    pub pk: f64,
    /// Low-quantile tracker of the log power (noise reference).
    pub nf: f64,
    /// Last log power over the noise reference (for neighbour features).
    pub la: f64,
    pub init: bool,
    /// Frames fed to the network.
    pub age: u32,
}

impl Default for NnState {
    fn default() -> Self {
        NnState {
            h: [0.0; HIDDEN],
            pk: 0.0,
            nf: 0.0,
            la: 0.0,
            init: false,
            age: 0,
        }
    }
}

#[inline]
fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-(x as f64)).exp() as f32)
}

/// Advances the GRU by one frame and returns the calibrated logit of P(key down).
pub fn step(st: &mut NnState, f: &[f32; INPUTS]) -> f64 {
    let mut x = [0f32; INPUTS];
    for i in 0..INPUTS {
        x[i] = (f[i] - w::MEAN[i]) / w::STD[i];
    }
    let mut gi = [0f32; 3 * HIDDEN];
    let mut gh = [0f32; 3 * HIDDEN];
    for g in 0..3 * HIDDEN {
        let mut s = w::B_IH[g];
        let row = &w::W_IH[g * INPUTS..(g + 1) * INPUTS];
        for i in 0..INPUTS {
            s += row[i] * x[i];
        }
        gi[g] = s;
        let mut s = w::B_HH[g];
        let row = &w::W_HH[g * HIDDEN..(g + 1) * HIDDEN];
        for i in 0..HIDDEN {
            s += row[i] * st.h[i];
        }
        gh[g] = s;
    }
    for j in 0..HIDDEN {
        let r = sigmoid(gi[j] + gh[j]);
        let z = sigmoid(gi[HIDDEN + j] + gh[HIDDEN + j]);
        let n = ((gi[2 * HIDDEN + j] + r * gh[2 * HIDDEN + j]) as f64).tanh() as f32;
        st.h[j] = (1.0 - z) * n + z * st.h[j];
    }
    let mut o = w::B_OUT;
    for j in 0..HIDDEN {
        o += w::W_OUT[j] * st.h[j];
    }
    (o / w::TEMPERATURE) as f64
}

/// Logit of the prior key-down probability.
pub fn prior_logit() -> f64 {
    w::PRIOR_LOGIT as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Logits of a fixed feature sequence, computed independently with the same weights.
    #[allow(clippy::excessive_precision)]
    const VECTORS: [([f32; INPUTS], f64); 12] = [
        (
            [9.0, 12.0, 1.8185948, -0.5, -1.513605, -1.9178486, -0.558831],
            10.361294746398926,
        ),
        (
            [
                9.0, 12.0, 0.824237, -0.5, -1.9999804, -1.0731459, 0.84033406,
            ],
            11.412612915039062,
        ),
        (
            [
                9.0, 12.0, -0.5758066, -0.5, -1.5019745, 0.2997544, 1.8258905,
            ],
            3.5310328006744385,
        ),
        (
            [
                0.3, 1.5, -1.6924409, -11.0, -0.2647035, 1.5251169, 1.9127519,
            ],
            0.9016862511634827,
        ),
        (
            [0.3, 1.5, -1.9760633, -11.0, 1.1028534, 1.9998237, 1.0581654],
            0.431060791015625,
        ),
        (
            [
                0.3,
                1.5,
                -1.2870762,
                -11.0,
                1.9275907,
                1.4902263,
                -0.31724533,
            ],
            -0.13068678975105286,
        ),
        (
            [
                9.0, 12.0, 0.03540385, -0.5, 1.8035767, 0.24714625, -1.5365093,
            ],
            0.10586696863174438,
        ),
        (
            [
                9.0, 12.0, 1.3404584, -0.5, 0.7918503, -1.1175781, -1.9995103,
            ],
            1.954030156135559,
        ),
        (
            [
                9.0, 12.0, 1.9857453, -0.5, -0.6096212, -1.9322356, -1.4783614,
            ],
            4.527341365814209,
        ),
        (
            [
                0.3,
                1.5,
                1.6536573,
                -11.0,
                -1.7110399,
                -1.7958554,
                -0.22956963,
            ],
            1.4623438119888306,
        ),
        (
            [
                0.3, 1.5, 0.50764674, -11.0, -1.9702926, -0.7755633, 1.1322153,
            ],
            -0.5112970471382141,
        ),
        (
            [0.3, 1.5, -0.8882253, -11.0, -1.259776, 0.6264576, 1.936729],
            -2.3071422576904297,
        ),
    ];

    #[test]
    fn forward_pass_matches_fixed_vectors() {
        let mut st = NnState::default();
        for (f, want) in VECTORS {
            let got = step(&mut st, &f);
            assert!(
                (got - want).abs() <= 1e-6 * want.abs().max(1.0),
                "{got} != {want}"
            );
        }
    }

    #[test]
    fn state_persists_between_frames() {
        let f = VECTORS[0].0;
        let mut a = NnState::default();
        let first = step(&mut a, &f);
        let second = step(&mut a, &f);
        assert_ne!(first, second);
        let mut b = NnState::default();
        assert_eq!(step(&mut b, &f), first);
    }

    #[test]
    fn prior_matches_the_training_data() {
        let p = 1.0 / (1.0 + (-prior_logit()).exp());
        assert!((p - 0.427).abs() < 0.001, "{p}");
    }
}
