//! Benchmark of the decoder on simulated and recorded CW.
//!
//! cargo run --release --example bench [MODE] [DIR] [--seeds N]
//!
//! sweep: character error rate (CER) by WPM and SNR; pileup: CER by pile-up size;
//! hf: CER by HF channel, fist and impairment; weak: CER near and below 0 dB and next to
//! a strong station; realistic: CER under conditions measured on the recordings;
//! silence: false text on background without CW; survey: conditions of the recordings;
//! scene: minutes of a contest band with user-facing metrics; cpu: time per audio second;
//! contest: callsigns, exchanges and latency for contest callers and pile-ups;
//! export DIR: contest scenes as WAV with manifest.tsv; score DIR OUT: score OUT/<file>.txt;
//! files: CER on recordings named after their text (default ../morse_test_data).

use cw4i::sim::{cer, score, Channel, Fist, Scene, Station, Tone};
use cw4i::Config;
use std::time::Instant;

const TEXTS: [&str; 3] = [
    "CQ TEST DE JA1ZLO JA1ZLO K",
    "5NN 1317M TU",
    "JH3WNZ/3 R 599 0123 BK",
];
const WPMS: [f64; 10] = [10.0, 20.0, 30.0, 35.0, 40.0, 50.0, 60.0, 80.0, 100.0, 120.0];
const SNRS: [f64; 4] = [f64::INFINITY, 20.0, 10.0, 5.0];

fn decoder_score(scene: &Scene) -> (f64, f64) {
    let t = Instant::now();
    let sc = score(scene, Config::default());
    let rt = t.elapsed().as_secs_f64() / scene.secs();
    (sc.iter().map(|s| s.cer).sum::<f64>() / sc.len() as f64, rt)
}

fn sweep(seeds: u64) {
    println!(
        "CER (%) by WPM and SNR, {seeds} seeds x {} texts",
        TEXTS.len()
    );
    print!("{:>5}", "WPM");
    for s in SNRS {
        print!(" {:>8}", format!("{s}dB"));
    }
    println!(" {:>8}", "x RT");
    for wpm in WPMS {
        print!("{wpm:>5}");
        let mut rt = 0.0;
        for snr in SNRS {
            let (mut c, mut n) = (0.0, 0.0);
            for seed in 1..=seeds {
                for text in TEXTS {
                    let mut sc = Scene::single(text, wpm, snr, seed);
                    sc.stations[0].jitter = 0.05;
                    let (e, r) = decoder_score(&sc);
                    c += e;
                    rt += r;
                    n += 1.0;
                }
            }
            print!(" {:>8.1}", 100.0 * c / n);
        }
        println!(
            " {:>8.4}",
            rt / (seeds as f64 * (TEXTS.len() * SNRS.len()) as f64)
        );
    }
}

fn pileup(seeds: u64) {
    println!("CER (%) by pile-up size, 20 dB, stations within 250 Hz, {seeds} seeds");
    print!("{:>5}", "n");
    for w in ["18-28", "30-45"] {
        print!(" {:>8}", format!("{w}wpm"));
    }
    println!();
    for n in [1, 2, 3, 4, 6] {
        print!("{n:>5}");
        for wpm in [(18.0, 28.0), (30.0, 45.0)] {
            let mut c = 0.0;
            for seed in 1..=seeds {
                c += decoder_score(&Scene::pileup(n, 250.0, wpm, 20.0, seed)).0;
            }
            print!(" {:>8.1}", 100.0 * c / seeds as f64);
        }
        println!();
    }
}

const WINDOW_SEC: f64 = 0.04;
const QUIET_PEAK: f64 = 30.0;
const GAP_GRACE: f64 = 2.0;
const OTHER_HZ: f64 = 60.0;

const FISTS: [(&str, Fist); 4] = [
    ("even", Fist::Even),
    ("paddle", Fist::Paddle),
    ("bug", Fist::Bug),
    ("straight", Fist::Straight),
];

fn mean(scenes: impl Iterator<Item = Scene>) -> f64 {
    let (mut c, mut n) = (0.0, 0.0);
    for sc in scenes {
        c += decoder_score(&sc).0;
        n += 1.0;
    }
    100.0 * c / n
}

fn hf_scenes(
    wpm: f64,
    snr: f64,
    fist: Fist,
    seeds: u64,
    f: impl Fn(&mut Scene) + Copy,
) -> impl Iterator<Item = Scene> {
    (1..=seeds).flat_map(move |seed| {
        TEXTS.iter().map(move |text| {
            let mut sc = Scene::single(text, wpm, snr, seed);
            sc.stations[0].fist = fist;
            sc.stations[0].jitter = 0.08;
            f(&mut sc);
            sc
        })
    })
}

