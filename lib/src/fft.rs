//! In-place iterative radix-2 complex FFT with precomputed twiddles.

use crate::complex::C64;

pub struct Fft {
    n: usize,
    rev: Vec<usize>,
    tw: Vec<C64>,
}

impl Fft {
    pub fn new(n: usize) -> Fft {
        let mut bits = 0;
        while (1usize << bits) < n {
            bits += 1;
        }
        let rev = (0..n)
            .map(|i| {
                let mut r = 0;
                for b in 0..bits {
                    if i & (1 << b) != 0 {
                        r |= 1 << (bits - 1 - b);
                    }
                }
                r
            })
            .collect();
        let tw = (0..n / 2)
            .map(|i| C64::cis(-2.0 * std::f64::consts::PI * i as f64 / n as f64))
            .collect();
        Fft { n, rev, tw }
    }

    pub fn transform(&self, x: &mut [C64]) {
        let n = self.n;
        for (i, &r) in self.rev.iter().enumerate() {
            if i < r {
                x.swap(i, r);
            }
        }
        let mut size = 2;
        while size <= n {
            let half = size / 2;
            let step = n / size;
            let mut start = 0;
            while start < n {
                for k in 0..half {
                    let w = self.tw[k * step];
                    let a = x[start + k];
                    let b = x[start + k + half] * w;
                    x[start + k] = a + b;
                    x[start + k + half] = a - b;
                }
                start += size;
            }
            size <<= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_naive_dft() {
        let n = 64;
        let x: Vec<C64> = (0..n)
            .map(|i| C64::new((i as f64 * 0.37).sin(), (i as f64 * 0.11).cos() - 0.2))
            .collect();
        let mut y = x.clone();
        Fft::new(n).transform(&mut y);
        for (k, yk) in y.iter().enumerate() {
            let mut s = C64::ZERO;
            for (i, xi) in x.iter().enumerate() {
                s += *xi * C64::cis(-2.0 * std::f64::consts::PI * (i * k) as f64 / n as f64);
            }
            assert!((s - *yk).abs() < 1e-10, "bin {k}");
        }
    }

    #[test]
    fn pure_tone_lands_in_its_bin() {
        let n = 256;
        let mut x: Vec<C64> = (0..n)
            .map(|i| C64::cis(2.0 * std::f64::consts::PI * 10.0 * i as f64 / n as f64))
            .collect();
        Fft::new(n).transform(&mut x);
        assert!((x[10].abs() - n as f64).abs() < 1e-9);
        assert!(x.iter().enumerate().all(|(k, v)| k == 10 || v.abs() < 1e-9));
    }
}
