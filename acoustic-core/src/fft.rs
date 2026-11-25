//! Minimal radix-2 complex FFT and magnitude-spectrum helper used to compute
//! the simulated magnitude frequency response from a synthesized RIR.

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug)]
pub struct Cplx {
    pub re: f64,
    pub im: f64,
}

impl Cplx {
    #[inline]
    fn add(self, o: Cplx) -> Cplx {
        Cplx { re: self.re + o.re, im: self.im + o.im }
    }
    #[inline]
    fn sub(self, o: Cplx) -> Cplx {
        Cplx { re: self.re - o.re, im: self.im - o.im }
    }
    #[inline]
    fn mul(self, o: Cplx) -> Cplx {
        Cplx {
            re: self.re * o.re - self.im * o.im,
            im: self.re * o.im + self.im * o.re,
        }
    }
    #[inline]
    fn mag(self) -> f64 {
        (self.re * self.re + self.im * self.im).sqrt()
    }
}

/// In-place iterative radix-2 FFT (Cooley–Tukey). `x.len()` must be a power of
/// two (the caller normalises).
pub fn fft(x: &mut [Cplx]) {
    let n = x.len();
    if n <= 1 || !n.is_power_of_two() {
        return;
    }
    // Bit-reversal permutation.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            x.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let ang = -2.0 * PI / len as f64;
        let wlen = Cplx { re: ang.cos(), im: ang.sin() };
        for i in (0..n).step_by(len) {
            let mut w = Cplx { re: 1.0, im: 0.0 };
            for k in 0..len / 2 {
                let u = x[i + k];
                let v = x[i + k + len / 2].mul(w);
                x[i + k] = u.add(v);
                x[i + k + len / 2] = u.sub(v);
                w = w.mul(wlen);
            }
        }
        len <<= 1;
    }
}

/// Magnitude spectrum (dB) of a real signal, Hann-windowed and normalised.
/// Returns `(frequencies_hz, magnitude_db)`.
pub fn magnitude_spectrum_db(signal: &[f64], sample_rate: f64) -> (Vec<f64>, Vec<f64>) {
    // Next power of two (cap at 2^17 to bound cost).
    let n = signal.len().next_power_of_two().min(1 << 17);
    let mut buf: Vec<Cplx> = (0..n)
        .map(|i| {
            let s = if i < signal.len() { signal[i] } else { 0.0 };
            // Hann window.
            let w = 0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos();
            Cplx { re: s * w, im: 0.0 }
        })
        .collect();
    fft(&mut buf);
    let nyquist = n / 2;
    let freqs: Vec<f64> = (0..nyquist).map(|i| i as f64 * sample_rate / n as f64).collect();
    let mags: Vec<f64> = (0..nyquist)
        .map(|i| {
            let m = buf[i].mag() / n as f64;
            // Avoid log(0).
            20.0 * (m.max(1e-12)).log10()
        })
        .collect();
    (freqs, mags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_of_sine_peaks_at_frequency() {
        let sr = 48000.0;
        let n = 4096;
        let f = 1000.0;
        let sig: Vec<f64> = (0..n).map(|i| (2.0 * PI * f * i as f64 / sr).sin()).collect();
        let (freqs, mags) = magnitude_spectrum_db(&sig, sr);
        // Find peak bin.
        let peak = freqs
            .iter()
            .zip(mags.iter())
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(&fr, _)| fr)
            .unwrap();
        assert!((peak - f).abs() < sr / n as f64 * 2.0, "peak at {peak}, expected {f}");
    }
}
