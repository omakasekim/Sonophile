//! Hybrid room impulse response (RIR) synthesis: exact early specular arrivals
//! (image source) + stochastic late-energy tail (7-band filtered noise with
//! per-band decay) + low-frequency FDTD modal response, combined through a
//! 4th-order Linkwitz–Riley crossover. Produces mono, stereo and a simplified
//! binaural (spherical-head ITD/ILD) pair.

use std::f64::consts::PI;

use crate::filters::{add_band_noise_tail, lr4_highpass_apply, lr4_lowpass_apply};
use crate::geom::Vec3;
use crate::image_source::EarlyReflection;
use crate::material::{Material, OCTAVE_BANDS_HZ};
use crate::metrics::band_rt60_from_histogram;
use crate::prng::Rng;
use crate::ray_tracer::RayTraceResult;
use crate::room::RoomGeometry;

pub const SPEED_OF_SOUND: f64 = 343.0;

/// RIR synthesis options.
#[derive(Clone, Debug)]
pub struct RirOptions {
    pub sample_rate: f64,
    pub decay_length: f64,
    pub crossover_freq: f64,
    pub seed: u64,
    /// Ratio of total late-tail energy to total early-reflection energy.
    pub late_energy_ratio: f64,
    pub normalize: bool,
    /// Listener head azimuth for the binaural pair (degrees).
    pub source_azimuth_deg: f64,
}

impl Default for RirOptions {
    fn default() -> Self {
        RirOptions {
            sample_rate: 48000.0,
            decay_length: 3.0,
            crossover_freq: 300.0,
            seed: 0xc0ffee,
            late_energy_ratio: 2.0,
            normalize: true,
            source_azimuth_deg: 0.0,
        }
    }
}

/// Synthesized multi-channel RIR.
#[derive(Clone, Debug)]
pub struct RirBuffers {
    pub sample_rate: f64,
    pub length: usize,
    pub mono: Vec<f32>,
    pub stereo_l: Vec<f32>,
    pub stereo_r: Vec<f32>,
    pub binaural_l: Vec<f32>,
    pub binaural_r: Vec<f32>,
}

/// Equal-power stereo pan gains from a unit arrival direction (horizontal
/// plane, `+x` = front).
fn pan_gains(dir: Vec3) -> (f64, f64) {
    let az = dir.y.atan2(dir.x);
    let phi = (az + PI / 2.0) * 0.5;
    (phi.cos(), phi.sin())
}

/// Spherical-head binaural parameters: `(delay_l, delay_r, gain_l, gain_r)`
/// (delays in samples).
fn binaural_params(az_deg: f64, sr: f64) -> (f64, f64, f64, f64) {
    let az = az_deg.to_radians().clamp(-PI / 2.0, PI / 2.0);
    let head_r = 0.0875; // m
    let itd = head_r / SPEED_OF_SOUND * (az + az.sin()); // s (Woodworth)
    let g_contra = 0.5 + 0.5 * az.cos();
    if az >= 0.0 {
        // Source to the right: right ear ipsilateral.
        (itd * sr, 0.0, g_contra, 1.0)
    } else {
        (0.0, -itd * sr, 1.0, g_contra)
    }
}

/// Linear-interpolation fractional delay.
fn apply_delay(buf: &[f64], delay_samples: f64) -> Vec<f64> {
    let n = buf.len();
    let mut out = vec![0.0; n];
    let i0 = delay_samples.floor() as isize;
    let frac = delay_samples - i0 as f64;
    for k in 0..n {
        let idx = k as isize - i0;
        let a = if idx >= 0 && (idx as usize) < n { buf[idx as usize] } else { 0.0 };
        let b = if idx + 1 >= 0 && ((idx + 1) as usize) < n { buf[(idx + 1) as usize] } else { 0.0 };
        out[k] = a + (b - a) * frac;
    }
    out
}

/// Linear resampling.
fn resample_linear(src: &[f64], src_rate: f64, dst_rate: f64, dst_len: usize) -> Vec<f64> {
    let mut out = vec![0.0; dst_len];
    if src.is_empty() || src_rate <= 0.0 {
        return out;
    }
    let step = src_rate / dst_rate;
    for (i, o) in out.iter_mut().enumerate() {
        let pos = i as f64 * step;
        let i0 = pos.floor() as usize;
        let frac = pos - i0 as f64;
        let a = src.get(i0).copied().unwrap_or(0.0);
        let b = src.get(i0 + 1).copied().unwrap_or(a);
        *o = a + (b - a) * frac;
    }
    out
}

