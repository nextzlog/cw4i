//! Trains the confirmation classifier of cw4i on simulated scenes and writes its weights.
//!
//! cargo run --release --manifest-path tools/train_confirm/Cargo.toml -- lib/src/confirm_weights.rs

#![allow(clippy::needless_range_loop)]

use cw4i::sim::{Channel, Fist, Rng, Scene};
use cw4i::{Config, Decoder};
use std::sync::Mutex;

const NF: usize = 12;
const NH: usize = 8;
const THREADS: usize = 32;
const EPOCHS: usize = 4000;
const LR: f32 = 0.01;
const L2: f32 = 1e-3;
const NEG_WEIGHT: f32 = 4.0;

fn config() -> Config {
    Config {
        birth_db: 6.0,
        birth_run: 6,
        ..Config::default()
    }
}

fn scenes() -> Vec<Scene> {
    let mut v = Vec::new();
    for seed in 101..=140 {
        for k in 0..8 {
            let mut sc = Scene::silence(60.0, seed * 10 + k);
            match k {
                1 => {
                    sc.crash_hz = 1.0;
                    sc.crash_amp = 1.0;
                }
                2 => {
                    sc.impulse_hz = 10.0;
                    sc.impulse_amp = 1.0;
                }
                3 => {
                    sc.buzz_hz = 100.0;
                    sc.buzz_amp = 0.1;
                }
                4 => sc.agc = 0.3,
                5 => sc.noise_wander = 10.0,
                6 => sc.noise_step = 10.0,
                7 => {
                    sc.crash_hz = 1.0;
                    sc.crash_amp = 1.0;
                    sc.agc = 0.3;
                }
                _ => {}
            }
            v.push(sc);
        }
    }
    let mut rng = Rng::new(7);
    for seed in 101..=700 {
        let kd = -8.0 + 16.0 * rng.uniform();
        let mut sc = Scene::contest(1, 0.0, (18.0, 40.0), kd, seed);
        let st = &mut sc.stations[0];
        st.fist = [Fist::Paddle, Fist::Bug, Fist::Straight, Fist::Even][(seed % 4) as usize];
        st.jitter = 0.05 + 0.05 * rng.uniform();
        if seed % 2 == 0 {
            st.channel = Channel::MID_QUIET;
            st.qsb_depth = 0.5 * rng.uniform();
            st.qsb_hz = 0.1 + 0.2 * rng.uniform();
        }
        sc.tail = 8.0;
        v.push(sc);
    }
    for seed in 101..=240 {
        let n = 2 + (seed % 7) as usize;
        v.push(Scene::contest(
            n,
            300.0,
            (25.0, 40.0),
            4.0 + (seed % 10) as f64,
            seed,
        ));
    }
    for seed in 101..=148 {
        let mut sc = Scene::operating(60.0, 0.5, 11.0, seed);
        if seed % 3 == 0 {
            sc.agc = 0.0;
        }
        if seed % 3 == 1 {
            sc.agc = 0.0;
            sc.crash_hz = 0.0;
        }
        v.push(sc);
    }
    v
}

fn records(sc: &Scene) -> Vec<(Vec<f32>, f32)> {
    let x = sc.render();
    let mut d = Decoder::with_config(sc.rate, config());
    d.record_confirmations(true);
    for c in x.chunks(512) {
        d.push_f64(c);
    }
    d.flush();
    let fs = d.frame_seconds();
    let lag = 0.04;
    d.take_confirmations()
        .into_iter()
        .map(|(_, fr, hz, f)| {
            let t = fr as f64 * fs + lag;
            let real = sc
                .stations
                .iter()
                .any(|s| (s.freq - hz).abs() < 25.0 && t > s.start - 0.5 && t < s.end() + 2.0);
            (f, f32::from(u8::from(real)))
        })
        .collect()
}

struct Net {
    w1: [[f32; NF]; NH],
    b1: [f32; NH],
    w2: [f32; NH],
    b2: f32,
}

fn forward(n: &Net, x: &[f32]) -> ([f32; NH], f32) {
    let mut h = [0.0; NH];
    let mut o = n.b2;
    for j in 0..NH {
        let mut a = n.b1[j];
        for i in 0..NF {
            a += n.w1[j][i] * x[i];
        }
        h[j] = a.tanh();
        o += n.w2[j] * h[j];
    }
    (h, 1.0 / (1.0 + (-o).exp()))
}

