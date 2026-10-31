//! Decoding tests on synthetic signals generated in the tests.

mod common;
use common::*;
use cw4i::{Config, Event, TokenKind, Verdict};

const CALL: &str = "CQ TEST DE JA1ZLO JA1ZLO K";

fn single(text: &str, wpm: f64, snr: f64, seed: u64) -> (Station, Vec<f64>) {
    let mut st = Station::new(text, 700.0, wpm);
    st.seed = seed;
    let secs = st.end() + 3.0;
    let x = scene(std::slice::from_ref(&st), secs, snr, seed);
    (st, x)
}

fn check_clean(wpm: f64) {
    let (_, x) = single(CALL, wpm, 20.0, 7);
    let t = texts(&decode(&x, 400, Config::default()));
    assert_eq!(t.len(), 1, "{wpm} WPM: {t:?}");
    assert_eq!(t[0].2, CALL, "{wpm} WPM");
    assert!((t[0].1 - 700.0).abs() < 3.0, "frequency {}", t[0].1);
}

#[test]
fn clean_15_wpm() {
    check_clean(15.0);
}

#[test]
fn clean_25_wpm() {
    check_clean(25.0);
}

#[test]
fn clean_35_wpm() {
    check_clean(35.0);
}

#[test]
fn clean_45_wpm() {
    check_clean(45.0);
}

#[test]
fn noiseless_signal_decodes() {
    let (_, x) = single("TEST DE JA1ZLO", 25.0, f64::INFINITY, 3);
    let t = texts(&decode(&x, 1000, Config::default()));
    assert_eq!(t.len(), 1, "{t:?}");
    assert_eq!(t[0].2, "TEST DE JA1ZLO");
}

#[test]
fn period_and_comma_decode() {
    let (st, x) = single("TEST, DE JA1ZLO.", 22.0, 20.0, 18);
    let t = texts(&decode(&x, 400, Config::default()));
    assert_eq!(t.len(), 1, "{t:?}");
    assert_eq!(t[0].2, st.text);
}

#[test]
fn noise_only_has_no_commits() {
    for seed in 1..=3 {
        let x = scene(&[], 30.0, 0.0, seed);
        let ev = decode(&x, 400, Config::default());
        assert_eq!(n_committed(&ev), 0, "seed {seed}: {:?}", texts(&ev));
        assert!(!ev.iter().any(|e| matches!(e, Event::TrackConfirmed { .. })));
    }
}

#[test]
fn louder_noise_only_has_no_commits() {
    // noise of a 0 dB SNR scene at ten times the amplitude
    let mut x = scene(&[], 20.0, 0.0, 11);
    for v in &mut x {
        *v *= 10.0;
    }
    assert_eq!(n_committed(&decode(&x, 128, Config::default())), 0);
}

#[test]
fn no_commits_in_trailing_noise() {
    let mut st = Station::new("CQ TEST DE JA1ZLO K", 700.0, 25.0);
    st.seed = 8;
    let end = st.end();
    let x = scene(std::slice::from_ref(&st), end + 30.0, 5.0, 8);
    let ev = decode(&x, 400, Config::default());
    let late = (end + 2.0) * RATE as f64;
    for e in &ev {
        if let Event::Commit { sample, text, .. } = e {
            if (*sample as f64) > late {
                assert!(text.trim().is_empty(), "late commit {text:?} at {sample}");
            }
        }
    }
    let t = texts(&ev);
    assert_eq!(t.len(), 1, "{t:?}");
    assert!(cer(&t[0].2, "CQ TEST DE JA1ZLO K") <= 0.1, "{t:?}");
}

#[test]
fn block_size_independence() {
    let mut a = Station::new("CQ TEST DE JA1ZLO K", 700.0, 25.0);
    a.jitter = 0.05;
    let mut b = Station::new("JH1ABC 5NN 1317M TU", 820.0, 22.0);
    b.start = 1.1;
    b.seed = 5;
    let secs = a.end().max(b.end()) + 2.0;
    let x = scene(&[a, b], secs, 12.0, 21);
    let cfg = Config::default();
    let e128 = decode_flush(&x, 128, cfg.clone());
    let e2400 = decode_flush(&x, 2400, cfg.clone());
    assert!(n_committed(&e128) > 10);
    assert_eq!(e128, e2400, "128 vs 2400 samples");
    // random block sizes, including empty and single-sample blocks
    let mut rng = Rng::new(99);
    let mut d = cw4i::Decoder::with_config(RATE, cfg);
    let mut er = Vec::new();
    let mut i = 0;
    while i < x.len() {
        let n = match rng.next_u64() % 10 {
            0 => 0,
            1 => 1,
            _ => (rng.next_u64() % 5000) as usize,
        };
        let j = (i + n).min(x.len());
        er.extend(d.push_f64(&x[i..j]));
        i = j;
    }
    er.extend(d.flush());
    assert_eq!(e128, er, "128 vs random blocks");
}

