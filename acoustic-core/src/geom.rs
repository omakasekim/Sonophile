//! 3-D geometry primitives and intersection routines for the geometric
//! (image-source / ray-tracing) solvers.

use serde::{Deserialize, Serialize};

/// 3-D point / vector.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
    pub fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
    pub fn scale(self, k: f64) -> Vec3 {
        Vec3::new(self.x * k, self.y * k, self.z * k)
    }
    pub fn neg(self) -> Vec3 {
        self.scale(-1.0)
    }
    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn normalized(self) -> Vec3 {
        let l = self.len();
        if l <= 0.0 {
            Vec3::new(0.0, 0.0, 0.0)
        } else {
            self.scale(1.0 / l)
        }
    }
    /// Squared distance.
    pub fn dist2(self, o: Vec3) -> f64 {
        self.sub(o).dot(self.sub(o))
    }
}

/// A ray `origin + t · dir`, `t ≥ 0`.
#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
}

/// A planar surface polygon (ccw winding when viewed from the room interior).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Surface {
    pub vertices: Vec<Vec3>,
    pub normal: Vec3,
    pub material: usize,
}

/// Ray/surface hit record.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub t: f64,
    pub point: Vec3,
    pub normal: Vec3,
    pub material: usize,
}

/// Axis-aligned bounding box.
#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn empty() -> Self {
        Aabb {
            min: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            max: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }
    pub fn include(&mut self, p: Vec3) {
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.min.z = self.min.z.min(p.z);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
        self.max.z = self.max.z.max(p.z);
    }
    pub fn from_points(pts: &[Vec3]) -> Self {
        let mut b = Aabb::empty();
        for &p in pts {
            b.include(p);
        }
        b
    }
    pub fn surface_area(&self) -> f64 {
        let d = self.max.sub(self.min);
        2.0 * (d.x * d.y + d.y * d.z + d.z * d.x)
    }
    pub fn contains(&self, p: Vec3) -> bool {
        p.x >= self.min.x && p.x <= self.max.x
            && p.y >= self.min.y && p.y <= self.max.y
            && p.z >= self.min.z && p.z <= self.max.z
    }
}

impl Surface {
    /// Area via fan triangulation of the (assumed planar) polygon.
    pub fn area(&self) -> f64 {
        if self.vertices.len() < 3 {
            return 0.0;
        }
        let mut acc = Vec3::new(0.0, 0.0, 0.0);
        let p0 = self.vertices[0];
        for i in 1..self.vertices.len() - 1 {
            let e1 = self.vertices[i].sub(p0);
            let e2 = self.vertices[i + 1].sub(p0);
            acc = acc.add(e1.cross(e2));
        }
        0.5 * acc.len()
    }

    /// Centroid (area-weighted average of triangle centroids).
    pub fn centroid(&self) -> Vec3 {
        if self.vertices.len() < 3 {
            return self.vertices.first().copied().unwrap_or(Vec3::new(0.0, 0.0, 0.0));
        }
        let p0 = self.vertices[0];
        let mut area_sum = 0.0;
        let mut c = Vec3::new(0.0, 0.0, 0.0);
        for i in 1..self.vertices.len() - 1 {
            let a = self.vertices[i];
            let b = self.vertices[i + 1];
            let tri = a.sub(p0).cross(b.sub(p0)).len() * 0.5;
            let cc = p0.add(a).add(b).scale(1.0 / 3.0);
            c = c.add(cc.scale(tri));
            area_sum += tri;
        }
        if area_sum <= 0.0 {
            p0
        } else {
            c.scale(1.0 / area_sum)
        }
    }

    /// Newell's method for a robust polygon normal (unit length).
    pub fn compute_normal(vertices: &[Vec3]) -> Vec3 {
        let mut n = Vec3::new(0.0, 0.0, 0.0);
        for i in 0..vertices.len() {
            let a = vertices[i];
            let b = vertices[(i + 1) % vertices.len()];
            n.x += (a.y - b.y) * (a.z + b.z);
            n.y += (a.z - b.z) * (a.x + b.x);
            n.z += (a.x - b.x) * (a.y + b.y);
        }
        n.normalized()
    }

    /// Ray-plane intersection, then point-in-polygon test. Returns `None` when
    /// the ray misses or hits from behind (relative to `normal`).
    pub fn intersect(&self, ray: &Ray, two_sided: bool) -> Option<Hit> {
        let denom = self.normal.dot(ray.dir);
        // Parallel or hitting from behind.
        if denom.abs() < 1e-12 {
            return None;
        }
        if denom > 0.0 && !two_sided {
            return None; // back face
        }
        let p0 = self.vertices[0];
        let t = p0.sub(ray.origin).dot(self.normal) / denom;
        if t <= 1e-9 {
            return None;
        }
        let point = ray.origin.add(ray.dir.scale(t));
        if !point_in_polygon(&self.vertices, &self.normal, point) {
            return None;
        }
        Some(Hit {
            t,
            point,
            normal: self.normal,
            material: self.material,
        })
    }
}

