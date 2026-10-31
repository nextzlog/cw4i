//! Front end: audio to complex STFT frames on an absolute sample clock.

use crate::complex::C64;
use crate::fft::Fft;

/// Tabulated window spectrum K(d) = sum_n w[n] exp(-i 2 pi n d / M) for offsets d in bins.
pub struct Kernel {
    /// Support radius in bins.
    pub r: f64,
    step: f64,
    tab: Vec<C64>,
    /// Spectrum of a linear amplitude ramp across the window (keying transient).
    tab2: Vec<C64>,
    /// Sum of |K|^2 over integer offsets (energy of a line in the bins).
    pub norm: f64,
}

impl Kernel {
    #[inline]
    pub fn at(&self, d: f64) -> C64 {
        self.lookup(&self.tab, d)
    }

    #[inline]
    pub fn ramp(&self, d: f64) -> C64 {
        self.lookup(&self.tab2, d)
    }

    #[inline]
    fn lookup(&self, tab: &[C64], d: f64) -> C64 {
        if d <= -self.r || d >= self.r {
            return C64::ZERO;
        }
        let x = (d + self.r) / self.step;
        let i = x as usize;
        let fr = x - i as f64;
        if i + 1 >= tab.len() {
            return tab[tab.len() - 1];
        }
        tab[i] * C64::new(1.0 - fr, 0.0) + tab[i + 1] * C64::new(fr, 0.0)
    }
}

/// Analysis frames per second (5 ms hop).
const FRAMES_PER_SEC: usize = 200;
/// Kernel support radius in units of the window's bin spacing M / N (main lobe and first
/// sidelobes) and the tabulation step in bins.
const KERNEL_RADIUS: f64 = 4.0;
const KERNEL_STEP: f64 = 1.0 / 32.0;

pub struct Frontend {
    pub rate: usize,
    /// Window length in samples.
    pub n: usize,
    /// FFT size (zero padded).
    pub m: usize,
    pub hop: usize,
    /// Kept bin range [lo, hi).
    pub lo: usize,
    pub hi: usize,
    win: Vec<f64>,
    buf: Vec<f64>,
    off: usize,
    pub pos: usize,
    fft: Fft,
    work: Vec<C64>,
    pub kernel: Kernel,
}

impl Frontend {
    pub fn new(rate: usize, win_sec: f64, lo_hz: f64, hi_hz: f64) -> Frontend {
        let n = (win_sec * rate as f64) as usize;
        let mut m = 1;
        while m < 2 * n {
            m <<= 1;
        }
        let pi = std::f64::consts::PI;
        let win: Vec<f64> = (0..n)
            .map(|i| 0.5 - 0.5 * (2.0 * pi * (i as f64 + 0.5) / n as f64).cos())
            .collect();
        let lo = (lo_hz * m as f64 / rate as f64) as usize;
        let hi = (hi_hz * m as f64 / rate as f64) as usize + 1;
        // kernel support: main lobe (2 M/N bins) plus first sidelobes
        let r = KERNEL_RADIUS * m as f64 / n as f64;
        let step = KERNEL_STEP;
        let cnt = (2.0 * r / step) as usize + 2;
        let mut tab = vec![C64::ZERO; cnt];
        let mut tab2 = vec![C64::ZERO; cnt];
        for j in 0..cnt {
            let d = -r + j as f64 * step;
            let mut s = C64::ZERO;
            let mut s2 = C64::ZERO;
            for (i, &w) in win.iter().enumerate() {
                let e = C64::new(w, 0.0) * C64::cis(-2.0 * pi * i as f64 * d / m as f64);
                s += e;
                s2 += e * C64::new((i as f64 - (n as f64 - 1.0) / 2.0) / n as f64, 0.0);
            }
            tab[j] = s;
            tab2[j] = s2;
        }
        Frontend {
            rate,
            n,
            m,
            hop: rate / FRAMES_PER_SEC,
            lo,
            hi,
            win,
            buf: Vec::new(),
            off: 0,
            pos: 0,
            fft: Fft::new(m),
            work: vec![C64::ZERO; m],
            kernel: {
                let mut k = Kernel {
                    r,
                    step,
                    tab,
                    tab2,
                    norm: 0.0,
                };
                for d in -(r as i64)..=(r as i64) {
                    let v = k.at(d as f64);
                    k.norm += v.re * v.re + v.im * v.im;
                }
                k
            },
        }
    }

