use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

pub const LO_HZ: f64 = 250.0;

pub const HI_HZ: f64 = 1300.0;

const ROWS: usize = 160;

const HOP_SEC: f64 = 0.02;

pub struct Waterfall {
    fft: Arc<dyn Fft<f32>>,
    size: usize,
    hop: usize,
    window: Vec<f32>,
    buffer: Vec<f32>,
    lo: usize,
    hi: usize,
    rows: Vec<Vec<f32>>,
    floor: f32,
    pub version: u64,
}

impl Waterfall {
    pub fn new(rate: u32) -> Self {
        let size = (rate as f64 / 12.0).log2().ceil().exp2() as usize;
        let size = size.clamp(256, 8192);
        let hop = (rate as f64 * HOP_SEC) as usize;
        let window = (0..size)
            .map(|i| {
                let x = std::f32::consts::PI * i as f32 / size as f32;
                x.sin() * x.sin()
            })
            .collect();
        let bin = rate as f64 / size as f64;
        Waterfall {
            fft: FftPlanner::new().plan_fft_forward(size),
            size,
            hop,
            window,
            buffer: Vec::new(),
            lo: (LO_HZ / bin) as usize,
            hi: (HI_HZ / bin).ceil() as usize,
            rows: Vec::new(),
            floor: -60.0,
            version: 0,
        }
    }

    pub fn push(&mut self, samples: &[f32]) {
        self.buffer.extend_from_slice(samples);
        while self.buffer.len() >= self.size {
            let mut x: Vec<Complex32> = self.buffer[..self.size]
                .iter()
                .zip(&self.window)
                .map(|(s, w)| Complex32::new(s * w, 0.0))
                .collect();
            self.fft.process(&mut x);
            let row: Vec<f32> = x[self.lo..self.hi]
                .iter()
                .map(|c| 10.0 * (c.norm_sqr() + 1e-12).log10())
                .collect();
            let mut sorted = row.clone();
            sorted.sort_by(f32::total_cmp);
            let median = sorted[sorted.len() / 2];
            self.floor += 0.05 * (median - self.floor);
            self.rows.insert(0, row);
            self.rows.truncate(ROWS);
            self.version += 1;
            self.buffer.drain(..self.hop.min(self.buffer.len()));
        }
    }

    pub fn image(&self, dark: bool, newest_bottom: bool) -> egui::ColorImage {
        let w = self.hi - self.lo;
        let mut pixels = Vec::with_capacity(w * ROWS);
        for y in 0..ROWS {
            let r = if newest_bottom { ROWS - 1 - y } else { y };
            match self.rows.get(r) {
                Some(row) => {
                    for &v in row {
                        let t = ((v - self.floor - 3.0) / 30.0).clamp(0.0, 1.0);
                        pixels.push(colour(t, dark));
                    }
                }
                None => pixels.extend(std::iter::repeat_n(colour(0.0, dark), w)),
            }
        }
        egui::ColorImage::new([w, ROWS], pixels)
    }

    pub fn image_vertical(&self, dark: bool, newest_right: bool) -> egui::ColorImage {
        let h = self.hi - self.lo;
        let mut pixels = Vec::with_capacity(h * ROWS);
        for f in 0..h {
            for x in 0..ROWS {
                let age = if newest_right { ROWS - 1 - x } else { x };
                let v = self.rows.get(age).map(|row| row[f]);
                let t = v.map_or(0.0, |v| ((v - self.floor - 3.0) / 30.0).clamp(0.0, 1.0));
                pixels.push(colour(t, dark));
            }
        }
        egui::ColorImage::new([ROWS, h], pixels)
    }

    pub fn clear(&mut self) {
        self.rows.clear();
        self.version += 1;
    }
}

fn colour(t: f32, dark: bool) -> egui::Color32 {
    let stops: [[f32; 3]; 4] = if dark {
        [
            [14.0, 17.0, 23.0],
            [30.0, 80.0, 160.0],
            [230.0, 160.0, 40.0],
            [255.0, 250.0, 235.0],
        ]
    } else {
        [
            [250.0, 250.0, 248.0],
            [140.0, 180.0, 230.0],
            [220.0, 120.0, 20.0],
            [60.0, 20.0, 0.0],
        ]
    };
    let x = t * 3.0;
    let i = (x.floor() as usize).min(2);
    let f = x - i as f32;
    let c = |k| (stops[i][k] + f * (stops[i + 1][k] - stops[i][k])) as u8;
    egui::Color32::from_rgb(c(0), c(1), c(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tone_shows_up_in_its_bin() {
        let rate = 8000;
        let mut w = Waterfall::new(rate);
        let tone: Vec<f32> = (0..rate)
            .map(|n| (2.0 * std::f32::consts::PI * 700.0 * n as f32 / rate as f32).sin())
            .collect();
        w.push(&tone);
        let row = &w.rows[0];
        let peak = row
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        let hz = (peak + w.lo) as f64 * rate as f64 / w.size as f64;
        assert!((hz - 700.0).abs() < 15.0, "{hz}");
        assert_eq!(w.image(true, false).size, [w.hi - w.lo, ROWS]);
        assert_eq!(w.image_vertical(false, true).size, [ROWS, w.hi - w.lo]);
    }

    #[test]
    fn the_newest_column_faces_the_lanes() {
        let rate = 8000;
        let mut w = Waterfall::new(rate);
        w.push(&vec![0.0; w.size]);
        let tone: Vec<f32> = (0..w.hop)
            .map(|n| (2.0 * std::f32::consts::PI * 700.0 * n as f32 / rate as f32).sin())
            .collect();
        w.push(&tone);
        assert_eq!(w.rows.len(), 2);
        let bin = ((700.0 / (rate as f64 / w.size as f64)) as usize) - w.lo;
        let at = |img: &egui::ColorImage, x: usize| img.pixels[bin * ROWS + x];
        let blank = colour(0.0, true);
        let left = w.image_vertical(true, true);
        assert_ne!(at(&left, ROWS - 1), blank);
        assert_eq!(at(&left, 0), blank);
        let right = w.image_vertical(true, false);
        assert_ne!(at(&right, 0), blank);
        assert_eq!(at(&right, ROWS - 1), blank);
        let row = |img: &egui::ColorImage, y: usize| img.pixels[y * (w.hi - w.lo) + bin];
        let top = w.image(true, false);
        assert_ne!(row(&top, 0), blank);
        assert_eq!(row(&top, ROWS - 1), blank);
        let bottom = w.image(true, true);
        assert_ne!(row(&bottom, ROWS - 1), blank);
        assert_eq!(row(&bottom, 0), blank);
    }
}
