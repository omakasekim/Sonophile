//! Finite-difference time-domain (FDTD) wave solver for the low-frequency band
//! (20 Hz – ~300 Hz). Implements the 2-D (and 3-D) explicit second-order
//! collocated scheme for the linearised wave equation with:
//!
//! * CFL-limited time step (`c·Δt/Δx ≤ 1/√2` in 2-D, `≤ 1/√3` in 3-D),
//! * first-order locally-reacting wall admittance boundary conditions (rigid
//!   Neumann wall when the admittance is `0`, a matched first-order absorber as
//!   it approaches `1`),
//! * live point-source excitation (sine / sweep / impulse) and probe recording.

use serde::{Deserialize, Serialize};

/// Dimensionless boundary admittance `G = ρc/Z` from a random-incidence
/// absorption coefficient `α` (matching the normal-incidence pressure
/// reflection coefficient `R = √(1−α)`).
pub fn admittance_from_absorption(alpha: f64) -> f64 {
    let a = alpha.clamp(0.0, 0.9999);
    let r = (1.0 - a).sqrt();
    ((1.0 - r) / (1.0 + r)).clamp(0.0, 0.9)
}

/// 2-D FDTD pressure grid.
#[derive(Clone, Debug)]
pub struct Fdtd2D {
    pub nx: usize,
    pub ny: usize,
    pub dx: f64,
    pub dt: f64,
    pub c: f64,
    p: Vec<f64>,
    p_prev: Vec<f64>,
    g: [f64; 4], // left, right, bottom, top admittances
    /// Source pressure added during the next step(s).
    source: Vec<f64>,
    step_count: usize,
    /// World-space origin of grid cell (0,0).
    origin_x: f64,
    origin_y: f64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct FdtdConfig {
    pub nx: usize,
    pub ny: usize,
    pub dx: f64,
    pub c: f64,
    pub cfl: f64,
    /// Admittances [left, right, bottom, top].
    pub admittance: [f64; 4],
    /// World-space origin of grid cell (0,0) — normally the room's min corner.
    pub origin_x: f64,
    pub origin_y: f64,
}

impl Default for FdtdConfig {
    fn default() -> Self {
        FdtdConfig {
            nx: 128,
            ny: 96,
            dx: 0.1,
            c: 343.0,
            cfl: 0.9,
            admittance: [0.02, 0.02, 0.02, 0.02],
            origin_x: 0.0,
            origin_y: 0.0,
        }
    }
}

impl Fdtd2D {
    pub fn new(cfg: &FdtdConfig) -> Self {
        let dt = cfg.cfl * cfg.dx / (cfg.c * 2.0_f64.sqrt());
        Fdtd2D {
            nx: cfg.nx.max(2),
            ny: cfg.ny.max(2),
            dx: cfg.dx,
            dt,
            c: cfg.c,
            p: vec![0.0; cfg.nx.max(2) * cfg.ny.max(2)],
            p_prev: vec![0.0; cfg.nx.max(2) * cfg.ny.max(2)],
            g: cfg.admittance,
            source: vec![0.0; cfg.nx.max(2) * cfg.ny.max(2)],
            step_count: 0,
            origin_x: cfg.origin_x,
            origin_y: cfg.origin_y,
        }
    }

    #[inline]
    fn idx(&self, i: usize, j: usize) -> usize {
        j * self.nx + i
    }

    /// Inject a pressure sample at the cell nearest `(x, y)` (metres, world
    /// space). Multiple calls accumulate until the next `step`.
    pub fn inject(&mut self, x: f64, y: f64, value: f64) {
        let i = (((x - self.origin_x) / self.dx).round() as isize).clamp(0, self.nx as isize - 1) as usize;
        let j = (((y - self.origin_y) / self.dx).round() as isize).clamp(0, self.ny as isize - 1) as usize;
        let idx = self.idx(i, j);
        self.source[idx] += value;
    }

    /// Read the pressure at the cell nearest `(x, y)`.
    pub fn probe(&self, x: f64, y: f64) -> f64 {
        let i = (((x - self.origin_x) / self.dx).round() as isize).clamp(0, self.nx as isize - 1) as usize;
        let j = (((y - self.origin_y) / self.dx).round() as isize).clamp(0, self.ny as isize - 1) as usize;
        self.p[self.idx(i, j)]
    }

    /// Advance the grid by one time step.
    pub fn step(&mut self) {
        let c2 = (self.c * self.dt / self.dx) * (self.c * self.dt / self.dx);
        let coef = self.dx / (self.c * self.dt);
        let mut next = vec![0.0; self.nx * self.ny];

        for j in 0..self.ny {
            for i in 0..self.nx {
                let cur = self.p[self.idx(i, j)];
                let prev = self.p_prev[self.idx(i, j)];

                // Ghost neighbours (impedance boundary conditions).
                let l = if i == 0 {
                    cur - self.g[0] * coef * (cur - prev)
                } else {
                    self.p[self.idx(i - 1, j)]
                };
                let r = if i == self.nx - 1 {
                    cur + self.g[1] * coef * (cur - prev)
                } else {
                    self.p[self.idx(i + 1, j)]
                };
                let b = if j == 0 {
                    cur - self.g[2] * coef * (cur - prev)
                } else {
                    self.p[self.idx(i, j - 1)]
                };
                let t = if j == self.ny - 1 {
                    cur + self.g[3] * coef * (cur - prev)
                } else {
                    self.p[self.idx(i, j + 1)]
                };

                let lap = l + r + b + t - 4.0 * cur;
                let src = self.source[self.idx(i, j)];
                next[self.idx(i, j)] = 2.0 * cur - prev + c2 * lap + src;
            }
        }

        self.p_prev.clone_from(&self.p);
        self.p.clone_from(&next);
        self.source.fill(0.0);
        self.step_count += 1;
    }