#[test]
fn f32_and_f64_input_agree_on_text() {
    let (_, x) = single("TEST DE JA1ZLO", 28.0, 15.0, 4);
    let xf: Vec<f32> = x.iter().map(|&v| v as f32).collect();
    let mut d = cw4i::Decoder::new(RATE);
    let mut ev = Vec::new();
    for c in xf.chunks(333) {
        ev.extend(d.push(c));
    }
    let a = texts(&ev);
    let b = texts(&decode(&x, 333, Config::default()));
    assert_eq!(a.len(), b.len());
    for (p, q) in a.iter().zip(b.iter()) {
        assert_eq!(p.2, q.2);
    }
}

#[test]
fn mid_stream_start_decodes_the_rest() {
    let (_, x) = single("CQ TEST DE JA1ZLO K", 25.0, 15.0, 9);
    // start inside the first character (C spans 0.50-1.03 s)
    let cut = (0.75 * RATE as f64) as usize;
    let t = texts(&decode(&x[cut..], 400, Config::default()));
    assert_eq!(t.len(), 1, "{t:?}");
    let got: String = t[0].2.chars().filter(|c| *c != ' ').collect();
    assert!(got.ends_with("QTESTDEJA1ZLOK"), "{got}");
    assert!(got.len() <= "CQTESTDEJA1ZLOK".len());
}

#[test]
fn mid_stream_start_commits_no_wrong_character() {
    let (_, x) = single("CQ TEST DE JA1ZLO K", 25.0, 15.0, 9);
    let cut = (0.75 * RATE as f64) as usize;
    let t = texts(&decode(&x[cut..], 400, Config::default()));
    let got: String = t[0].2.chars().filter(|c| *c != ' ').collect();
    assert!("CQTESTDEJA1ZLOK".contains(&got), "{got}");
}

#[test]
fn mid_stream_start_in_a_gap() {
    let st = Station::new("CQ TEST DE JA1ZLO K", 700.0, 25.0);
    let iv = st.intervals();
    // start in the word gap after "CQ" (the fourth mark ends C, the next four are Q)
    let gap = (iv[7].1 + iv[8].0) / 2.0;
    let x = scene(std::slice::from_ref(&st), st.end() + 3.0, 15.0, 10);
    let t = texts(&decode(
        &x[(gap * RATE as f64) as usize..],
        400,
        Config::default(),
    ));
    assert_eq!(t.len(), 1, "{t:?}");
    assert_eq!(t[0].2, "TEST DE JA1ZLO K");
}

#[test]
fn two_stations_100_hz_apart() {
    let a = Station::new("CQ TEST DE JA1ZLO K", 700.0, 25.0);
    let mut b = Station::new("JH1ABC 5NN 1317M TU", 800.0, 22.0);
    b.seed = 2;
    b.start = 0.9;
    let secs = a.end().max(b.end()) + 3.0;
    let x = scene(&[a.clone(), b.clone()], secs, 15.0, 3);
    let t = texts(&decode(&x, 400, Config::default()));
    assert_eq!(t.len(), 2, "{t:?}");
    let near = |f: f64| t.iter().find(|x| (x.1 - f).abs() < 10.0).unwrap();
    assert!(cer(&near(700.0).2, &a.text) <= 0.05, "{t:?}");
    assert!(cer(&near(800.0).2, &b.text) <= 0.05, "{t:?}");
}

#[test]
fn overlapping_stations_at_different_speeds() {
    let mut a = Station::new("TEST JA1ZLO", 650.0, 18.0);
    a.seed = 31;
    let mut b = Station::new("5NN 1317M", 760.0, 32.0);
    b.seed = 32;
    b.start = 0.8;
    let secs = a.end().max(b.end()) + 3.0;
    let x = scene(&[a.clone(), b.clone()], secs, 18.0, 33);
    let t = texts(&decode(&x, 400, Config::default()));
    assert_eq!(t.len(), 2, "{t:?}");
}

