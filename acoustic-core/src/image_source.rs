//! Image-source early-reflection solver (exact specular reflections up to a
//! configurable order, with occlusion checks).

use crate::geom::{Ray, Surface, Vec3};
use crate::material::Material;
use crate::prng::Rng;
use crate::room::RoomGeometry;

/// One early specular reflection arrival.
#[derive(Clone, Debug)]
pub struct EarlyReflection {
    /// Arrival time (s).
    pub time: f64,
    /// Total path length (m).
    pub distance: f64,
    /// Unit arrival direction at the listener (from listener toward the
    /// apparent image source).
    pub direction: Vec3,
    /// Per-band pressure amplitude (already includes distance attenuation and
    /// reflection coefficients).
    pub amplitude: [f64; 7],
    /// Reflection order.
    pub order: u32,
    /// The reflecting surface indices (for visualization).
    pub surface_path: Vec<usize>,
    /// The bounce points `r₁ … r_k` (for visualization).
    pub reflection_points: Vec<Vec3>,
}

/// Reflect a point across a surface's plane.
fn reflect_point(p: Vec3, surf: &Surface) -> Vec3 {
    let d = p.sub(surf.vertices[0]).dot(surf.normal);
    p.sub(surf.normal.scale(2.0 * d))
}

/// Is the open segment `a → b` blocked by any surface (other than those whose
/// indices are in `skip`)?
fn occluded(a: Vec3, b: Vec3, surfaces: &[Surface], skip: &[usize]) -> bool {
    let dir = b.sub(a);
    let len = dir.len();
    if len <= 1e-9 {
        return false;
    }
    let ray = Ray {
        origin: a,
        dir: dir.scale(1.0 / len),
    };
    for (i, s) in surfaces.iter().enumerate() {
        if skip.contains(&i) {
            continue;
        }
        if let Some(h) = s.intersect(&ray, true) {
            if h.t < len - 1e-6 {
                return true;
            }
        }
    }
    false
}

/// Compute early reflections between `source` and `listener` up to `max_order`
/// (≥ 1). Returns arrivals sorted by time.
pub fn early_reflections(
    room: &RoomGeometry,
    source: Vec3,
    listener: Vec3,
    materials: &[Material],
    max_order: u32,
    speed_of_sound: f64,
    directivity: &str,
) -> Vec<EarlyReflection> {
    let surfaces = &room.surfaces;
    let mut out: Vec<EarlyReflection> = Vec::new();

    // Enumerate all non-repeating surface sequences of length 1..=max_order.
    fn recurse(
        room: &RoomGeometry,
        source: Vec3,
        listener: Vec3,
        materials: &[Material],
        order: u32,
        max_order: u32,
        path: &mut Vec<usize>,
        out: &mut Vec<EarlyReflection>,
        speed: f64,
        directivity: &str,
    ) {
        let surfaces = &room.surfaces;
        // Build image chain.
        let mut img = source;
        let mut images: Vec<Vec3> = Vec::with_capacity(path.len());
        for &si in path.iter() {
            img = reflect_point(img, &surfaces[si]);
            images.push(img);
        }
        // Unwind from the listener.
        let mut target = listener;
        let mut reflection_points: Vec<Vec3> = Vec::with_capacity(path.len());
        let mut valid = true;
        for j in (0..path.len()).rev() {
            let si = path[j];
            let ray = Ray {
                origin: images[j],
                dir: target.sub(images[j]),
            };
            match surfaces[si].intersect(&ray, true) {
                Some(h) => {
                    reflection_points.push(h.point);
                    target = h.point;
                }
                None => {
                    valid = false;
                    break;
                }
            }
        }
        if !valid {
            return;
        }
        // Full path: source → r1 → r2 → … → rk → listener (r_k is first pushed).
        reflection_points.reverse(); // now r1, r2, ..., rk
        // Occlusion: each leg must be unobstructed.
        let mut total_distance = 0.0;
        let mut prev = source;
        let mut legs_ok = true;
        for (idx, &rp) in reflection_points.iter().enumerate() {
            total_distance += rp.sub(prev).len();
            if occluded(prev, rp, surfaces, &path[idx..]) {
                legs_ok = false;
                break;
            }
            prev = rp;
        }
        if !legs_ok {
            return;
        }
        total_distance += listener.sub(prev).len();
        if occluded(prev, listener, surfaces, &[]) {
            return;
        }
        if total_distance <= 1e-9 {
            return;
        }

        // Per-band amplitude: distance attenuation × ∏ sqrt(1 − α) per bounce,
        // weighted by the source directivity on the outgoing first leg.
        let mut amp = [1.0 / total_distance; 7];
        for &si in path.iter() {
            let m = materials.get(surfaces[si].material).cloned().unwrap_or_default();
            for b in 0..7 {
                amp[b] *= (1.0 - m.absorption[b]).clamp(0.0, 1.0).sqrt();
            }
        }
        let first_leg_dir = reflection_points[0].sub(source).normalized();
        let dg = crate::geom::directivity_gain(directivity, first_leg_dir, Vec3::new(1.0, 0.0, 0.0));
        for b in 0..7 {
            amp[b] *= dg;
        }
        let dir = images[images.len() - 1].sub(listener).normalized();
        out.push(EarlyReflection {
            time: total_distance / speed,
            distance: total_distance,
            direction: dir,
            amplitude: amp,
            order,
            surface_path: path.clone(),
            reflection_points: reflection_points.clone(),
        });

        if order < max_order {
            for next in 0..surfaces.len() {
                if path.last() == Some(&next) {
                    continue; // no immediate re-reflection of the same plane
                }
                path.push(next);
                recurse(room, source, listener, materials, order + 1, max_order, path, out, speed, directivity);
                path.pop();
            }
        }
    }

    let mut path = Vec::new();
    for first in 0..surfaces.len() {
        path.push(first);
        recurse(room, source, listener, materials, 1, max_order, &mut path, &mut out, speed_of_sound, directivity);
        path.pop();
    }

    out.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Compute the arrival time of the direct path and the ITDG (first reflection).
pub fn direct_arrival(source: Vec3, listener: Vec3, speed_of_sound: f64) -> f64 {
    source.sub(listener).len() / speed_of_sound
}

/// Stochastic ray tracing entry point (delegated to the ray-tracer module).
#[allow(dead_code)]
pub fn _unused(_r: &mut Rng) {}
