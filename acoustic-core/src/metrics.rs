//! Acoustic metrics: reverberation time (RT60 via T20/T30), Early Decay Time
//! (EDT), clarity (C50/C80), Initial Time Delay Gap (ITDG), and per-band decay
//! rates derived from a ray-tracing energy histogram.

/// Linear regression `y = a + b·x`; returns `(slope b, intercept a)`.
pub fn linear_regression(x: &[f64], y: &[f64]) -> (f64, f64) {
    let n = x.len().min(y.len());
    if n < 2 {
        return (0.0, 0.0);
    }
    let sx: f64 = x.iter().sum();
    let sy: f64 = y.iter().sum();
    let sxx: f64 = x.iter().map(|v| v * v).sum();
    let sxy: f64 = x.iter().zip(y.iter()).map(|(a, b)| a * b).sum();
    let denom = n as f64 * sxx - sx * sx;
    if denom.abs() < 1e-12 {
        return (0.0, 0.0);
    }
    let slope = (n as f64 * sxy - sx * sy) / denom;
    let intercept = (sy - slope * sx) / n as f64;
    (slope, intercept)
}

/// Schroeder backward-integrated energy decay curve in dB (0 dB at the peak).
pub fn schroeder_edc_db(h: &[f64]) -> Vec<f64> {
    let n = h.len();
    let mut edc = vec![0.0_f64; n];
    let mut acc = 0.0_f64;
    for i in (0..n).rev() {
        acc += h[i] * h[i];
        edc[i] = acc;
    }
    let peak = edc.first().copied().unwrap_or(1.0).max(1e-20);
    edc.iter().map(|&e| 10.0 * (e / peak).max(1e-12).log10()).collect()
}

/// RT60 from an energy decay curve using the `-5 … -35 dB` (T30) and
/// `-5 … -25 dB` (T20) segments, plus EDT from `0 … -10 dB`.
/// Returns `(rt60_t30, rt60_t20, edt)` in seconds (or `∞` if the decay is too
/// short to measure).
pub fn rt60_t20_t30_edt(edc_db: &[f64], sample_rate: f64) -> (f64, f64, f64) {
    let n = edc_db.len();
    let to_t = |idx: usize| idx as f64 / sample_rate;

    let first_below = |db: f64| -> Option<usize> {
        edc_db.iter().position(|&v| v <= db)
    };

    let fit_segment = |lo: f64, hi: f64| -> Option<f64> {
        let start = first_below(lo)?;
        let end = first_below(hi)?;
        if end <= start {
            return None;
        }
        let xs: Vec<f64> = (start..=end).map(to_t).collect();
        let ys: Vec<f64> = (start..=end).map(|i| edc_db[i]).collect();
        let (slope, _) = linear_regression(&xs, &ys);
        if slope >= 0.0 {
            None
        } else {
            Some(-60.0 / slope)
        }
    };

    // EDT: 0 … -10 dB → extrapolate to 60 dB (×6).
    let edt = {
        let start = first_below(0.0).unwrap_or(0);
        let end = first_below(-10.0);
        match end {
            Some(e) if e > start => {
                let xs: Vec<f64> = (start..=e).map(to_t).collect();
                let ys: Vec<f64> = (start..=e).map(|i| edc_db[i]).collect();
                let (slope, _) = linear_regression(&xs, &ys);
                if slope < 0.0 { -60.0 / slope } else { f64::INFINITY }
            }
            _ => f64::INFINITY,
        }
    };

    let t20 = fit_segment(-5.0, -25.0).unwrap_or(f64::INFINITY);
    let t30 = fit_segment(-5.0, -35.0).unwrap_or(f64::INFINITY);
    let _ = n;
    (t30, t20, edt)
}

