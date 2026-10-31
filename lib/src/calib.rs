//! Calibration of reported probabilities: logistic maps fitted offline on synthetic
//! signals, applied to features the decoder already has.

/// Number of features of a committed character.
pub const NF: usize = 16;

/// Number of model inputs: the features and products of them (see `expand`).
pub const NX: usize = NF + 7;

/// Weights of the committed-character calibrator (inputs of `expand`, then the bias).
const CHAR_W: [f64; NX + 1] = [
    0.22406, -1.584292, 1.075925, 0.623467, 0.095426, -0.453642, 4.704633, -2.529764, 6.185716,
    0.535593, -0.536882, -0.327943, 1.985361, -3.445623, 1.565201, -1.650516, 0.023015, -0.301145,
    4.137061, 1.437048, -3.959869, -2.572825, 4.001581, -7.136382,
];

/// Model inputs of a feature vector: the features, the excess of the log-odds over 99 %
/// and 99.9 %, and products of SNR, timing jitter, Morse evidence and fade depth.
pub fn expand(f: &[f32; NF]) -> [f64; NX] {
    let mut x = [0.0; NX];
    for i in 0..NF {
        x[i] = f[i] as f64;
    }
    let (l, snr, scale, llr, fade) = (x[0], x[6], x[7], x[8], x[15]);
    x[NF] = (l - 4.6).max(0.0);
    x[NF + 1] = (l - 6.9).max(0.0);
    x[NF + 2] = scale * scale;
    x[NF + 3] = snr * scale;
    x[NF + 4] = llr * scale;
    x[NF + 5] = snr * snr;
    x[NF + 6] = fade * l / 10.0;
    x
}

fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

/// Log-odds of a probability, bounded.
pub fn logit(p: f64) -> f64 {
    let p = p.clamp(1e-4, 1.0 - 1e-6);
    (p / (1.0 - p)).ln()
}

/// The alternatives of a character whose probability moved from `raw` to `cal`: the
/// chosen character takes `cal`, the others share the rest in their old proportions.
pub fn alts(a: &[crate::model::Alt], ch: char, raw: f64, cal: f64) -> Vec<crate::model::Alt> {
    let k = if raw < 1.0 {
        (1.0 - cal) / (1.0 - raw)
    } else {
        0.0
    };
    a.iter()
        .map(|x| crate::model::Alt {
            ch: x.ch,
            p: ((if x.ch == ch { cal } else { x.p * k }) * 1000.0).round() / 1000.0,
        })
        .collect()
}

/// A committed or held character with its calibrated probability.
pub fn character(m: &crate::model::CharMeta, ch: char) -> crate::Char {
    if ch == ' ' {
        return crate::Char {
            ch,
            p: m.conf,
            doubtful: false,
            alts: Vec::new(),
        };
    }
    let p = char_p(&m.feat);
    crate::Char {
        ch,
        p: (p * 1000.0).round() / 1000.0,
        doubtful: m.doubt,
        alts: alts(&m.alts, ch, m.conf, p),
    }
}

/// Calibrated probabilities of the hypothesis groups of a track's uncommitted text: the
/// first group is calibrated like a committed character with the track's features, the
/// others share the rest in their old proportions.
pub fn guesses(gs: &[crate::Guess], feat: impl Fn(f64) -> [f32; NF]) -> Vec<crate::Guess> {
    let Some(g0) = gs.first() else {
        return Vec::new();
    };
    let cal = char_p(&feat(g0.p));
    let k = if g0.p < 1.0 {
        (1.0 - cal) / (1.0 - g0.p)
    } else {
        0.0
    };
    gs.iter()
        .enumerate()
        .map(|(i, g)| crate::Guess {
            p: (if i == 0 { cal } else { g.p * k } * 1000.0).round() / 1000.0,
            ..g.clone()
        })
        .collect()
}

/// Number of features of a track's station probability.
pub const NS: usize = 9;

/// Weights of the station probability (features of `station_features`, then the bias).
const STATION_W: [f64; NS + 1] = [
    5.79274, 1.984469, 6.532936, 0.839225, 0.868201, 0.821955, 0.608123, -0.523987, -2.356638,
    -2.078254,
];