/// Add a one-pole low-passed impulse (≈ `amp · lp^k`) starting at time `t`.
fn add_lp_impulse(buf: &mut [f64], sr: f64, t: f64, amp: f64, fc: f64) {
    let idx = (t * sr).round() as isize;
    if idx < 0 || idx as usize >= buf.len() || amp.abs() < 1e-12 {
        return;
    }
    let lp = (-2.0 * PI * fc / sr).exp().clamp(0.0, 0.999);
    let mut v = amp;
    let mut k = 0isize;
    while k < 128 && idx + k < buf.len() as isize {
        if idx + k >= 0 {
            buf[(idx + k) as usize] += v;
        }
        v *= lp;
        k += 1;
        if v.abs() < 1e-9 {
            break;
        }
    }
}

/// Synthesize the RIR from the three hybrid components.
///
/// * `early`: exact specular arrivals from the image-source solver.
/// * `hist`: stochastic ray-traced energy histogram (late field).
/// * `fdtd_low`: recorded low-frequency pressure at the listener (one sample
///   per FDTD step, at rate `1/fdtd_dt`).
#[allow(clippy::too_many_arguments)]
pub fn synthesize_rir(
    room: &RoomGeometry,
    materials: &[Material],
    source: Vec3,
    listener: Vec3,
    early: &[EarlyReflection],
    hist: &RayTraceResult,
    fdtd_low: &[f64],
    fdtd_dt: f64,
    opts: &RirOptions,
) -> RirBuffers {
    let sr = opts.sample_rate;
    let n = ((sr * opts.decay_length).round() as usize).max(1);
    let c = SPEED_OF_SOUND;

    // --- Early + direct (mono and panned) ---
    let mut early_mono = vec![0.0; n];
    let mut early_l = vec![0.0; n];
    let mut early_r = vec![0.0; n];

    let direct_dist = source.sub(listener).len().max(0.1);
    let direct_t = direct_dist / c;
    let direct_amp = 1.0 / direct_dist;
    let (dl, dr) = pan_gains(source.sub(listener).normalized());
    add_lp_impulse(&mut early_mono, sr, direct_t, direct_amp, 18000.0);
    add_lp_impulse(&mut early_l, sr, direct_t, direct_amp * dl, 18000.0);
    add_lp_impulse(&mut early_r, sr, direct_t, direct_amp * dr, 18000.0);

    let mut early_energy = direct_amp * direct_amp;
    for e in early {
        if e.time * sr >= n as f64 {
            continue;
        }
        let mean_amp = e.amplitude.iter().sum::<f64>() / 7.0;
        let brightness = (e.amplitude[6] / e.amplitude[0].max(1e-9)).clamp(0.0, 1.0);
        let fc = 1500.0 + 16500.0 * brightness;
        let (el, er) = pan_gains(e.direction);
        add_lp_impulse(&mut early_mono, sr, e.time, mean_amp, fc);
        add_lp_impulse(&mut early_l, sr, e.time, mean_amp * el, fc);
        add_lp_impulse(&mut early_r, sr, e.time, mean_amp * er, fc);
        early_energy += mean_amp * mean_amp;
    }

    // --- Late tail (decorrelated stereo noise, per-band decay) ---
    let band_rt = band_rt60_from_histogram(&hist.histogram, hist.bin_duration);
    let fallback_rt = 0.8;
    let mut late_a = vec![0.0; n];
    let mut late_b = vec![0.0; n];
    for b in 0..7 {
        let rt = if band_rt[b].is_finite() && band_rt[b] > 0.01 {
            band_rt[b]
        } else {
            fallback_rt
        };
        let mut rng_a = Rng::new(opts.seed ^ (0x9e37_79b9_u64.wrapping_mul(b as u64 + 1)));
        let mut rng_b = Rng::new(opts.seed ^ (0xbf58_476d_u64.wrapping_mul(b as u64 + 1)));
        add_band_noise_tail(&mut late_a, OCTAVE_BANDS_HZ[b], rt, sr, &mut rng_a);
        add_band_noise_tail(&mut late_b, OCTAVE_BANDS_HZ[b], rt, sr, &mut rng_b);
    }
    // Calibrate late-tail energy against early energy.
    let late_raw_energy = late_a.iter().map(|v| v * v).sum::<f64>().max(1e-20);
    let target_late = early_energy * opts.late_energy_ratio;
    let late_scale = (target_late / late_raw_energy).sqrt();
    for v in late_a.iter_mut() {
        *v *= late_scale;
    }
    for v in late_b.iter_mut() {
        *v *= late_scale;
    }

    // --- Modal (FDTD) low band ---
    let modal = resample_linear(fdtd_low, 1.0 / fdtd_dt.max(1e-9), sr, n);

    // --- Crossover combine ---
    let combine = |early: &mut [f64], late: &[f64], low: &[f64]| -> Vec<f64> {
        let mut high = vec![0.0; n];
        for i in 0..n {
            high[i] = early[i] + late[i];
        }
        let mut low = low.to_vec();
        lr4_highpass_apply(&mut high, opts.crossover_freq, sr);
        lr4_lowpass_apply(&mut low, opts.crossover_freq, sr);
        for i in 0..n {
            high[i] += low[i];
        }
        high
    };

    let mono = combine(&mut early_mono, &late_a, &modal);
    let stereo_l = combine(&mut early_l, &late_a, &modal);
    let stereo_r = combine(&mut early_r, &late_b, &modal);

    // --- Binaural (spherical-head ITD/ILD applied to mono) ---
    let (dl_s, dr_s, gl, gr) = binaural_params(opts.source_azimuth_deg, sr);
    let mut bin_l = apply_delay(&mono, dl_s);
    let mut bin_r = apply_delay(&mono, dr_s);
    for v in bin_l.iter_mut() {
        *v *= gl;
    }
    for v in bin_r.iter_mut() {
        *v *= gr;
    }

    // --- Normalize ---
    let mut mono = mono;
    let mut sl = stereo_l;
    let mut sr_b = stereo_r;
    let mut bl = bin_l;
    let mut br = bin_r;
    if opts.normalize {
        let peak = mono
            .iter()
            .chain(sl.iter())
            .chain(sr_b.iter())
            .chain(bl.iter())
            .chain(br.iter())
            .fold(0.0_f64, |a, &v| a.max(v.abs()))
            .max(1e-9);
        let k = 0.98 / peak;
        for buf in [&mut mono, &mut sl, &mut sr_b, &mut bl, &mut br] {
            for v in buf.iter_mut() {
                *v *= k;
            }
        }
    }

    let _ = (room, materials);
    RirBuffers {
        sample_rate: sr,
        length: n,
        mono: mono.iter().map(|&v| v as f32).collect(),
        stereo_l: sl.iter().map(|&v| v as f32).collect(),
        stereo_r: sr_b.iter().map(|&v| v as f32).collect(),
        binaural_l: bl.iter().map(|&v| v as f32).collect(),
        binaural_r: br.iter().map(|&v| v as f32).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pan_gains_center_and_sides() {
        let (l, r) = pan_gains(Vec3::new(1.0, 0.0, 0.0));
        assert!((l - r).abs() < 1e-9); // front = center
        let (l, r) = pan_gains(Vec3::new(0.0, 1.0, 0.0));
        assert!(l < 0.01 && r > 0.99); // right
    }

    #[test]
    fn delay_is_lossless_shift() {
        let sig = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let d = apply_delay(&sig, 1.0);
        assert!((d[1] - 1.0).abs() < 1e-12);
        assert!((d[4] - 4.0).abs() < 1e-12);
    }

    #[test]
    fn resample_preserves_dc() {
        let src = vec![1.0; 100];
        let out = resample_linear(&src, 100.0, 200.0, 200);
        assert!(out.iter().all(|&v| (v - 1.0).abs() < 1e-12));
    }

    #[test]
    fn binaural_places_right_source_right() {
        let (dl, dr, gl, gr) = binaural_params(90.0, 48000.0);
        assert!(dl > dr); // right source → left ear delayed
        assert!(gl < gr); // left (contralateral) ear attenuated
    }
}