/// Clarity indices C50 and C80 (dB) from an impulse response.
pub fn clarity(h: &[f64], sample_rate: f64) -> (f64, f64) {
    let e = |t0: usize, t1: usize| -> f64 {
        h[t0..t1.min(h.len())].iter().map(|v| v * v).sum()
    };
    let c50 = 10.0 * (e(0, (0.050 * sample_rate) as usize) / e((0.050 * sample_rate) as usize, h.len()).max(1e-20)).log10();
    let c80 = 10.0 * (e(0, (0.080 * sample_rate) as usize) / e((0.080 * sample_rate) as usize, h.len()).max(1e-20)).log10();
    (c50, c80)
}

/// Per-band RT60 from a ray-tracing energy histogram (Schroeder integration +
/// linear regression over `-5 … -35 dB`).
pub fn band_rt60_from_histogram(hist: &[[f64; 7]], bin_duration: f64) -> [f64; 7] {
    let n = hist.len();
    let mut out = [f64::INFINITY; 7];
    if n == 0 {
        return out;
    }
    for b in 0..7 {
        // Energy per band over time.
        let energy: Vec<f64> = hist.iter().map(|bin| bin[b]).collect();
        // Schroeder integrate.
        let mut edc = vec![0.0_f64; n];
        let mut acc = 0.0;
        for i in (0..n).rev() {
            acc += energy[i];
            edc[i] = acc;
        }
        let peak = edc[0].max(1e-20);
        let edc_db: Vec<f64> = edc.iter().map(|&e| 10.0 * (e / peak).max(1e-12).log10()).collect();
        let first_below = |db: f64| edc_db.iter().position(|&v| v <= db);
        if let (Some(start), Some(end)) = (first_below(-5.0), first_below(-35.0)) {
            if end > start {
                let xs: Vec<f64> = (start..=end).map(|i| i as f64 * bin_duration).collect();
                let ys: Vec<f64> = (start..=end).map(|i| edc_db[i]).collect();
                let (slope, _) = linear_regression(&xs, &ys);
                if slope < 0.0 {
                    out[b] = -60.0 / slope;
                }
            }
        }
    }
    out
}

/// ITDG in seconds (0 when there is no measurable first reflection).
pub fn itdg(direct_time: f64, first_reflection_time: f64) -> f64 {
    (first_reflection_time - direct_time).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_regression_perfect_fit() {
        let x = [0.0, 1.0, 2.0, 3.0];
        let y = [1.0, 3.0, 5.0, 7.0]; // y = 1 + 2x
        let (s, i) = linear_regression(&x, &y);
        assert!((s - 2.0).abs() < 1e-12);
        assert!((i - 1.0).abs() < 1e-12);
    }

    #[test]
    fn exponential_decay_gives_rt60() {
        // Synthetic exponentially decaying noise, RT60 = 0.5 s.
        let sr = 48000.0;
        let rt60 = 0.5;
        let decay = 6.9078 / rt60;
        let n = (sr * 1.0) as usize;
        let h: Vec<f64> = (0..n).map(|i| {
            let t = i as f64 / sr;
            (-decay * t).exp() * ((i as f64 * 0.7).sin() * 0.5 + 0.5)
        }).collect();
        let edc = schroeder_edc_db(&h);
        let (t30, _t20, _edt) = rt60_t20_t30_edt(&edc, sr);
        assert!(t30.is_finite());
        assert!((t30 - rt60).abs() < 0.08, "t30 = {t30}, expected ~{rt60}");
    }

    #[test]
    fn band_rt60_from_histogram_decays() {
        let n = 1000;
        let mut hist = vec![[0.0_f64; 7]; n];
        for b in 0..7 {
            let rt = 0.3 + b as f64 * 0.2;
            // The histogram stores *energy*, so a 60 dB energy decay over `rt`
            // seconds needs decay constant 60/10·ln(10) / rt ≈ 13.816 / rt.
            let decay = 13.8156 / rt;
            for (i, bin) in hist.iter_mut().enumerate() {
                bin[b] = (-decay * i as f64 * 0.002).exp();
            }
        }
        let rt = band_rt60_from_histogram(&hist, 0.002);
        for b in 0..7 {
            let expect = 0.3 + b as f64 * 0.2;
            assert!((rt[b] - expect).abs() < 0.12, "band {b}: {} vs {expect}", rt[b]);
        }
    }
}