#[test]
fn weak_station_next_to_strong_one_has_no_ghosts() {
    let a = Station::new("CQ TEST DE JA1ZLO K", 700.0, 25.0);
    let mut w = Station::new("JH1ABC 5NN 1317M TU", 900.0, 22.0);
    w.seed = 2;
    w.start = 0.9;
    w.amp = 0.03; // 20 dB weaker
    let secs = a.end().max(w.end()) + 3.0;
    let x = scene(&[a.clone(), w.clone()], secs, 25.0, 4);
    let t = texts(&decode(&x, 400, Config::default()));
    assert!(t.len() <= 2, "ghost tracks: {t:?}");
    for (_, f, s) in &t {
        let ok = ((f - 700.0).abs() < 10.0 && cer(s, &a.text) <= 0.1)
            || ((f - 900.0).abs() < 10.0 && cer(s, &w.text) <= 0.2);
        assert!(ok, "unexpected track {f} Hz {s:?}");
    }
    assert!(t.iter().any(|x| (x.1 - 700.0).abs() < 10.0));
}

#[test]
fn strong_station_has_no_skirt_ghosts() {
    let mut a = Station::new("CQ TEST DE JA1ZLO JA1ZLO K", 700.0, 30.0);
    a.amp = 0.8;
    let x = scene(std::slice::from_ref(&a), a.end() + 3.0, 40.0, 12);
    let t = texts(&decode(&x, 400, Config::default()));
    assert_eq!(t.len(), 1, "{t:?}");
    assert_eq!(t[0].2, a.text);
}

#[test]
fn periodic_fading_and_jitter() {
    let mut f = Station::new("CQ TEST DE JA1ZLO K", 700.0, 25.0);
    f.qsb_depth = 0.6;
    f.qsb_hz = 0.3;
    f.jitter = 0.1;
    let x = scene(std::slice::from_ref(&f), f.end() + 3.0, 15.0, 5);
    let t = texts(&decode(&x, 400, Config::default()));
    assert_eq!(t.len(), 1, "{t:?}");
    assert!(cer(&t[0].2, &f.text) <= 0.2, "{t:?}");
}

#[test]
fn rayleigh_fading_and_jitter() {
    let mut r = Station::new("CQ TEST DE JA1ZLO K", 700.0, 25.0);
    r.fade_hz = 0.3;
    r.jitter = 0.1;
    let x = scene(std::slice::from_ref(&r), r.end() + 3.0, 15.0, 6);
    let t = texts(&decode(&x, 400, Config::default()));
    assert_eq!(t.len(), 1, "{t:?}");
    assert!(cer(&t[0].2, &r.text) <= 0.25, "{t:?}");
}

#[test]
fn jitter_20_percent() {
    let mut s = Station::new("CQ TEST DE JA1ZLO K", 700.0, 22.0);
    s.jitter = 0.2;
    s.seed = 13;
    let x = scene(std::slice::from_ref(&s), s.end() + 3.0, 20.0, 13);
    let t = texts(&decode(&x, 400, Config::default()));
    assert!(!t.is_empty());
    assert!(cer(&t[0].2, &s.text) <= 0.3, "{t:?}");
}

#[test]
fn moderate_noise_decodes() {
    let (st, x) = single(CALL, 25.0, 5.0, 14);
    let t = texts(&decode(&x, 400, Config::default()));
    assert_eq!(t.len(), 1, "{t:?}");
    assert!(cer(&t[0].2, &st.text) <= 0.1, "{t:?}");
}

#[test]
fn speed_change_within_transmission() {
    let mut a = Station::new("CQ TEST DE", 700.0, 18.0);
    a.seed = 15;
    let mut b = Station::new("JA1ZLO JA1ZLO K", 700.0, 32.0);
    b.seed = 15;
    b.start = a.end() + 0.6;
    let x = scene(&[a.clone(), b.clone()], b.end() + 3.0, 20.0, 15);
    let t = texts(&decode(&x, 400, Config::default()));
    assert_eq!(t.len(), 1, "{t:?}");
    assert!(cer(&t[0].2, "CQ TEST DE JA1ZLO JA1ZLO K") <= 0.1, "{t:?}");
}