/// Point-in-polygon test for a (planar, possibly non-convex) polygon by
/// projecting onto the dominant normal axis and ray-casting in 2-D.
pub fn point_in_polygon(vertices: &[Vec3], normal: &Vec3, p: Vec3) -> bool {
    if vertices.len() < 3 {
        return false;
    }
    // Drop the axis with the largest normal component → 2-D projection.
    let ax = normal.x.abs();
    let ay = normal.y.abs();
    let az = normal.z.abs();
    let (u, v): (fn(Vec3) -> f64, fn(Vec3) -> f64) = if ax >= ay && ax >= az {
        (|q: Vec3| q.y, |q: Vec3| q.z)
    } else if ay >= ax && ay >= az {
        (|q: Vec3| q.x, |q: Vec3| q.z)
    } else {
        (|q: Vec3| q.x, |q: Vec3| q.y)
    };
    let mut inside = false;
    let n = vertices.len();
    let mut j = n - 1;
    for i in 0..n {
        let vi = vertices[i];
        let vj = vertices[j];
        let (ui, vi_) = (u(vi), v(vi));
        let (uj, vj_) = (u(vj), v(vj));
        let (up, vp) = (u(p), v(p));
        let intersects = ((vi_ > vp) != (vj_ > vp))
            && (up < (uj - ui) * (vp - vi_) / (vj_ - vi_) + ui);
        if intersects {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Slab-method ray/AABB intersection (`t ≥ 0`).
pub fn ray_aabb(ray: &Ray, bb: &Aabb) -> Option<f64> {
    let mut tmin = 0.0_f64;
    let mut tmax = f64::INFINITY;
    let o = [ray.origin.x, ray.origin.y, ray.origin.z];
    let d = [ray.dir.x, ray.dir.y, ray.dir.z];
    let lo = [bb.min.x, bb.min.y, bb.min.z];
    let hi = [bb.max.x, bb.max.y, bb.max.z];
    for i in 0..3 {
        if d[i].abs() < 1e-12 {
            if o[i] < lo[i] || o[i] > hi[i] {
                return None;
            }
        } else {
            let inv = 1.0 / d[i];
            let mut t1 = (lo[i] - o[i]) * inv;
            let mut t2 = (hi[i] - o[i]) * inv;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            tmin = tmin.max(t1);
            tmax = tmax.min(t2);
            if tmin > tmax {
                return None;
            }
        }
    }
    Some(tmin)
}

/// Reflect `d` about a unit normal `n` (`n·d < 0` assumed for incoming).
pub fn reflect(d: Vec3, n: Vec3) -> Vec3 {
    d.sub(n.scale(2.0 * d.dot(n)))
}

/// Radiation gain of a source directivity pattern for a given outgoing unit
/// direction `dir` relative to a unit `forward` axis. `θ` is the off-axis
/// angle; all patterns are normalised so on-axis gain is 1.
pub fn directivity_gain(pattern: &str, dir: Vec3, forward: Vec3) -> f64 {
    let cos_theta = dir.dot(forward).clamp(-1.0, 1.0);
    match pattern {
        "cardioid" => 0.5 * (1.0 + cos_theta),
        "supercardioid" => (0.37 + 0.63 * cos_theta).max(0.0),
        "cone" => (cos_theta * 0.5 + 0.5).powi(4),
        // omnidirectional and anything unknown → uniform radiation.
        _ => 1.0,
    }
}

/// Refract/transmit is intentionally excluded: transmission is modelled as
/// per-band energy leakage (a ray continues through the partition attenuated)
/// rather than Snell refraction, which is the standard simplification for room
/// acoustics thin partitions.

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_square_xy(z: f64, normal: Vec3) -> Surface {
        Surface {
            vertices: vec![
                Vec3::new(0.0, 0.0, z),
                Vec3::new(1.0, 0.0, z),
                Vec3::new(1.0, 1.0, z),
                Vec3::new(0.0, 1.0, z),
            ],
            normal,
            material: 0,
        }
    }

    #[test]
    fn ray_hits_square_center() {
        let s = unit_square_xy(0.0, Vec3::new(0.0, 0.0, 1.0));
        let ray = Ray {
            origin: Vec3::new(0.5, 0.5, 5.0),
            dir: Vec3::new(0.0, 0.0, -1.0),
        };
        let h = s.intersect(&ray, false).unwrap();
        assert!((h.t - 5.0).abs() < 1e-9);
        assert!(h.point.x > 0.0 && h.point.x < 1.0);
    }

    #[test]
    fn ray_misses_from_behind() {
        let s = unit_square_xy(0.0, Vec3::new(0.0, 0.0, 1.0));
        let ray = Ray {
            origin: Vec3::new(0.5, 0.5, -5.0),
            dir: Vec3::new(0.0, 0.0, 1.0),
        };
        assert!(s.intersect(&ray, false).is_none());
        // two-sided still finds it
        assert!(s.intersect(&ray, true).is_some());
    }

    #[test]
    fn normal_and_area() {
        let s = unit_square_xy(0.0, Vec3::new(0.0, 0.0, 1.0));
        let n = Surface::compute_normal(&s.vertices);
        assert!(n.z > 0.99);
        assert!((s.area() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn ray_aabb_hit_and_miss() {
        let bb = Aabb::from_points(&[Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0)]);
        let hit = Ray { origin: Vec3::new(0.5, 0.5, 3.0), dir: Vec3::new(0.0, 0.0, -1.0) };
        assert!(ray_aabb(&hit, &bb).is_some());
        let miss = Ray { origin: Vec3::new(2.0, 0.5, 3.0), dir: Vec3::new(0.0, 0.0, -1.0) };
        assert!(ray_aabb(&miss, &bb).is_none());
    }

    #[test]
    fn point_in_polygon_concave() {
        let n = Vec3::new(0.0, 0.0, 1.0);
        // L-shaped polygon in the xy-plane.
        let verts = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(2.0, 1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(1.0, 2.0, 0.0),
            Vec3::new(0.0, 2.0, 0.0),
        ];
        assert!(point_in_polygon(&verts, &n, Vec3::new(0.5, 0.5, 0.0)));
        assert!(!point_in_polygon(&verts, &n, Vec3::new(1.5, 1.5, 0.0)));
        assert!(point_in_polygon(&verts, &n, Vec3::new(1.5, 0.5, 0.0)));
    }
}
