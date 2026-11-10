//! Small DSP building blocks shared by the analyzer and the demo synth.

use std::f32::consts::PI;

/// RBJ biquad, transposed direct form II.
#[derive(Clone, Debug)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterKind {
    LowPass,
    HighPass,
    /// Band-pass with 0 dB peak gain.
    BandPass,
}

impl Biquad {
    pub fn new(kind: FilterKind, sample_rate: f32, freq: f32, q: f32) -> Self {
        let mut b = Biquad {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        };
        b.set(kind, sample_rate, freq, q);
        b
    }

    /// Recomputes coefficients, keeping the filter state (for sweeps).
    pub fn set(&mut self, kind: FilterKind, sample_rate: f32, freq: f32, q: f32) {
        let f = freq.clamp(10.0, sample_rate * 0.45);
        let w0 = 2.0 * PI * f / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q.max(0.05));
        let a0 = 1.0 + alpha;
        let (b0, b1, b2) = match kind {
            FilterKind::LowPass => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0),
            FilterKind::HighPass => ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0),
            FilterKind::BandPass => (alpha, 0.0, -alpha),
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = -2.0 * cos / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// Exponential smoothing factor for a rate (1/s) over `dt` seconds.
#[inline]
pub fn smooth_k(rate: f32, dt: f32) -> f32 {
    1.0 - (-rate * dt).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms_through(kind: FilterKind, cutoff: f32, tone: f32) -> f32 {
        let sr = 48_000.0;
        let mut f = Biquad::new(kind, sr, cutoff, 0.707);
        let mut acc = 0.0;
        let n = 48_000;
        for i in 0..n {
            let x = (2.0 * PI * tone * i as f32 / sr).sin();
            let y = f.process(x);
            if i > n / 2 {
                acc += y * y;
            }
        }
        (acc / (n / 2) as f32).sqrt()
    }

    #[test]
    fn lowpass_passes_lows_and_cuts_highs() {
        assert!(rms_through(FilterKind::LowPass, 150.0, 50.0) > 0.6);
        assert!(rms_through(FilterKind::LowPass, 150.0, 4_000.0) < 0.01);
    }

    #[test]
    fn highpass_passes_highs_and_cuts_lows() {
        assert!(rms_through(FilterKind::HighPass, 2_000.0, 8_000.0) > 0.6);
        assert!(rms_through(FilterKind::HighPass, 2_000.0, 60.0) < 0.01);
    }

    #[test]
    fn bandpass_peaks_at_center() {
        let center = rms_through(FilterKind::BandPass, 1_000.0, 1_000.0);
        let off = rms_through(FilterKind::BandPass, 1_000.0, 100.0);
        assert!(center > 0.6 && off < center * 0.3, "{center} {off}");
    }
}