    pub fn step_many(&mut self, n: usize) {
        for _ in 0..n {
            self.step();
        }
    }

    /// Current pressure field (row-major, `nx × ny`).
    pub fn grid(&self) -> &[f64] {
        &self.p
    }

    pub fn current_time(&self) -> f64 {
        self.step_count as f64 * self.dt
    }

    /// Reset the field.
    pub fn reset(&mut self) {
        self.p.fill(0.0);
        self.p_prev.fill(0.0);
        self.source.fill(0.0);
        self.step_count = 0;
    }
}

/// 3-D FDTD pressure grid (collocated scheme). Memory is `nx·ny·nz` doubles, so
/// keep grids small; the real-time heatmap uses [`Fdtd2D`].
#[derive(Clone, Debug)]
pub struct Fdtd3D {
    pub nx: usize,
    pub ny: usize,
    pub nz: usize,
    pub dx: f64,
    pub dt: f64,
    pub c: f64,
    p: Vec<f64>,
    p_prev: Vec<f64>,
    g: [f64; 6], // -x, +x, -y, +y, -z, +z
    source: Vec<f64>,
}

impl Fdtd3D {
    pub fn new(nx: usize, ny: usize, nz: usize, dx: f64, c: f64, admittance: [f64; 6]) -> Self {
        let dt = 0.9 * dx / (c * 3.0_f64.sqrt());
        Fdtd3D {
            nx: nx.max(2),
            ny: ny.max(2),
            nz: nz.max(2),
            dx,
            dt,
            c,
            p: vec![0.0; nx.max(2) * ny.max(2) * nz.max(2)],
            p_prev: vec![0.0; nx.max(2) * ny.max(2) * nz.max(2)],
            g: admittance,
            source: vec![0.0; nx.max(2) * ny.max(2) * nz.max(2)],
        }
    }

    #[inline]
    fn idx(&self, i: usize, j: usize, k: usize) -> usize {
        (k * self.ny + j) * self.nx + i
    }

    pub fn inject(&mut self, i: usize, j: usize, k: usize, value: f64) {
        let n = self.idx(
            i.min(self.nx - 1),
            j.min(self.ny - 1),
            k.min(self.nz - 1),
        );
        self.source[n] += value;
    }

    pub fn step(&mut self) {
        let c2 = (self.c * self.dt / self.dx) * (self.c * self.dt / self.dx);
        let coef = self.dx / (self.c * self.dt);
        let mut next = vec![0.0; self.nx * self.ny * self.nz];
        for k in 0..self.nz {
            for j in 0..self.ny {
                for i in 0..self.nx {
                    let cur = self.p[self.idx(i, j, k)];
                    let prev = self.p_prev[self.idx(i, j, k)];
                    let xm = if i == 0 { cur - self.g[0] * coef * (cur - prev) } else { self.p[self.idx(i - 1, j, k)] };
                    let xp = if i == self.nx - 1 { cur + self.g[1] * coef * (cur - prev) } else { self.p[self.idx(i + 1, j, k)] };
                    let ym = if j == 0 { cur - self.g[2] * coef * (cur - prev) } else { self.p[self.idx(i, j - 1, k)] };
                    let yp = if j == self.ny - 1 { cur + self.g[3] * coef * (cur - prev) } else { self.p[self.idx(i, j + 1, k)] };
                    let zm = if k == 0 { cur - self.g[4] * coef * (cur - prev) } else { self.p[self.idx(i, j, k - 1)] };
                    let zp = if k == self.nz - 1 { cur + self.g[5] * coef * (cur - prev) } else { self.p[self.idx(i, j, k + 1)] };
                    let lap = xm + xp + ym + yp + zm + zp - 6.0 * cur;
                    next[self.idx(i, j, k)] = 2.0 * cur - prev + c2 * lap + self.source[self.idx(i, j, k)];
                }
            }
        }
        self.p_prev.clone_from(&self.p);
        self.p.clone_from(&next);
        self.source.fill(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cfl_is_stable() {
        let cfg = FdtdConfig { nx: 40, ny: 40, ..Default::default() };
        let mut f = Fdtd2D::new(&cfg);
        f.inject(2.0, 2.0, 1.0);
        // Run long enough for the wave to propagate across the grid.
        let steps = (3.0 / f.dt) as usize;
        f.step_many(steps);
        let max_abs = f.grid().iter().fold(0.0_f64, |m, &v| m.max(v.abs()));
        // A stable scheme must not blow up.
        assert!(max_abs.is_finite());
        assert!(max_abs < 100.0, "field blew up: {max_abs}");
    }

    #[test]
    fn probe_sees_excitation() {
        let cfg = FdtdConfig { nx: 40, ny: 40, ..Default::default() };
        let mut f = Fdtd2D::new(&cfg);
        f.inject(2.0, 2.0, 1.0);
        f.step();
        assert!(f.probe(2.0, 2.0).abs() > 0.0);
    }

    #[test]
    fn admittance_mapping() {
        assert!(admittance_from_absorption(0.0) < 1e-12);
        assert!(admittance_from_absorption(0.9) > admittance_from_absorption(0.1));
        assert!(admittance_from_absorption(1.0) <= 0.9);
    }

    #[test]
    fn fdtd3d_runs() {
        let mut f = Fdtd3D::new(8, 8, 8, 0.1, 343.0, [0.02; 6]);
        f.inject(4, 4, 4, 1.0);
        for _ in 0..20 {
            f.step();
        }
    }
}
