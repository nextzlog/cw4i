//! Short-window envelope keyer for very fast stations: the track's own tone is mixed
//! down, smoothed by two boxcars of a few milliseconds and keyed every millisecond with
//! a threshold between its noise and peak levels.

use crate::Element;
use std::collections::VecDeque;

const BOX_SEC: f64 = 0.003;
const TICK_SEC: f64 = 0.001;
const MIN_TICKS: u32 = 2;
const PEAK_DECAY: f64 = 0.998;
const NOISE_RATE: f64 = 0.01;
const NOISE_GATE: f64 = 2.0;
const WARM_TICKS: u32 = 50;
const PEAK_SHARE: f64 = 0.25;
const NOISE_MUL: f64 = 8.0;
const HYSTERESIS: f64 = 1.26;
const ELEMENTS: usize = 16;

pub struct FastKeyer {
    ph: f64,
    b1: VecDeque<(f64, f64)>,
    b2: VecDeque<(f64, f64)>,
    s1: (f64, f64),
    s2: (f64, f64),
    len: usize,
    tick: usize,
    n: usize,
    noise: f64,
    peak: f64,
    pub on: bool,
    run: u32,
    hold: u32,
    warm: u32,
    els: VecDeque<f64>,
    pub out: Vec<Element>,
}

impl FastKeyer {
    pub fn new(rate: f64) -> FastKeyer {
        FastKeyer {
            ph: 0.0,
            b1: VecDeque::new(),
            b2: VecDeque::new(),
            s1: (0.0, 0.0),
            s2: (0.0, 0.0),
            len: ((BOX_SEC * rate).round() as usize).max(1),
            tick: ((TICK_SEC * rate).round() as usize).max(1),
            n: 0,
            noise: 0.0,
            peak: 0.0,
            on: false,
            run: 0,
            hold: 0,
            warm: 0,
            els: VecDeque::new(),
            out: Vec::new(),
        }
    }

    pub fn feed(&mut self, x: &[f64], hz: f64, rate: f64) {
        let w = 2.0 * std::f64::consts::PI * hz / rate;
        let (rc, rs) = (w.cos(), w.sin());
        let (mut c, mut sn) = (self.ph.cos(), self.ph.sin());
        for &v in x {
            let z = (v * c, -v * sn);
            (c, sn) = (c * rc - sn * rs, sn * rc + c * rs);
            let y = boxcar(&mut self.b1, &mut self.s1, z, self.len);
            let y = boxcar(&mut self.b2, &mut self.s2, y, self.len);
            self.n += 1;
            if self.n.is_multiple_of(self.tick) && self.b2.len() == self.len {
                self.decide(y.0 * y.0 + y.1 * y.1);
            }
        }
        self.ph = (self.ph + w * x.len() as f64) % (2.0 * std::f64::consts::PI);
    }

    fn decide(&mut self, p: f64) {
        if self.warm < WARM_TICKS {
            self.warm += 1;
            self.noise += (p - self.noise) / self.warm as f64;
            self.peak = self.peak.max(p);
            return;
        }
        self.peak = p.max(self.peak * PEAK_DECAY);
        let th = (PEAK_SHARE * self.peak).max(NOISE_MUL * self.noise);
        let want = if self.on {
            p > th / HYSTERESIS
        } else {
            p > th * HYSTERESIS
        };
        if want != self.on {
            self.hold += 1;
            if self.hold >= MIN_TICKS {
                let d = self.run as f64 * TICK_SEC;
                self.els.push_back(d);
                if self.els.len() > ELEMENTS {
                    self.els.pop_front();
                }
                self.out.push(Element { mark: self.on, d });
                self.on = want;
                self.run = self.hold;
                self.hold = 0;
            }
        } else {
            self.hold = 0;
            self.run += 1;
        }
        if p < NOISE_GATE * self.noise {
            self.noise += NOISE_RATE * (p - self.noise);
        }
    }

    /// Lower quartile (s) of the recent elements, 0 until enough are seen.
    pub fn unit(&self) -> f64 {
        if self.els.len() < ELEMENTS / 2 {
            return 0.0;
        }
        let mut d: Vec<f64> = self.els.iter().copied().collect();
        d.sort_by(f64::total_cmp);
        d[d.len() / 4]
    }

    pub fn gap(&self) -> f64 {
        if self.on {
            0.0
        } else {
            self.run as f64 * TICK_SEC
        }
    }
}

fn boxcar(
    b: &mut VecDeque<(f64, f64)>,
    s: &mut (f64, f64),
    z: (f64, f64),
    len: usize,
) -> (f64, f64) {
    b.push_back(z);
    s.0 += z.0;
    s.1 += z.1;
    if b.len() > len {
        let o = b.pop_front().unwrap();
        s.0 -= o.0;
        s.1 -= o.1;
    }
    (s.0 / len as f64, s.1 / len as f64)
}