    /// Converts a fractional bin index to Hz.
    #[inline]
    pub fn bin_hz(&self, b: f64) -> f64 {
        b * self.rate as f64 / self.m as f64
    }

    #[inline]
    pub fn hz_bin(&self, f: f64) -> f64 {
        f * self.m as f64 / self.rate as f64
    }

    /// Appends samples to the internal buffer.
    pub fn feed(&mut self, x: impl Iterator<Item = f64>) {
        self.buf.extend(x);
    }

    /// Samples of the last computed frame that were not in the frame before.
    pub fn newest(&self) -> &[f64] {
        let end = self.off - self.hop + self.n;
        &self.buf[end - self.hop..end]
    }

    /// Computes the next complete frame (bins lo..hi-1) into `spec` and returns its start sample.
    pub fn next_frame(&mut self, spec: &mut Vec<C64>) -> Option<usize> {
        if self.off + self.n > self.buf.len() {
            // compact the consumed part of the buffer
            self.buf.drain(..self.off);
            self.pos += self.off;
            self.off = 0;
            return None;
        }
        let off = self.off;
        for i in 0..self.n {
            self.work[i] = C64::new(self.buf[off + i] * self.win[i], 0.0);
        }
        for w in &mut self.work[self.n..] {
            *w = C64::ZERO;
        }
        self.fft.transform(&mut self.work);
        spec.clear();
        spec.extend_from_slice(&self.work[self.lo..self.hi]);
        self.off += self.hop;
        Some(self.pos + off)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_at_common_rates() {
        let f = Frontend::new(8000, 0.040, 250.0, 1300.0);
        assert_eq!((f.n, f.m, f.hop, f.lo, f.hi), (320, 1024, 40, 32, 167));
        let f = Frontend::new(48000, 0.040, 250.0, 1300.0);
        assert_eq!((f.n, f.m, f.hop), (1920, 4096, 240));
        assert!((f.bin_hz(f.hz_bin(700.0)) - 700.0).abs() < 1e-9);
    }

    #[test]
    fn kernel_peak_and_support() {
        let f = Frontend::new(8000, 0.040, 250.0, 1300.0);
        let k = &f.kernel;
        // the window sum (Hann: n/2) at zero offset, symmetric magnitude, zero outside
        assert!((k.at(0.0).abs() - 160.0).abs() < 0.5); // table interpolation
        assert!((k.at(1.25).abs() - k.at(-1.25).abs()).abs() < 0.5);
        assert_eq!(k.at(k.r + 0.1), C64::ZERO);
        assert!(k.at(0.0).abs() > k.at(2.0).abs());
    }

    #[test]
    fn frames_follow_the_sample_clock() {
        let mut f = Frontend::new(8000, 0.040, 250.0, 1300.0);
        let mut spec = Vec::new();
        f.feed((0..1000).map(|i| (i as f64 * 0.55).sin()));
        let mut starts = Vec::new();
        while let Some(s) = f.next_frame(&mut spec) {
            starts.push(s);
            assert_eq!(spec.len(), f.hi - f.lo);
        }
        assert_eq!(
            starts,
            (0..=(1000 - 320) / 40).map(|i| i * 40).collect::<Vec<_>>()
        );
        f.feed((1000..1100).map(|i| (i as f64 * 0.55).sin()));
        assert_eq!(f.next_frame(&mut spec), Some(starts.last().unwrap() + 40));
    }
}