type Case = (&'static str, fn(&mut Scene));

fn hf(seeds: u64) {
    for wpm in [25.0, 40.0] {
        println!(
            "CER (%) by HF channel and fist, {wpm} WPM, {seeds} seeds x {} texts",
            TEXTS.len()
        );
        print!("{:<15} {:<9}", "channel", "fist");
        for snr in [20.0, 10.0, 5.0] {
            print!(" {:>6}", format!("{snr}dB"));
        }
        println!();
        for (name, ch) in Channel::presets() {
            for (fname, fist) in FISTS {
                print!("{name:<15} {fname:<9}");
                for snr in [20.0, 10.0, 5.0] {
                    let c = mean(hf_scenes(wpm, snr, fist, seeds, |sc| {
                        sc.stations[0].channel = ch
                    }));
                    print!(" {c:>6.1}");
                }
                println!();
            }
        }
    }
    println!("CER (%) by impairment, straight key, 10 dB, {seeds} seeds");
    let cases: [Case; 7] = [
        ("none", |_| {}),
        ("qrn", |sc| {
            sc.crash_hz = 2.0;
            sc.crash_amp = 1.0;
        }),
        ("deep qsb", |sc| {
            sc.stations[0].qsb_depth = 0.97;
            sc.stations[0].qsb_hz = 0.2;
        }),
        ("doppler", |sc| sc.stations[0].channel.shift = 3.0),
        ("chirp", |sc| sc.stations[0].chirp = 40.0),
        ("drift", |sc| sc.stations[0].drift = 2.0),
        ("agc pumping", |sc| {
            let mut st = Station::new("TEST TEST TEST TEST TEST TEST", 900.0, 28.0);
            st.amp = 3.0;
            st.seed = 99;
            sc.stations.push(st);
            sc.agc = 0.3;
        }),
    ];
    println!("{:<12} {:>6} {:>6}", "case", "25wpm", "40wpm");
    for (name, f) in cases {
        print!("{name:<12}");
        for wpm in [25.0, 40.0] {
            let (mut c, mut n) = (0.0, 0.0);
            for sc in hf_scenes(wpm, 10.0, Fist::Straight, seeds, f) {
                c += score(&sc, Config::default())[0].cer;
                n += 1.0;
            }
            print!(" {:>6.1}", 100.0 * c / n);
        }
        println!();
    }
    println!("CER (%) by pile-up size under mid-latitude moderate, 20 dB, {seeds} seeds");
    for n in [2, 4, 6] {
        let c = mean((1..=seeds).map(|seed| {
            let mut sc = Scene::pileup(n, 250.0, (18.0, 35.0), 20.0, seed);
            for st in &mut sc.stations {
                st.channel = Channel::MID_MODERATE;
            }
            sc
        }));
        println!("{n:>5} {c:>6.1}");
    }
}

fn first_cer(sc: &Scene) -> f64 {
    score(sc, Config::default())[0].cer
}

fn noise_garbage(seeds: u64) -> usize {
    (1..=seeds)
        .map(|seed| {
            let mut sc = Scene::single("E", 20.0, -10.0, seed);
            sc.stations[0].amp = 0.0;
            sc.tail = 20.0;
            sc.impulse_hz = 3.0;
            sc.impulse_amp = 0.3;
            let ev = cw4i::sim::decode(&sc.render(), sc.rate, Config::default());
            cw4i::sim::texts(&ev)
                .iter()
                .map(|t| t.2.replace(' ', "").len())
                .sum::<usize>()
        })
        .sum()
}

fn weak(seeds: u64) {
    println!(
        "CER (%) of a weak station by SNR in 2500 Hz, {seeds} seeds x {} texts",
        TEXTS.len()
    );
    print!("{:>5}", "WPM");
    let snrs = [0.0, -3.0, -6.0, -8.0, -10.0];
    for s in snrs {
        print!(" {:>6}", format!("{s}dB"));
    }
    println!();
    for wpm in [15.0, 20.0, 25.0, 30.0] {
        print!("{wpm:>5}");
        for snr in snrs {
            let c = mean(hf_scenes(wpm, snr, Fist::Even, seeds, |_| {}));
            print!(" {c:>6.1}");
        }
        println!();
    }
    println!(
        "characters committed on {seeds} x 20 s of noise: {}",
        noise_garbage(seeds)
    );
    println!("CER (%) of a station below a strong one, 25 WPM, weak at 10 dB, {seeds} seeds");
    print!("{:>8}", "below");
    let offs = [0.0, 50.0, 100.0, 200.0];
    for o in offs {
        print!(" {:>6}", format!("{o}Hz"));
    }
    println!();
    for below in [20.0, 30.0, 40.0] {
        print!("{:>8}", format!("{below}dB"));
        for off in offs {
            let mut c = 0.0;
            let mut n = 0.0;
            for seed in 1..=seeds {
                for text in TEXTS {
                    let mut sc = Scene::single(text, 25.0, 10.0, seed);
                    let mut st = Station::new(
                        "TEST DE JA1ZLO JA1ZLO TEST TEST CQ TEST DE JA1ZLO",
                        700.0 + off,
                        27.0,
                    );
                    st.amp = sc.stations[0].amp * 10f64.powf(below / 20.0);
                    st.start = 0.2;
                    st.seed = seed ^ 0x33;
                    sc.stations.push(st);
                    c += first_cer(&sc);
                    n += 1.0;
                }
            }
            print!(" {:>6.1}", 100.0 * c / n);
        }
        println!();
    }
}

fn realistic(seeds: u64) {
    println!(
        "CER (%) under conditions measured on the recordings, {seeds} seeds x {} texts",
        TEXTS.len()
    );
    for nb in [false, true] {
        let (mut c, mut n) = (0.0, 0.0);
        for seed in 1..=seeds {
            for text in TEXTS {
                c += first_cer(&Scene::realistic(text, nb, seed));
                n += 1.0;
            }
        }
        let label = if nb {
            "with a strong neighbour"
        } else {
            "alone"
        };
        println!("{label:<24} {:>6.1}", 100.0 * c / n);
    }
}

/// SNR of the key-down carrier in 2500 Hz for an average power (half keyed) in 500 Hz.
fn keydown_2500(avg_500: f64) -> f64 {
    avg_500 - 10.0 * (2500.0f64 / 500.0).log10() + 3.0
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(f64::NAN)
}

/// Callsigns and exchanges shown (%), CER (%), inserted T or E per station and median
/// latency of the callsign in seconds and in characters (10 units).
fn contest_row(scenes: impl Iterator<Item = Scene>) -> (f64, f64, f64, f64, f64, f64) {
    let (mut call, mut exch, mut c, mut te, mut n) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let (mut lat, mut latc) = (Vec::new(), Vec::new());
    for sc in scenes {
        for (r, st) in cw4i::sim::read_contest(&sc, Config::default())
            .iter()
            .zip(&sc.stations)
        {
            call += f64::from(u8::from(r.call_ok));
            exch += f64::from(u8::from(r.exchange_ok));
            c += r.cer;
            te += r.te_inserted as f64;
            n += 1.0;
            if let Some(l) = r.latency {
                lat.push(l);
                latc.push(l / (12.0 / st.wpm));
            }
        }
    }
    (
        100.0 * call / n,
        100.0 * exch / n,
        100.0 * c / n,
        te / n,
        median(lat),
        median(latc),
    )
}

fn contest(seeds: u64) {
    println!(
        "Single caller, paddle, mild QSB, {} seeds; SNR is the average power in 500 Hz",
        4 * seeds
    );
    println!(
        "{:>5} {:>6} {:>7} {:>7} {:>6} {:>6} {:>8} {:>7}",
        "WPM", "SNR", "call %", "exch %", "CER %", "T/E", "lat s", "lat ch"
    );
    for wpm in [25.0, 30.0, 35.0, 40.0, 45.0] {
        for snr in [5.0, 10.0, 20.0] {
            let r = contest_row(
                (1..=4 * seeds)
                    .map(|seed| Scene::contest(1, 0.0, (wpm, wpm), keydown_2500(snr), seed)),
            );
            println!(
                "{wpm:>5} {snr:>6} {:>7.1} {:>7.1} {:>6.1} {:>6.2} {:>8.2} {:>7.2}",
                r.0, r.1, r.2, r.3, r.4, r.5
            );
        }
    }
    println!(
        "Pile-ups at 30 to 40 WPM within 300 Hz, levels within 10 dB, 10 dB in 500 Hz, {} seeds",
        2 * seeds
    );
    println!(
        "{:>5} {:>7} {:>7} {:>6} {:>6} {:>8} {:>7}",
        "n", "call %", "exch %", "CER %", "T/E", "lat s", "lat ch"
    );
    for n in [2, 4, 8] {
        let r = contest_row(
            (1..=2 * seeds)
                .map(|seed| Scene::contest(n, 300.0, (30.0, 40.0), keydown_2500(10.0), seed)),
        );
        println!(
            "{n:>5} {:>7.1} {:>7.1} {:>6.1} {:>6.2} {:>8.2} {:>7.2}",
            r.0, r.1, r.2, r.3, r.4, r.5
        );
    }
}

/// Minutes of a contest band: transmissions shown correctly, latency of callsigns,
/// wrong callsigns per hour, garbage per minute of silence and callers missed.
fn scene(seeds: u64) {
    let variants: [Background; 3] = [
        ("band noise only", |sc| {
            sc.agc = 0.0;
            sc.crash_hz = 0.0;
        }),
        ("with static crashes", |sc| sc.agc = 0.0),
        ("with crashes and AGC", |_| {}),
    ];
    for (label, f) in variants {
        println!("{label}");
        scene_row(seeds, f);
    }
}

fn scene_row(seeds: u64, f: fn(&mut Scene)) {
    let secs = 120.0;
    let (mut shown, mut missed, mut n) = (0.0, 0.0, 0.0);
    let (mut wrong, mut gap_chars, mut gap_secs, mut lat) = (0, 0, 0.0f64, Vec::new());
    for seed in 1..=seeds {
        let mut sc = Scene::operating(secs, 0.5, keydown_2500(15.0), seed);
        f(&mut sc);
        let truth: Vec<String> = sc
            .stations
            .iter()
            .flat_map(|s| {
                s.text
                    .split_whitespace()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .collect();
        for (r, st) in cw4i::sim::read_contest(&sc, Config::default())
            .iter()
            .zip(&sc.stations)
        {
            if st.text.starts_with("CQ") {
                continue;
            }
            n += 1.0;
            shown += f64::from(u8::from(r.call_ok && r.exchange_ok));
            missed += f64::from(u8::from(!r.call_ok));
            if let Some(l) = r.latency {
                lat.push(l / (12.0 / st.wpm));
            }
        }
        let busy: Vec<(f64, f64, f64)> = sc
            .stations
            .iter()
            .map(|s| (s.start, s.end(), s.freq))
            .collect();
        let ev = cw4i::sim::decode(&sc.render(), sc.rate, Config::default());
        for e in &ev {
            if let cw4i::Event::Commit {
                sample, text, freq, ..
            } = e
            {
                let t = *sample as f64 / sc.rate as f64;
                wrong += text
                    .split_whitespace()
                    .filter(|w| is_call_like(w) && !truth.iter().any(|x| x == w))
                    .count();
                let own = busy.iter().any(|b| {
                    t > b.0 - 0.5 && t < b.1 + GAP_GRACE && (b.2 - freq).abs() < OTHER_HZ / 2.0
                });
                if !own {
                    gap_chars += text.replace(' ', "").len();
                }
            }
        }
        gap_secs += sc.secs();
    }
    let hours = seeds as f64 * secs / 3600.0;
    println!(
        "Operating scenes: {seeds} x {secs} s, CQ at 32 WPM, callers at 0.5/s, 15 dB in 500 Hz"
    );
    println!(
        "transmissions shown correctly    {:>6.1} %",
        100.0 * shown / n
    );
    println!(
        "callers missed                   {:>6.1} %",
        100.0 * missed / n
    );
    println!(
        "callsign latency (median)        {:>6.2} characters",
        median(lat)
    );
    println!(
        "wrong callsigns per hour         {:>6.1}",
        wrong as f64 / hours
    );
    println!(
        "characters with no station on air {:>5.1} per hour",
        gap_chars as f64 / (gap_secs / 3600.0)
    );
}

/// CPU time per second of audio with many active stations, at 8 and 48 kHz.
fn cpu() {
    println!("CPU seconds per second of audio on one core");
    for n in [1, 10, 20] {
        for rate in [8000, 48000] {
            let mut sc = Scene::pileup(n, 500.0, (20.0, 35.0), 20.0, 1);
            sc.rate = rate;
            for st in &mut sc.stations {
                st.text = "CQ TEST DE JA1ZLO JA1ZLO TEST".into();
            }
            let x = sc.render();
            let t = Instant::now();
            let ev = cw4i::sim::decode(&x, rate, Config::default());
            let s = t.elapsed().as_secs_f64() / (x.len() as f64 / rate as f64);
            let tracks = cw4i::sim::texts(&ev).len();
            println!("{n:>3} stations at {rate:>5} Hz ({tracks:>2} tracks): {s:.4}");
        }
    }
}

fn write_wav(path: &std::path::Path, x: &[f64], rate: u32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let peak = x.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-9);
    if let Ok(mut w) = hound::WavWriter::create(path, spec) {
        for &v in x {
            let _ = w.write_sample((v / peak * 30000.0) as i16);
        }
        let _ = w.finalize();
    }
}

/// Writes contest scenes as WAV files with a manifest of the truth (file, frequency,
/// WPM, start, text) for scoring other decoders.
fn export(dir: &str, seeds: u64) {
    let _ = std::fs::create_dir_all(dir);
    let mut manifest = String::from("file\tfreq\twpm\tstart\ttext\n");
    let mut k = 0;
    for (n, snr) in [
        (1, 5.0),
        (1, 10.0),
        (1, 20.0),
        (2, 10.0),
        (4, 10.0),
        (8, 10.0),
    ] {
        for wpm in [(25.0, 25.0), (35.0, 35.0), (45.0, 45.0), (30.0, 40.0)] {
            if (n == 1) == (wpm.0 != wpm.1) {
                continue;
            }
            for seed in 1..=seeds {
                let spread = if n == 1 { 0.0 } else { 300.0 };
                let sc = Scene::contest(n, spread, wpm, keydown_2500(snr), 1000 * seed + k);
                let name = format!("contest_{k:04}_n{n}_{}wpm_{snr}db.wav", wpm.0);
                k += 1;
                write_wav(
                    &std::path::Path::new(dir).join(&name),
                    &sc.render(),
                    sc.rate,
                );
                for st in &sc.stations {
                    manifest += &format!(
                        "{name}\t{:.1}\t{:.1}\t{:.3}\t{}\n",
                        st.freq, st.wpm, st.start, st.text
                    );
                }
            }
        }
    }
    let _ = std::fs::write(std::path::Path::new(dir).join("manifest.tsv"), manifest);
    println!("wrote {k} files and manifest.tsv to {dir}");
}

/// Scores the plain-text output of any decoder (OUT/<file>.txt, any layout) against the
/// manifest in DIR: callsigns found, exchanges found and CER of the best-matching stretch.
fn score_tool(dir: &str, out: &str) {
    let Ok(m) = std::fs::read_to_string(std::path::Path::new(dir).join("manifest.tsv")) else {
        println!("no manifest.tsv in {dir}");
        return;
    };
    let (mut call, mut exch, mut c, mut n, mut busted) = (0.0, 0.0, 0.0, 0.0, 0);
    let mut calls_seen: std::collections::HashMap<String, Vec<String>> = Default::default();
    for line in m.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 5 {
            continue;
        }
        let txt = std::fs::read_to_string(std::path::Path::new(out).join(format!("{}.txt", f[0])))
            .unwrap_or_default()
            .to_uppercase();
        let mut words = f[4].split_whitespace();
        let cs = words.next().unwrap_or("");
        let ex: String = words.collect::<Vec<_>>().join("");
        let squash: String = txt.chars().filter(|c| !c.is_whitespace()).collect();
        call += f64::from(u8::from(txt.split_whitespace().any(|w| w == cs)));
        exch += f64::from(u8::from(!ex.is_empty() && squash.contains(&ex)));
        c += cw4i::sim::cer_within(&txt, f[4]).min(1.0);
        n += 1.0;
        calls_seen
            .entry(f[0].to_string())
            .or_default()
            .push(cs.to_string());
    }
    for (file, truth) in &calls_seen {
        let txt = std::fs::read_to_string(std::path::Path::new(out).join(format!("{file}.txt")))
            .unwrap_or_default()
            .to_uppercase();
        busted += txt
            .split_whitespace()
            .filter(|w| is_call_like(w) && !truth.iter().any(|t| t == w))
            .count();
    }
    println!(
        "{n} transmissions: callsign {:.1} %, exchange {:.1} %, CER within {:.1} %, busted calls {busted}",
        100.0 * call / n,
        100.0 * exch / n,
        100.0 * c / n
    );
}

fn is_call_like(w: &str) -> bool {
    let base = w.split('/').next().unwrap_or("");
    let b = base.as_bytes();
    let Some(d) = b.iter().rposition(|c| c.is_ascii_digit()) else {
        return false;
    };
    b[..d].iter().any(|c| c.is_ascii_alphabetic())
        && d + 1 < b.len()
        && b[d + 1..].iter().all(|c| c.is_ascii_alphabetic())
}

/// False characters per minute and confirmed tracks in a decode.
fn false_text(x: &[f64], rate: u32) -> (usize, usize) {
    let ev = cw4i::sim::decode(x, rate, Config::default());
    let chars = cw4i::sim::texts(&ev)
        .iter()
        .map(|t| t.2.replace(' ', "").len())
        .sum();
    let conf = ev
        .iter()
        .filter(|e| matches!(e, cw4i::Event::TrackConfirmed { .. }))
        .count();
    (chars, conf)
}

type Background = (&'static str, fn(&mut Scene));

const BACKGROUNDS: [Background; 8] = [
    ("white", |_| {}),
    ("static crashes", |sc| {
        sc.crash_hz = 1.0;
        sc.crash_amp = 1.0;
    }),
    ("impulses", |sc| {
        sc.impulse_hz = 10.0;
        sc.impulse_amp = 1.0;
    }),
    ("machine buzz", |sc| {
        sc.buzz_hz = 100.0;
        sc.buzz_amp = 0.1;
    }),
    ("carriers", |sc| {
        sc.qrm.push(Tone {
            freq: 700.0,
            amp: 0.3,
        });
        sc.qrm.push(Tone {
            freq: 950.0,
            amp: 0.05,
        });
    }),
    ("agc breathing", |sc| {
        sc.crash_hz = 1.0;
        sc.crash_amp = 1.0;
        sc.agc = 0.3;
    }),
    ("level wander", |sc| sc.noise_wander = 10.0),
    ("level step", |sc| sc.noise_step = 10.0),
];

/// Blocks of 0.5 s of a recording in which no bin rises well above the median noise.
fn quiet_blocks(x: &[f64], rate: u32) -> Vec<f64> {
    let n = (WINDOW_SEC * rate as f64) as usize;
    let fft = rustfft::FftPlanner::new().plan_fft_forward(n);
    let hz = rate as f64 / n as f64;
    let (b0, b1) = ((250.0 / hz) as usize, (1300.0 / hz) as usize);
    let block = rate as usize / 2;
    let peaks: Vec<(f64, f64)> = x
        .chunks_exact(block)
        .map(|c| {
            let mut all = Vec::new();
            for f in c.chunks_exact(n) {
                let mut b: Vec<rustfft::num_complex::Complex<f64>> = f
                    .iter()
                    .map(|&v| rustfft::num_complex::Complex::new(v, 0.0))
                    .collect();
                fft.process(&mut b);
                all.extend(b[b0..b1].iter().map(|v| v.norm_sqr()));
            }
            let mx = all.iter().cloned().fold(0.0, f64::max);
            all.sort_by(f64::total_cmp);
            (mx, all[all.len() / 2])
        })
        .collect();
    let mut med: Vec<f64> = peaks.iter().map(|p| p.1).collect();
    med.sort_by(f64::total_cmp);
    let floor = med.get(med.len() / 2).copied().unwrap_or(0.0);
    x.chunks_exact(block)
        .zip(&peaks)
        .filter(|(_, p)| p.0 < QUIET_PEAK * floor)
        .flat_map(|(c, _)| c.iter().copied())
        .collect()
}

fn silence(seeds: u64, dir: &str) {
    println!("False text on 60 s of background without CW, {seeds} seeds");
    println!("{:<16} {:>10} {:>10}", "background", "chars/min", "tracks");
    for (name, f) in BACKGROUNDS {
        let (mut c, mut t) = (0, 0);
        for seed in 1..=seeds {
            let mut sc = Scene::silence(60.0, seed);
            f(&mut sc);
            let (a, b) = false_text(&sc.render(), sc.rate);
            c += a;
            t += b;
        }
        println!("{name:<16} {:>10.2} {t:>10}", c as f64 / seeds as f64);
    }
    let (mut c, mut t, mut secs) = (0, 0, 0.0);
    for name in wav_names(dir) {
        let Some((rate, x)) = read_wav(&std::path::Path::new(dir).join(&name)) else {
            continue;
        };
        let q = quiet_blocks(&x, rate);
        if q.len() < 3 * rate as usize {
            continue;
        }
        secs += q.len() as f64 / rate as f64;
        let (a, b) = false_text(&q, rate);
        c += a;
        t += b;
    }
    println!(
        "{:<16} {:>10.2} {t:>10}   ({secs:.0} s of quiet parts of the recordings)",
        "recordings",
        c as f64 / (secs / 60.0).max(1e-9)
    );
    println!("Gaps of 20 s between three transmissions at 10 dB, {seeds} seeds");
    let (mut gap_chars, mut gap_secs, mut quiet) = (0, 0.0, 0.0f64);
    for seed in 1..=seeds {
        let text = "CQ TEST DE JA1ZLO K";
        let mut sts = Vec::new();
        let mut start = 2.0;
        for k in 0..3 {
            let mut st = Station::new(text, 700.0, 25.0);
            st.start = start;
            st.seed = seed * 10 + k;
            start = st.end() + 20.0;
            sts.push(st);
        }
        let ends: Vec<(f64, f64)> = sts.iter().map(|s| (s.start, s.end())).collect();
        let mut sc = Scene::new(8000, sts, 10.0, seed);
        sc.tail = 20.0;
        let ev = cw4i::sim::decode(&sc.render(), sc.rate, Config::default());
        for e in &ev {
            if let cw4i::Event::Commit { sample, text, .. } = e {
                let t = *sample as f64 / 8000.0;
                let n = text.replace(' ', "").len();
                if n == 0 {
                    continue;
                }
                if let Some(&(_, end)) = ends.iter().rev().find(|w| w.0 <= t) {
                    if t > end + GAP_GRACE {
                        gap_chars += n;
                    }
                    quiet = quiet.max(t - end);
                }
            }
        }
        gap_secs += 3.0 * 20.0;
    }
    println!(
        "false chars/min in gaps {:.2}, longest commit after a transmission ended {quiet:.1} s",
        gap_chars as f64 / (gap_secs / 60.0)
    );
}

/// Per recording, from a 40 ms STFT: SNR in 2500 Hz of the strongest tone (key-down
/// power over the median noise of bins 60 to 150 Hz away, inside a receiver CW filter), the spread of its key-down
/// level over 0.5 s blocks (fade depth) and the rate of its swings, the level of the
/// second strongest tone below it (spread) and the kurtosis of the audio (impulses).
fn survey(dir: &str) {
    println!(
        "{:<34} {:>7} {:>6} {:>6} {:>7} {:>6}",
        "file", "snr2500", "fadedB", "fadeHz", "2nd dB", "kurt"
    );
    for name in wav_names(dir) {
        let Some((rate, x)) = read_wav(&std::path::Path::new(dir).join(&name)) else {
            continue;
        };
        let n = (WINDOW_SEC * rate as f64) as usize;
        let hop = rate as usize / 200;
        let win: Vec<f64> = (0..n)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * (i as f64 + 0.5) / n as f64).cos())
            .collect();
        let fft = rustfft::FftPlanner::new().plan_fft_forward(n);
        let hz = rate as f64 / n as f64;
        let (b0, b1) = ((250.0 / hz) as usize, (1300.0 / hz) as usize);
        let mut frames: Vec<Vec<f64>> = Vec::new();
        let mut i = 0;
        while i + n <= x.len() {
            let mut b: Vec<rustfft::num_complex::Complex<f64>> = (0..n)
                .map(|k| rustfft::num_complex::Complex::new(x[i + k] * win[k], 0.0))
                .collect();
            fft.process(&mut b);
            frames.push(b[b0..b1].iter().map(|v| v.norm_sqr()).collect());
            i += hop;
        }
        let pct = |mut v: Vec<f64>, q: f64| {
            v.sort_by(f64::total_cmp);
            v.get(((v.len() as f64 - 1.0) * q) as usize)
                .copied()
                .unwrap_or(f64::NAN)
        };
        let nb = b1 - b0;
        let on: Vec<f64> = (0..nb)
            .map(|k| pct(frames.iter().map(|f| f[k]).collect(), 0.9))
            .collect();
        let k0 = (0..nb)
            .max_by(|&a, &b| on[a].total_cmp(&on[b]))
            .unwrap_or(0);
        let far: Vec<f64> = frames
            .iter()
            .flat_map(|f| {
                (0..nb)
                    .filter(|&k| {
                        let d = (k as f64 - k0 as f64).abs() * hz;
                        (60.0..150.0).contains(&d)
                    })
                    .map(|k| f[k])
                    .collect::<Vec<_>>()
            })
            .collect();
        let noise = pct(far, 0.5) / 0.69;
        let snr =
            10.0 * ((on[k0] - noise).max(1e-30) / noise / (2500.0 * WINDOW_SEC / 1.5)).log10();
        let second = (0..nb)
            .filter(|&k| (k as f64 - k0 as f64).abs() * hz > 60.0)
            .map(|k| on[k])
            .fold(0.0, f64::max);
        let second_db = 10.0 * ((second - noise).max(1e-30) / (on[k0] - noise).max(1e-30)).log10();
        let block = 100;
        let lv: Vec<f64> = frames
            .chunks(block)
            .filter_map(|c| {
                let v: Vec<f64> = c
                    .iter()
                    .map(|f| f[k0])
                    .filter(|&p| p > 10.0 * noise)
                    .collect();
                (v.len() > 5).then(|| 10.0 * pct(v, 0.8).log10())
            })
            .collect();
        let depth = pct(lv.clone(), 0.95) - pct(lv.clone(), 0.05);
        let m = pct(lv.clone(), 0.5);
        let cross = lv
            .windows(2)
            .filter(|w| (w[0] - m) * (w[1] - m) < 0.0)
            .count();
        let fade_hz = cross as f64 / 2.0 / (lv.len() as f64 * block as f64 / 200.0).max(0.1);
        let m2 = x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64;
        let m4 = x.iter().map(|v| v.powi(4)).sum::<f64>() / x.len() as f64;
        println!(
            "{name:<34} {snr:>7.1} {depth:>6.1} {fade_hz:>6.2} {second_db:>7.1} {:>6.1}",
            m4 / (m2 * m2)
        );
    }
}