#[test]
fn unconfirmed_short_text_is_held() {
    // track confirmation needs two informative characters (three or more elements with
    // dots and dashes); "TEST 5NN" has none, so nothing is committed while streaming
    let (_, x) = single("TEST 5NN", 25.0, 30.0, 3);
    let mut d = cw4i::Decoder::new(RATE);
    let mut ev = Vec::new();
    let mut held = String::new();
    for c in x.chunks(400) {
        ev.extend(d.push_f64(c));
        let h: String = d
            .snapshot()
            .iter()
            .flat_map(|s| s.held.iter().map(|c| c.ch))
            .collect();
        if h.len() > held.len() {
            held = h;
        }
    }
    assert_eq!(n_committed(&ev), 0);
    assert!(held.contains("TEST"), "{held:?}");
}

#[test]
fn other_sample_rates() {
    for rate in [11025u32, 16000] {
        let st = Station::new("TEST JA1ZLO", 700.0, 25.0);
        let n = ((st.end() + 3.0) * rate as f64) as usize;
        let x: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 / rate as f64;
                let on = st.intervals().iter().any(|&(a, b)| t >= a && t < b);
                if on {
                    0.3 * (2.0 * std::f64::consts::PI * 700.0 * t).sin()
                } else {
                    0.0
                }
            })
            .collect();
        let mut d = cw4i::Decoder::new(rate);
        let mut ev = Vec::new();
        for c in x.chunks(512) {
            ev.extend(d.push_f64(c));
        }
        let t = texts(&ev);
        assert_eq!(t.len(), 1, "{rate} Hz: {t:?}");
        assert_eq!(t[0].2, "TEST JA1ZLO", "{rate} Hz");
    }
}

#[test]
fn jitter_30_percent_keeps_characters_whole() {
    // intra-character gaps as long as character gaps must not split characters
    let mut sum = 0.0;
    for seed in 1..=6 {
        let mut s = Station::new("CQ TEST DE JA1ZLO K", 700.0, 22.0);
        s.jitter = 0.3;
        s.seed = 20 + seed;
        let x = scene(std::slice::from_ref(&s), s.end() + 3.0, 20.0, 20 + seed);
        let t = texts(&decode(&x, 400, Config::default()));
        sum += t.first().map_or(1.0, |t| cer(&t.2, &s.text));
    }
    assert!(sum / 6.0 <= 0.35, "mean CER {}", sum / 6.0);
}

#[test]
fn weak_station_next_to_strong_one_holds_doubtful_characters() {
    for seed in 1..=4 {
        let a = Station::new("CQ TEST DE JA1ZLO JA1ZLO K", 700.0, 25.0);
        let mut w = Station::new("JH1ABC 5NN 1317M TU", 800.0, 21.0);
        w.seed = 30 + seed;
        w.start = 0.9;
        w.amp = 0.03; // 20 dB weaker
        let secs = a.end().max(w.end()) + 3.0;
        let x = scene(&[a.clone(), w.clone()], secs, 25.0, 30 + seed);
        for e in decode(&x, 400, Config::default()) {
            if let Event::Commit { freq, chars, .. } = e {
                if (freq - 800.0).abs() < 30.0 {
                    assert!(chars.iter().all(|c| !c.doubtful), "seed {seed}: {chars:?}");
                }
            }
        }
    }
}

#[test]
fn tokens_of_a_clean_call_are_confirmed() {
    let (_, x) = single("CQ TEST DE JA1ZLO JA1ZLO K", 25.0, 15.0, 41);
    let toks: Vec<(String, TokenKind, Verdict)> = decode(&x, 400, Config::default())
        .into_iter()
        .filter_map(|e| match e {
            Event::Token {
                text,
                kind,
                verdict,
                ..
            } => Some((text, kind, verdict)),
            _ => None,
        })
        .collect();
    assert!(
        toks.iter()
            .filter(|t| t.0 == "JA1ZLO")
            .all(|t| t.1 == TokenKind::Callsign && t.2.confirmed()),
        "{toks:?}"
    );
    assert!(toks.iter().any(|t| t.0 == "JA1ZLO"), "{toks:?}");
    assert!(toks
        .iter()
        .any(|t| t.0 == "DE" && t.1 == TokenKind::Procedure));
}