/// Features of a track for its station probability: windowed keying evidence, key-down
/// SNR (dB), Morse evidence of the timing model, characters emitted, informative
/// characters, confirmed, outshone, guarded, and age (s).
#[allow(clippy::too_many_arguments)]
pub fn station_features(
    evidence: f64,
    snr_db: f64,
    llr: f64,
    emitted: i64,
    informative: i64,
    confirmed: bool,
    outshone: bool,
    guarded: bool,
    age_s: f64,
) -> [f32; NS] {
    [
        (evidence / 50.0).clamp(-3.0, 3.0),
        snr_db / 20.0,
        (llr / 20.0).clamp(-2.0, 2.0),
        (1.0 + emitted.max(0) as f64).ln() / 4.0,
        (1.0 + informative.max(0) as f64).ln() / 2.0,
        confirmed as u8 as f64,
        outshone as u8 as f64,
        guarded as u8 as f64,
        (age_s / 10.0).min(3.0),
    ]
    .map(|x| x as f32)
}

/// Probability that a track is a station.
pub fn station_p(f: &[f32; NS]) -> f64 {
    let mut z = STATION_W[NS];
    for i in 0..NS {
        z += STATION_W[i] * f[i] as f64;
    }
    sigmoid(z)
}

/// Number of features of a callsign's confidence.
pub const NT: usize = 5;

/// Weights of the callsign confidence (features of `token_features`, then the bias).
const TOKEN_W: [f64; NT + 1] = [1.429397, 2.420782, 5.661712, 4.408726, 0.109937, -4.509236];

/// Features of a callsign token: lowest and summed log calibrated character probability,
/// valid syntax, an earlier matching read on the track, and the log-odds of the track's
/// station probability.
pub fn token_features(
    min_lp: f64,
    sum_lp: f64,
    valid: bool,
    repeat: bool,
    station: f64,
) -> [f32; NT] {
    [
        min_lp.max(-10.0),
        sum_lp.max(-20.0),
        valid as u8 as f64,
        repeat as u8 as f64,
        logit(station),
    ]
    .map(|x| x as f32)
}

/// Probability that a callsign token is right.
pub fn token_p(f: &[f32; NT]) -> f64 {
    let mut z = TOKEN_W[NT];
    for i in 0..NT {
        z += TOKEN_W[i] * f[i] as f64;
    }
    sigmoid(z)
}

/// Calibrated probability of a committed character.
pub fn char_p(f: &[f32; NF]) -> f64 {
    let x = expand(f);
    let mut z = CHAR_W[NX];
    for i in 0..NX {
        z += CHAR_W[i] * x[i];
    }
    sigmoid(z)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feat(p: f64) -> [f32; NF] {
        let mut f = [0.0f32; NF];
        f[0] = logit(p) as f32;
        f[5] = 0.5;
        f[6] = 1.0;
        f[7] = 0.7;
        f[8] = 1.0;
        f[12] = 0.5;
        f[13] = 0.5;
        f
    }

    #[test]
    fn probabilities_are_bounded_and_forced_guesses_lower() {
        for p in [0.5, 0.9, 0.99, 0.999] {
            let q = char_p(&feat(p));
            assert!((0.0..=1.0).contains(&q));
            let mut f = feat(p);
            f[1] = 1.0;
            assert!(char_p(&f) < q, "{p}");
        }
    }

    #[test]
    fn alternatives_keep_their_proportions() {
        let a = [
            crate::model::Alt { ch: 'A', p: 0.9 },
            crate::model::Alt { ch: 'N', p: 0.06 },
            crate::model::Alt { ch: 'R', p: 0.04 },
        ];
        let b = alts(&a, 'A', 0.9, 0.8);
        assert_eq!(b[0].p, 0.8);
        assert!((b[1].p - 0.12).abs() < 1e-9 && (b[2].p - 0.08).abs() < 1e-9);
    }

    #[test]
    fn station_probability_grows_with_evidence() {
        let weak = station_features(0.0, 3.0, 0.0, 0, 0, false, false, false, 0.5);
        let strong = station_features(150.0, 20.0, 30.0, 20, 5, true, false, false, 10.0);
        assert!(station_p(&strong) > 0.99 && station_p(&weak) < station_p(&strong));
    }
}
