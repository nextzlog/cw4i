//! Minimal complex number type (f64).

use std::ops::{Add, AddAssign, Div, Mul, Sub, SubAssign};

/// Complex number with f64 parts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct C64 {
    pub re: f64,
    pub im: f64,
}

impl C64 {
    pub const ZERO: C64 = C64 { re: 0.0, im: 0.0 };

    #[inline]
    pub const fn new(re: f64, im: f64) -> C64 {
        C64 { re, im }
    }

    /// exp(i theta).
    #[inline]
    pub fn cis(theta: f64) -> C64 {
        let (s, c) = theta.sin_cos();
        C64 { re: c, im: s }
    }

    #[inline]
    pub fn conj(self) -> C64 {
        C64 {
            re: self.re,
            im: -self.im,
        }
    }

    #[inline]
    pub fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }

    #[inline]
    pub fn arg(self) -> f64 {
        self.im.atan2(self.re)
    }

    #[inline]
    pub fn is_zero(self) -> bool {
        self.re == 0.0 && self.im == 0.0
    }
}

impl Add for C64 {
    type Output = C64;
    #[inline]
    fn add(self, o: C64) -> C64 {
        C64::new(self.re + o.re, self.im + o.im)
    }
}

impl Sub for C64 {
    type Output = C64;
    #[inline]
    fn sub(self, o: C64) -> C64 {
        C64::new(self.re - o.re, self.im - o.im)
    }
}

impl AddAssign for C64 {
    #[inline]
    fn add_assign(&mut self, o: C64) {
        *self = *self + o;
    }
}

impl SubAssign for C64 {
    #[inline]
    fn sub_assign(&mut self, o: C64) {
        *self = *self - o;
    }
}

impl Mul for C64 {
    type Output = C64;
    #[inline]
    fn mul(self, o: C64) -> C64 {
        C64::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
}

impl Div for C64 {
    type Output = C64;
    /// Smith's algorithm (robust complex division).
    #[inline]
    fn div(self, m: C64) -> C64 {
        let (e, f);
        if m.re.abs() >= m.im.abs() {
            let ratio = m.im / m.re;
            let denom = m.re + ratio * m.im;
            e = (self.re + self.im * ratio) / denom;
            f = (self.im - self.re * ratio) / denom;
        } else {
            let ratio = m.re / m.im;
            let denom = m.im + ratio * m.re;
            e = (self.re * ratio + self.im) / denom;
            f = (self.im * ratio - self.re) / denom;
        }
        C64::new(e, f)
    }
}
