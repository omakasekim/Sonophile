//! Stochastic ray tracer with per-octave-band energy tracking, scattering,
//! transmission leakage and air absorption.

use crate::geom::{reflect, Ray, Surface, Vec3};
use crate::material::Material;
use crate::prng::{fibonacci_sphere, Rng};
use crate::room::{air_absorption_db_per_m, RoomGeometry};

/// Ray-tracing configuration.
#[derive(Clone, Debug)]
pub struct RayOptions {
    pub ray_count: usize,
    pub max_bounces: usize,
    pub max_time: f64,
    pub n_bins: usize,
    pub temp_c: f64,
    pub humidity: f64,
    pub seed: u64,
    /// Maximum number of ray polylines returned for visualization.
    pub max_viz_rays: usize,
    /// Source directivity pattern (omnidirectional | cardioid | supercardioid | cone).
    pub directivity: String,
}

impl Default for RayOptions {
    fn default() -> Self {
        RayOptions {
            ray_count: 20_000,
            max_bounces: 40,
            max_time: 2.0,
            n_bins: 2048,
            temp_c: 20.0,
            humidity: 50.0,
            seed: 0x5eed_5eed,
            max_viz_rays: 400,
            directivity: "omnidirectional".into(),
        }
    }
}

/// Result of a stochastic ray trace.
#[derive(Clone, Debug)]
pub struct RayTraceResult {
    /// Mean energy per bin per band (`n_bins × 7`).
    pub histogram: Vec<[f64; 7]>,
    pub bin_duration: f64,
    pub ray_count: usize,
    /// Ray polylines (origin followed by hit points) for the visualizer.
    pub ray_paths: Vec<Vec<Vec3>>,
}

fn nearest_hit(ray: &Ray, surfaces: &[Surface]) -> Option<(usize, crate::geom::Hit)> {
    let mut best: Option<(usize, crate::geom::Hit)> = None;
    for (i, s) in surfaces.iter().enumerate() {
        if let Some(h) = s.intersect(ray, false) {
            if best.map_or(true, |(_, b)| h.t < b.t) {
                best = Some((i, h));
            }
        }
    }
    best
}

/// Run the stochastic ray tracer.
pub fn trace_rays(
    room: &RoomGeometry,
    source: Vec3,
    materials: &[Material],
    opts: &RayOptions,
) -> RayTraceResult {
    let surfaces = &room.surfaces;
    let bin_duration = opts.max_time / opts.n_bins.max(1) as f64;
    let mut hist = vec![[0.0_f64; 7]; opts.n_bins.max(1)];
    let mut ray_paths: Vec<Vec<Vec3>> = Vec::new();
    let air: Vec<f64> = (0..7)
        .map(|b| air_absorption_db_per_m(opts.temp_c, opts.humidity, b))
        .collect();

    let mut rng = Rng::new(opts.seed);
    let base_dirs = fibonacci_sphere(opts.ray_count);

    for (_r, &base) in base_dirs.iter().enumerate().take(opts.ray_count) {
        // Stratified: jitter each Fibonacci direction slightly.
        let jitter = Vec3::new(
            rng.range(-0.02, 0.02),
            rng.range(-0.02, 0.02),
            rng.range(-0.02, 0.02),
        );
        let dir = base.add(jitter).normalized();

        // Source directivity weights the initial energy per ray.
        let forward = Vec3::new(1.0, 0.0, 0.0);
        let dg = crate::geom::directivity_gain(&opts.directivity, dir, forward);
        let mut energy = [dg; 7];
        let mut time = 0.0;
        let mut origin = source;
        let mut dir = dir;
        let mut path: Vec<Vec3> = vec![source];

        for _bounce in 0..opts.max_bounces {
            if energy.iter().sum::<f64>() < 1e-7 || time >= opts.max_time {
                break;
            }
            let ray = Ray { origin, dir };
            let hit = match nearest_hit(&ray, surfaces) {
                Some(h) => h,
                None => break,
            };
            let (_si, h) = hit;
            let d = h.t;
            time += d / 343.0; // speed of sound (metres/s)

            if time >= opts.max_time {
                break;
            }

            // Air absorption + geometric spreading.
            let spread = 1.0 / (1.0 + (time * 343.0).powi(2));
            for b in 0..7 {
                energy[b] *= 10.0_f64.powf(-air[b] * d / 10.0);
            }

            // Deposit bounced energy into the histogram.
            let bin = ((time / bin_duration) as usize).min(hist.len() - 1);
            for b in 0..7 {
                hist[bin][b] += energy[b] * spread;
            }

            // Path recording (for visualization).
            path.push(h.point);
            if path.len() > opts.max_bounces + 2 {
                break;
            }

            let m = materials.get(h.material).cloned().unwrap_or_default();
            // Surface absorption + transmission leakage.
            for b in 0..7 {
                energy[b] *= (1.0 - m.absorption[b]).max(0.0);
                energy[b] *= (1.0 - m.transmission[b]).max(0.0);
            }
            // Scattering: blend specular ↔ Lambertian.
            let mean_s = m.mean_scattering();
            let new_dir = if rng.next_f64() < mean_s {
                rng.cosine_hemisphere(h.normal)
            } else {
                reflect(dir, h.normal)
            };
            origin = h.point.add(h.normal.scale(1e-4)); // nudge off the surface
            dir = new_dir.normalized();
        }

        if ray_paths.len() < opts.max_viz_rays {
            ray_paths.push(path);
        }
    }

    // Normalise by ray count.
    let n = opts.ray_count.max(1) as f64;
    for bin in hist.iter_mut() {
        for v in bin.iter_mut() {
            *v /= n;
        }
    }

    RayTraceResult {
        histogram: hist,
        bin_duration,
        ray_count: opts.ray_count,
        ray_paths,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::room::RoomSpec;

    #[test]
    fn trace_produces_decaying_histogram() {
        let spec = RoomSpec::default();
        let room = RoomGeometry::build(&spec);
        let mats = vec![crate::material::preset("drywall"); 16];
        let opts = RayOptions {
            ray_count: 2000,
            max_bounces: 20,
            max_time: 1.0,
            n_bins: 100,
            ..Default::default()
        };
        let res = trace_rays(&room, Vec3::new(0.0, 0.0, 1.5), &mats, &opts);
        assert_eq!(res.histogram.len(), 100);
        // Energy should exist and be non-negative.
        assert!(res.histogram.iter().all(|b| b.iter().all(|&e| e >= 0.0)));
        // Early bins should generally have more energy than late bins.
        let early: f64 = res.histogram[..20].iter().map(|b| b[3]).sum();
        let late: f64 = res.histogram[80..].iter().map(|b| b[3]).sum();
        assert!(early > late);
    }
}
