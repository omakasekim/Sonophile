//! Deterministic, seedable pseudo-random number generator (SplitMix64) used by
//! the stochastic ray tracer so results are reproducible across runs and
//! platforms (including wasm, where no OS entropy is used).

/// SplitMix64 generator.
#[derive(Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }

    /// Uniform direction on the unit sphere (Marsaglia's method).
    pub fn unit_sphere(&mut self) -> crate::geom::Vec3 {
        use crate::geom::Vec3;
        loop {
            let x = self.range(-1.0, 1.0);
            let y = self.range(-1.0, 1.0);
            let z = self.range(-1.0, 1.0);
            let v = Vec3::new(x, y, z);
            let l2 = v.dot(v);
            if l2 <= 1.0 && l2 > 1e-12 {
                return v.scale(1.0 / l2.sqrt());
            }
        }
    }

    /// Cosine-weighted (Lambertian) direction about a unit normal.
    pub fn cosine_hemisphere(&mut self, normal: crate::geom::Vec3) -> crate::geom::Vec3 {
        use crate::geom::Vec3;
        // Tangent/bitangent basis.
        let up = if normal.z.abs() < 0.9 {
            Vec3::new(0.0, 0.0, 1.0)
        } else {
            Vec3::new(1.0, 0.0, 0.0)
        };
        let tangent = normal.cross(up).normalized();
        let bitangent = normal.cross(tangent);
        let r = self.next_f64().sqrt();
        let phi = 2.0 * std::f64::consts::PI * self.next_f64();
        let x = r * phi.cos();
        let y = r * phi.sin();
        let z = (1.0 - r * r).sqrt();
        tangent
            .scale(x)
            .add(bitangent.scale(y))
            .add(normal.scale(z))
            .normalized()
    }
}

/// `n` quasi-uniform directions on the unit sphere via the Fibonacci spiral
/// (low-discrepancy sampling for the diffuse ray budget).
pub fn fibonacci_sphere(n: usize) -> Vec<crate::geom::Vec3> {
    use crate::geom::Vec3;
    let mut out = Vec::with_capacity(n);
    let golden = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
    for i in 0..n {
        let y = if n == 1 {
            0.0
        } else {
            1.0 - (i as f64 / (n - 1) as f64) * 2.0
        };
        let radius = (1.0 - y * y).sqrt();
        let theta = golden * i as f64;
        out.push(Vec3::new(theta.cos() * radius, y, theta.sin() * radius));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn unit_vectors() {
        let mut r = Rng::new(7);
        for _ in 0..1000 {
            let v = r.unit_sphere();
            assert!((v.len() - 1.0).abs() < 1e-9);
            let h = r.cosine_hemisphere(crate::geom::Vec3::new(0.0, 0.0, 1.0));
            assert!(h.z > -1e-9);
        }
    }

    #[test]
    fn fibonacci_unit() {
        for &n in &[1, 2, 100] {
            for v in fibonacci_sphere(n) {
                assert!((v.len() - 1.0).abs() < 1e-9);
            }
        }
    }
}