fn wav_names(dir: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".wav"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Expected text from a file name: easy_JF2OZHp2.wav reads JF2OZH/2.
fn expected(name: &str) -> Option<String> {
    let stem = name.strip_suffix(".wav")?.trim_end_matches("-noise");
    let tags = [
        "easy",
        "hard",
        "veryhard",
        "veryveryhard",
        "fading",
        "tate",
        "hayasugi",
        "sokudokawaru",
        "machinenoise",
        "pile",
    ];
    let mut parts: Vec<&str> = stem.split('_').collect();
    while parts.len() > 1 && tags.contains(&parts[0]) {
        parts.remove(0);
    }
    let s = parts.join(" ").replace(['(', ')'], "");
    if s.starts_with("pileup") || s == "jissai" {
        return None;
    }
    let mut out = String::new();
    let cs: Vec<char> = s.chars().collect();
    for (i, &c) in cs.iter().enumerate() {
        let digit = cs.get(i + 1).is_some_and(|d| d.is_ascii_digit());
        out.push(if c == 'p' && digit { '/' } else { c });
    }
    Some(out)
}

fn read_wav(path: &std::path::Path) -> Option<(u32, Vec<f64>)> {
    let mut r = hound::WavReader::open(path).ok()?;
    let spec = r.spec();
    let ch = spec.channels as usize;
    let x: Vec<f64> = match spec.sample_format {
        hound::SampleFormat::Float => r
            .samples::<f32>()
            .map(|s| s.unwrap_or(0.0) as f64)
            .collect(),
        hound::SampleFormat::Int => {
            let full = (1i64 << (spec.bits_per_sample - 1)) as f64;
            r.samples::<i32>()
                .map(|s| s.unwrap_or(0) as f64 / full)
                .collect()
        }
    };
    Some((spec.sample_rate, x.chunks(ch).map(|c| c[0]).collect()))
}

fn files(dir: &str) {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".wav"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    let (mut sum, mut n) = (0.0, 0.0);
    let mut extra_sum = 0;
    let mut within_sum = 0.0;
    for name in names {
        let Some((rate, x)) = read_wav(&std::path::Path::new(dir).join(&name)) else {
            println!("{name}: unreadable");
            continue;
        };
        let t = Instant::now();
        let tr = cw4i::sim::texts(&cw4i::sim::decode(&x, rate, Config::default()));
        let rt = t.elapsed().as_secs_f64() / (x.len() as f64 / rate as f64);
        let want = expected(&name);
        let pick = |cands: Vec<String>| -> (String, f64) {
            match &want {
                Some(w) => cands
                    .into_iter()
                    .map(|c| (cer(&c, w).min(1.0), c))
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(e, c)| (c, e))
                    .unwrap_or((String::new(), 1.0)),
                None => (cands.join(" | "), f64::NAN),
            }
        };
        let (got, e) = pick(tr.iter().map(|t| t.2.clone()).collect());
        let within = want.as_ref().map_or(f64::NAN, |w| {
            tr.iter()
                .map(|t| cw4i::sim::cer_within(&t.2, w).min(1.0))
                .fold(1.0, f64::min)
        });
        if !within.is_nan() {
            within_sum += within;
        }
        let extra: usize = if want.is_some() {
            let f = tr.iter().find(|t| t.2 == got).map_or(0.0, |t| t.1);
            tr.iter()
                .filter(|t| (t.1 - f).abs() > OTHER_HZ)
                .map(|t| t.2.replace(' ', "").len())
                .sum()
        } else {
            0
        };
        extra_sum += extra;
        print!(
            "{name:<34} want {:<16} cer {:>5.0}% within {:>5.0}% {got:<24} other {extra:>3}",
            want.clone().unwrap_or_default(),
            100.0 * e,
            100.0 * within
        );
        println!(" {rate} Hz {rt:.3} xRT");
        if !e.is_nan() {
            sum += e;
            n += 1.0;
        }
    }
    println!(
        "mean CER over {n} labelled files: {:.1}% (within the decoded text {:.1}%), characters on other tracks: {extra_sum}",
        100.0 * sum / n,
        100.0 * within_sum / n
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let seeds = args
        .iter()
        .position(|a| a == "--seeds")
        .and_then(|i| args.get(i + 1)?.parse().ok())
        .unwrap_or(2);
    let pos: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && (*i == 0 || args[i - 1] != "--seeds"))
        .map(|(_, a)| a)
        .collect();
    let mode = pos.first().map(|s| s.as_str()).unwrap_or("all");
    let dir = pos
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("../morse_test_data");
    if mode == "sweep" || mode == "all" {
        sweep(seeds);
    }
    if mode == "pileup" || mode == "all" {
        pileup(seeds);
    }
    if mode == "hf" || mode == "all" {
        hf(seeds);
    }
    if mode == "weak" || mode == "all" {
        weak(seeds);
    }
    if mode == "realistic" || mode == "all" {
        realistic(seeds);
    }
    if mode == "silence" || mode == "all" {
        silence(seeds, dir);
    }
    if mode == "cpu" || mode == "all" {
        cpu();
    }
    if mode == "scene" || mode == "all" {
        scene(seeds);
    }
    if mode == "contest" || mode == "all" {
        contest(seeds);
    }
    if mode == "export" {
        export(pos.get(1).map(|s| s.as_str()).unwrap_or("scenes"), seeds);
    }
    if mode == "score" {
        score_tool(
            pos.get(1).map(|s| s.as_str()).unwrap_or("scenes"),
            pos.get(2).map(|s| s.as_str()).unwrap_or("out"),
        );
    }
    if mode == "survey" {
        survey(dir);
    }
    if mode == "files" || mode == "all" {
        files(dir);
    }
}