fn train(data: &[(Vec<f32>, f32)]) -> Net {
    let mut rng = Rng::new(42);
    let mut n = Net {
        w1: [[0.0; NF]; NH],
        b1: [0.0; NH],
        w2: [0.0; NH],
        b2: 0.0,
    };
    for j in 0..NH {
        for i in 0..NF {
            n.w1[j][i] = 0.3 * rng.normal() as f32;
        }
        n.w2[j] = 0.3 * rng.normal() as f32;
    }
    let np = NH * NF + NH + NH + 1;
    let (mut m, mut v) = (vec![0.0f32; np], vec![0.0f32; np]);
    for ep in 1..=EPOCHS {
        let mut g = vec![0.0f32; np];
        let mut wsum = 0.0;
        for (x, y) in data {
            let w = if *y > 0.5 { 1.0 } else { NEG_WEIGHT };
            wsum += w;
            let (h, p) = forward(&n, x);
            let d = w * (p - y);
            for j in 0..NH {
                g[NH * NF + NH + j] += d * h[j];
                let dh = d * n.w2[j] * (1.0 - h[j] * h[j]);
                for i in 0..NF {
                    g[j * NF + i] += dh * x[i];
                }
                g[NH * NF + j] += dh;
            }
            g[np - 1] += d;
        }
        let mut params: Vec<f32> = n.w1.iter().flatten().copied().collect();
        params.extend(n.b1);
        params.extend(n.w2);
        params.push(n.b2);
        for k in 0..np {
            let gk = g[k] / wsum + L2 * params[k];
            m[k] = 0.9 * m[k] + 0.1 * gk;
            v[k] = 0.999 * v[k] + 0.001 * gk * gk;
            let mh = m[k] / (1.0 - 0.9f32.powi(ep as i32));
            let vh = v[k] / (1.0 - 0.999f32.powi(ep as i32));
            params[k] -= LR * mh / (vh.sqrt() + 1e-8);
        }
        for j in 0..NH {
            for i in 0..NF {
                n.w1[j][i] = params[j * NF + i];
            }
            n.b1[j] = params[NH * NF + j];
            n.w2[j] = params[NH * NF + NH + j];
        }
        n.b2 = params[np - 1];
    }
    n
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or("lib/src/confirm_weights.rs".into());
    let scenes = scenes();
    let queue = Mutex::new(scenes.into_iter().enumerate().collect::<Vec<_>>());
    let all = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| loop {
                let Some((i, sc)) = queue.lock().unwrap().pop() else {
                    break;
                };
                let r = records(&sc);
                all.lock().unwrap().push((i, r));
            });
        }
    });
    let mut all = all.into_inner().unwrap();
    all.sort_by_key(|x| x.0);
    let (mut tr, mut va) = (Vec::new(), Vec::new());
    for (i, r) in all {
        if i % 5 == 4 {
            va.extend(r);
        } else {
            tr.extend(r);
        }
    }
    let mut mean = [0.0f32; NF];
    let mut sd = [0.0f32; NF];
    for (x, _) in &tr {
        for i in 0..NF {
            mean[i] += x[i] / tr.len() as f32;
        }
    }
    for (x, _) in &tr {
        for i in 0..NF {
            sd[i] += (x[i] - mean[i]).powi(2) / tr.len() as f32;
        }
    }
    for v in &mut sd {
        *v = v.sqrt().max(1e-3);
    }
    let norm = |d: &mut Vec<(Vec<f32>, f32)>| {
        for (x, _) in d.iter_mut() {
            for i in 0..NF {
                x[i] = (x[i] - mean[i]) / sd[i];
            }
        }
    };
    norm(&mut tr);
    norm(&mut va);
    let pos = tr.iter().filter(|x| x.1 > 0.5).count();
    println!(
        "train {} ({pos} stations), validation {}",
        tr.len(),
        va.len()
    );
    let net = train(&tr);
    for (name, d) in [("train", &tr), ("validation", &va)] {
        let (mut tp, mut fp, mut p, mut n) = (0, 0, 0, 0);
        for (x, y) in d.iter() {
            let q = forward(&net, x).1 >= 0.5;
            if *y > 0.5 {
                p += 1;
                tp += usize::from(q);
            } else {
                n += 1;
                fp += usize::from(q);
            }
        }
        println!("{name}: stations confirmed {tp}/{p}, noise confirmed {fp}/{n}");
    }
    let f = |v: &[f32]| {
        v.iter()
            .map(|x| format!("{x:.6}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut src = String::from(
        "//! Weights of the confirmation classifier, written by tools/train_confirm.\n\n#![allow(clippy::excessive_precision)]\n\nuse crate::confirm::{NF, NH};\n\n",
    );
    src += &format!("pub const MEAN: [f32; NF] = [{}];\n", f(&mean));
    src += &format!("pub const SD: [f32; NF] = [{}];\n", f(&sd));
    src += "pub const W1: [[f32; NF]; NH] = [\n";
    for row in &net.w1 {
        src += &format!("    [{}],\n", f(row));
    }
    src += "];\n";
    src += &format!("pub const B1: [f32; NH] = [{}];\n", f(&net.b1));
    src += &format!("pub const W2: [f32; NH] = [{}];\n", f(&net.w2));
    src += &format!("pub const B2: f32 = {:.6};\n", net.b2);
    std::fs::write(&out, src).expect("write weights");
    println!("wrote {out}");
}
