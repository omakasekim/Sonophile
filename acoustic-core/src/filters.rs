//! Lightweight IIR filters (biquads) used by the RIR synthesiser: octave-band
//! band-passes for the late-reverb tail, and 4th-order Linkwitz–Riley crossover
//! filters for the low-frequency modal / high-frequency geometric blend.

/// Transposed direct-form-II biquad.
#[derive(Clone, Debug)]
pub struct Biquad {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    pub fn new(b0: f64, b1: f64, b2: f64, a1: f64, a2: f64) -> Self {
        Biquad { b0, b1, b2, a1, a2, z1: 0.0, z2: 0.0 }
    }

    /// RBJ-cookbook coefficients.
    fn rbj(f: f64, sr: f64, q: f64, kind: &str) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * (f.clamp(1.0, sr / 2.0 - 1.0)) / sr;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q.max(1e-3));
        match kind {
            "lowpass" => {
                let b0 = (1.0 - cw) / 2.0;
                let b1 = 1.0 - cw;
                let b2 = (1.0 - cw) / 2.0;
                let a0 = 1.0 + alpha;
                Biquad::new(b0 / a0, b1 / a0, b2 / a0, -2.0 * cw / a0, (1.0 - alpha) / a0)
            }
            "highpass" => {
                let b0 = (1.0 + cw) / 2.0;
                let b1 = -(1.0 + cw);
                let b2 = (1.0 + cw) / 2.0;
                let a0 = 1.0 + alpha;
                Biquad::new(b0 / a0, b1 / a0, b2 / a0, -2.0 * cw / a0, (1.0 - alpha) / a0)
            }
            _ => {
                // bandpass (constant 0 dB peak gain)
                let b0 = alpha;
                let b1 = 0.0;
                let b2 = -alpha;
                let a0 = 1.0 + alpha;
                Biquad::new(b0 / a0, b1 / a0, b2 / a0, -2.0 * cw / a0, (1.0 - alpha) / a0)
            }
        }
    }

    pub fn lowpass(f: f64, sr: f64, q: f64) -> Self {
        Self::rbj(f, sr, q, "lowpass")
    }
    pub fn highpass(f: f64, sr: f64, q: f64) -> Self {
        Self::rbj(f, sr, q, "highpass")
    }
    pub fn bandpass(f: f64, sr: f64, q: f64) -> Self {
        Self::rbj(f, sr, q, "bandpass")
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// 4th-order Linkwitz–Riley low-pass (two cascaded 2nd-order Butterworth).
pub fn lr4_lowpass_apply(buf: &mut [f64], fc: f64, sr: f64) {
    let mut f1 = Biquad::lowpass(fc, sr, 0.7071);
    let mut f2 = Biquad::lowpass(fc, sr, 0.7071);
    for s in buf.iter_mut() {
        *s = f2.process(f1.process(*s));
    }
}

/// 4th-order Linkwitz–Riley high-pass.
pub fn lr4_highpass_apply(buf: &mut [f64], fc: f64, sr: f64) {
    let mut f1 = Biquad::highpass(fc, sr, 0.7071);
    let mut f2 = Biquad::highpass(fc, sr, 0.7071);
    for s in buf.iter_mut() {
        *s = f2.process(f1.process(*s));
    }
}

/// Generate a band-limited noise tail with an exponential decay `e^(−t·6.9078/RT60)`
/// for a single octave band and accumulate it into `out`.
pub fn add_band_noise_tail(
    out: &mut [f64],
    center: f64,
    rt60: f64,
    sr: f64,
    rng: &mut crate::prng::Rng,
) {
    let mut bp = Biquad::bandpass(center, sr, 1.41);
    let decay = if rt60.is_finite() && rt60 > 0.001 {
        6.9078 / rt60
    } else {
        0.0
    };
    for (n, s) in out.iter_mut().enumerate() {
        // White noise.
        let noise = rng.next_f64() * 2.0 - 1.0;
        let filtered = bp.process(noise);
        let t = n as f64 / sr;
        *s += filtered * (-decay * t).exp();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prng::Rng;

    #[test]
    fn lowpass_attenuates_high_freq() {
        let sr = 48000.0;
        let mut dc = vec![1.0; 1000];
        lr4_lowpass_apply(&mut dc, 200.0, sr);
        // DC passes essentially unchanged.
        assert!((dc[999] - 1.0).abs() < 0.01);
        let mut hf = vec![0.0; 1000];
        for (i, s) in hf.iter_mut().enumerate() {
            *s = (2.0 * std::f64::consts::PI * 10000.0 * i as f64 / sr).sin();
        }
        lr4_lowpass_apply(&mut hf, 200.0, sr);
        let rms: f64 = (hf.iter().map(|&v| v * v).sum::<f64>() / 1000.0).sqrt();
        assert!(rms < 0.1, "high frequency leaked: {rms}");
    }

    #[test]
    fn noise_tail_decays() {
        let sr = 48000.0;
        let n = 48000;
        let mut buf = vec![0.0; n];
        add_band_noise_tail(&mut buf, 1000.0, 0.5, sr, &mut Rng::new(1));
        let early: f64 = buf[..4800].iter().map(|v| v.abs()).sum();
        let late: f64 = buf[n - 4800..].iter().map(|v| v.abs()).sum();
        assert!(early > late);
    }
}
