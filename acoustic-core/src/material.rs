//! Octave-band material model.
//!
//! All frequency-dependent surface properties are expressed over the seven
//! standard octave bands `125 Hz … 8 kHz`. Absorption, scattering and
//! transmission are all clamped to `[0, 1]` on construction so a bad JSON
//! blueprint can never produce non-physical (out-of-range) coefficients.

use serde::{Deserialize, Serialize};

/// Octave-band centre frequencies (Hz).
pub const OCTAVE_BANDS_HZ: [f64; 7] = [125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0];
/// Number of octave bands.
pub const N_BANDS: usize = 7;

fn clamp01(x: f64) -> f64 {
    if x.is_nan() {
        0.0
    } else {
        x.clamp(0.0, 1.0)
    }
}

/// A surface material with 7-band absorption / scattering / transmission.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// Random-incidence absorption coefficient per band `α ∈ [0, 1]`.
    pub absorption: [f64; 7],
    /// Scattering coefficient per band `s ∈ [0, 1]` (0 = pure specular,
    /// 1 = fully diffuse).
    pub scattering: [f64; 7],
    /// Transmission (leakage) coefficient per band `t ∈ [0, 1]`.
    pub transmission: [f64; 7],
}

impl Default for Material {
    fn default() -> Self {
        Material {
            name: "drywall".into(),
            absorption: [0.08, 0.09, 0.10, 0.08, 0.06, 0.05, 0.04],
            scattering: [0.1; 7],
            transmission: [0.0; 7],
        }
    }
}

impl Material {
    /// Sanitised constructor: every coefficient is clamped to `[0, 1]` and NaN
    /// is treated as `0`.
    pub fn sanitized(name: String, absorption: [f64; 7], scattering: [f64; 7], transmission: [f64; 7]) -> Self {
        let mut a = [0.0; 7];
        let mut s = [0.0; 7];
        let mut t = [0.0; 7];
        for i in 0..7 {
            a[i] = clamp01(absorption[i]);
            s[i] = clamp01(scattering[i]);
            t[i] = clamp01(transmission[i]);
        }
        Material { name, absorption: a, scattering: s, transmission: t }
    }

    /// Mean absorption across bands (broadband).
    pub fn mean_absorption(&self) -> f64 {
        self.absorption.iter().sum::<f64>() / N_BANDS as f64
    }

    /// Mean scattering across bands.
    pub fn mean_scattering(&self) -> f64 {
        self.scattering.iter().sum::<f64>() / N_BANDS as f64
    }
}

/// Index of the octave band containing `freq` (clamped to the valid range).
pub fn band_index_for_freq(freq: f64) -> usize {
    let f = freq.max(OCTAVE_BANDS_HZ[0]).min(OCTAVE_BANDS_HZ[N_BANDS - 1]);
    // log2 distance to the closest band centre.
    let mut best = 0;
    let mut best_d = f64::INFINITY;
    for (i, &c) in OCTAVE_BANDS_HZ.iter().enumerate() {
        let d = (f / c).ln().abs();
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    best
}

/// Built-in material presets.
pub fn preset(name: &str) -> Material {
    // (name, [absorption], [scattering], [transmission])
    macro_rules! mat {
        ($n:expr, $a:expr, $s:expr, $t:expr) => {
            Material::sanitized($n.into(), $a, $s, $t)
        };
    }
    match name {
        "concrete" => mat!("concrete",
            [0.01, 0.01, 0.02, 0.02, 0.02, 0.03, 0.03],
            [0.05, 0.05, 0.05, 0.05, 0.05, 0.05, 0.05],
            [0.0; 7]),
        "brick" => mat!("brick",
            [0.03, 0.03, 0.03, 0.04, 0.05, 0.07, 0.07],
            [0.1; 7],
            [0.0; 7]),
        "drywall" => mat!("drywall",
            [0.08, 0.09, 0.10, 0.08, 0.06, 0.05, 0.04],
            [0.1; 7],
            [0.0; 7]),
        "glass" => mat!("glass",
            [0.18, 0.06, 0.04, 0.03, 0.02, 0.02, 0.02],
            [0.05; 7],
            [0.0; 7]),
        "hardwood" => mat!("hardwood",
            [0.15, 0.11, 0.10, 0.07, 0.06, 0.06, 0.05],
            [0.2; 7],
            [0.0; 7]),
        "carpet" => mat!("carpet",
            [0.08, 0.24, 0.57, 0.69, 0.71, 0.73, 0.73],
            [0.6; 7],
            [0.0; 7]),
        "mineral-wool" => mat!("mineral-wool",
            [0.16, 0.43, 0.74, 0.96, 1.00, 1.00, 1.00],
            [0.5; 7],
            [0.0; 7]),
        "diffuser" => mat!("diffuser",
            [0.10, 0.10, 0.12, 0.14, 0.16, 0.18, 0.20],
            [0.9, 0.9, 0.9, 0.9, 0.9, 0.9, 0.9],
            [0.0; 7]),
        "bass-trap" => mat!("bass-trap",
            [0.90, 0.95, 0.98, 0.99, 1.00, 1.00, 1.00],
            [0.5; 7],
            [0.0; 7]),
        "door" => mat!("door",
            [0.10, 0.12, 0.14, 0.16, 0.18, 0.20, 0.20],
            [0.1; 7],
            [0.35, 0.35, 0.35, 0.35, 0.35, 0.35, 0.35]),
        "open-aperture" => mat!("open-aperture",
            [0.05; 7],
            [0.0; 7],
            [1.0; 7]),
        _ => Material::default(),
    }
}

/// All preset names (for the UI material library).
pub fn preset_names() -> Vec<&'static str> {
    vec![
        "concrete", "brick", "drywall", "glass", "hardwood", "carpet",
        "mineral-wool", "diffuser", "bass-trap", "door", "open-aperture",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_index_clamps_and_maps() {
        assert_eq!(band_index_for_freq(1000.0), 3);
        assert_eq!(band_index_for_freq(20.0), 0);
        assert_eq!(band_index_for_freq(20000.0), 6);
        assert_eq!(band_index_for_freq(8000.0), 6);
    }

    #[test]
    fn sanitized_clamps() {
        let m = Material::sanitized(
            "x".into(),
            [2.0, -1.0, f64::NAN, 0.5, 0.5, 0.5, 0.5],
            [0.5; 7],
            [0.0; 7],
        );
        assert_eq!(m.absorption[0], 1.0);
        assert_eq!(m.absorption[1], 0.0);
        assert_eq!(m.absorption[2], 0.0);
        assert_eq!(m.absorption[3], 0.5);
    }

    #[test]
    fn presets_exist() {
        for name in preset_names() {
            let m = preset(name);
            assert!(m.absorption.iter().all(|&a| (0.0..=1.0).contains(&a)));
            assert!(m.transmission.iter().all(|&t| (0.0..=1.0).contains(&t)));
        }
    }
}