/// Mean CER of a station 20 dB below a strong one at `df` Hz, its committed characters,
/// and how many of them are wrong (over the seeds).
fn weak_beside_strong(df: f64, seeds: std::ops::RangeInclusive<u64>) -> (f64, usize, usize) {
    let (mut sum, mut n, mut chars, mut wrong) = (0.0, 0.0, 0, 0);
    for seed in seeds {
        let a = Station::new("CQ TEST DE JA1ZLO JA1ZLO K", 700.0, 27.0);
        let mut w = Station::new("JH1ABC 5NN 1317M TU", 700.0 + df, 22.0);
        w.seed = 40 + seed;
        w.start = 0.9;
        w.amp = 0.03; // 20 dB weaker
        let secs = a.end().max(w.end()) + 3.0;
        let x = scene(&[a.clone(), w.clone()], secs, 25.0, 40 + seed);
        let t = texts(&decode(&x, 400, Config::default()));
        let got: String = t
            .iter()
            .filter(|x| (x.1 - w.freq).abs() < 15.0)
            .map(|x| x.2.clone())
            .collect();
        sum += cer(&got, &w.text);
        n += 1.0;
        let g: Vec<char> = got.chars().filter(|c| *c != ' ').collect();
        chars += g.len();
        wrong += g.len().saturating_sub(lcs(
            &g,
            &w.text.replace(' ', "").chars().collect::<Vec<_>>(),
        ));
    }
    (sum / n, chars, wrong)
}

/// Length of the longest common subsequence.
fn lcs(a: &[char], b: &[char]) -> usize {
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in 0..a.len() {
        for j in 0..b.len() {
            d[i + 1][j + 1] = if a[i] == b[j] {
                d[i][j] + 1
            } else {
                d[i][j + 1].max(d[i + 1][j])
            };
        }
    }
    d[a.len()][b.len()]
}

#[test]
fn weak_station_80_hz_from_strong_one_is_read_through_its_edges() {
    let (c, chars, wrong) = weak_beside_strong(80.0, 1..=4);
    assert!(c <= 0.1, "mean CER {c}");
    assert!(
        wrong * 20 <= chars,
        "{wrong} of {chars} committed characters wrong"
    );
}

#[test]
fn weak_station_25_hz_away_is_tracked_at_its_frequency() {
    let mut hits = 0;
    for seed in 1..=4 {
        let a = Station::new("CQ TEST DE JA1ZLO JA1ZLO K", 700.0, 25.0);
        let mut w = Station::new("JH1ABC 5NN 1317M TU", 725.0, 21.0);
        w.seed = 50 + seed;
        w.start = 0.9;
        w.amp = 0.3 * 10f64.powf(-0.5); // 10 dB weaker
        let secs = a.end().max(w.end()) + 3.0;
        let x = scene(&[a.clone(), w.clone()], secs, 15.0, 50 + seed);
        let born: Vec<f64> = decode(&x, 400, Config::default())
            .iter()
            .filter_map(|e| match e {
                Event::TrackBorn { freq, .. } if (freq - 700.0).abs() > 12.0 => Some(*freq),
                _ => None,
            })
            .collect();
        if born.iter().any(|f| (f - 725.0).abs() < 8.0) {
            hits += 1;
        }
    }
    assert!(hits >= 3, "{hits} of 4 seeds");
}

#[test]
fn first_character_after_a_clean_start_completes_a_callsign() {
    // the recording starts 20 ms before the first mark; O alone could be the tail of J,
    // but OO1ZQG/1 is a callsign and O1ZQG/1 is none
    let mut st = Station::new("OO1ZQG/1", 700.0, 22.0);
    st.start = 0.02;
    let x = scene(std::slice::from_ref(&st), st.end() + 3.0, 25.0, 9);
    let ev = decode_flush(&x, 400, Config::default());
    let t = texts(&ev);
    assert_eq!(t.len(), 1, "{t:?}");
    assert_eq!(t[0].2.trim(), "OO1ZQG/1");
    // the character decided by the syntax is doubtful, so the callsign is not confirmed
    for e in &ev {
        if let Event::Token { text, verdict, .. } = e {
            assert!(!verdict.confirmed(), "{text}: {verdict:?}");
        }
    }
}
