//! Room modes (axial / tangential / oblique) for rectangular rooms.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModeKind {
    Axial,
    Tangential,
    Oblique,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct RoomMode {
    pub freq: f64,
    pub nx: u32,
    pub ny: u32,
    pub nz: u32,
    pub kind: ModeKind,
}

/// Enumerate room modes `f = (c/2)·√((nₓ/Lₓ)² + (n_y/L_y)² + (n_z/L_z)²)` up to
/// `max_freq`. Non-rectangular rooms are approximated by their bounding box.
pub fn room_modes(dims: (f64, f64, f64), c: f64, max_freq: f64) -> Vec<RoomMode> {
    let (lx, ly, lz) = dims;
    let mut out = Vec::new();
    let max_nx = ((max_freq * 2.0 * lx / c).ceil() as u32).min(40);
    let max_ny = ((max_freq * 2.0 * ly / c).ceil() as u32).min(40);
    let max_nz = ((max_freq * 2.0 * lz / c).ceil() as u32).min(40);
    for nx in 0..=max_nx {
        for ny in 0..=max_ny {
            for nz in 0..=max_nz {
                if nx == 0 && ny == 0 && nz == 0 {
                    continue;
                }
                let f = c / 2.0
                    * ((nx as f64 / lx).powi(2) + (ny as f64 / ly).powi(2) + (nz as f64 / lz).powi(2)).sqrt();
                if f > max_freq {
                    continue;
                }
                let count = (nx != 0) as u8 + (ny != 0) as u8 + (nz != 0) as u8;
                let kind = match count {
                    1 => ModeKind::Axial,
                    2 => ModeKind::Tangential,
                    _ => ModeKind::Oblique,
                };
                out.push(RoomMode { freq: f, nx, ny, nz, kind });
            }
        }
    }
    out.sort_by(|a, b| a.freq.partial_cmp(&b.freq).unwrap_or(std::cmp::Ordering::Equal));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axial_mode_of_first_dimension() {
        // Lx = 5, c = 343 → first axial mode at 34.3 Hz.
        let modes = room_modes((5.0, 4.0, 2.7), 343.0, 40.0);
        assert!(modes.iter().any(|m| {
            m.kind == ModeKind::Axial
                && m.nx == 1
                && m.ny == 0
                && m.nz == 0
                && (m.freq - 34.3).abs() < 0.1
        }));
    }

    #[test]
    fn modes_sorted_and_bounded() {
        let modes = room_modes((6.0, 5.0, 3.0), 343.0, 200.0);
        assert!(modes.windows(2).all(|w| w[0].freq <= w[1].freq));
        assert!(modes.iter().all(|m| m.freq <= 200.0));
    }
}
