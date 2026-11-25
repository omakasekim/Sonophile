//! Room geometry: turn a 2-D floorplan + height into a set of planar
//! [`Surface`]s (floor, ceiling, walls with door/window apertures, and box
//! furniture) suitable for image-source and ray tracing.

use serde::{Deserialize, Serialize};

use crate::geom::{Aabb, Surface, Vec3};
use crate::material::{Material, N_BANDS};

/// 2-D point in the floorplan.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Point2 {
    pub x: f64,
    pub y: f64,
}

/// A door/window opening on a wall (transmission leakage).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApertureSpec {
    pub wall_index: usize,
    /// Parametric span `[start, end]` along the wall edge (0..1).
    pub start: f64,
    pub end: f64,
    /// Vertical span `[bottom, top]` in metres.
    pub bottom: f64,
    pub top: f64,
    pub material: usize,
}

/// A rectangular furniture block (axis-aligned or rotated about z).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoxSpec {
    pub cx: f64,
    pub cy: f64,
    pub w: f64,
    pub d: f64,
    pub h: f64,
    /// Rotation about z in radians.
    pub rot: f64,
    pub material: usize,
}

/// The full room specification (what the editor serialises to JSON).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoomSpec {
    /// Footprint vertices (ccw when viewed from above).
    pub floorplan: Vec<Point2>,
    pub height: f64,
    pub wall_material: usize,
    pub floor_material: usize,
    pub ceiling_material: usize,
    pub apertures: Vec<ApertureSpec>,
    pub furniture: Vec<BoxSpec>,
}

impl RoomSpec {
    /// A default rectangular room.
    pub fn default() -> Self {
        let w = 5.0;
        let d = 4.0;
        RoomSpec {
            floorplan: vec![
                Point2 { x: -w / 2.0, y: -d / 2.0 },
                Point2 { x: w / 2.0, y: -d / 2.0 },
                Point2 { x: w / 2.0, y: d / 2.0 },
                Point2 { x: -w / 2.0, y: d / 2.0 },
            ],
            height: 2.7,
            wall_material: 0,
            floor_material: 0,
            ceiling_material: 0,
            apertures: vec![],
            furniture: vec![],
        }
    }
}

/// A built room: a surface set plus a bounding box.
#[derive(Clone, Debug)]
pub struct RoomGeometry {
    pub surfaces: Vec<Surface>,
    pub bounds: Aabb,
}

fn wall_normal(p0: Point2, p1: Point2) -> Vec3 {
    // ccw footprint → interior is left of each edge → inward normal (−dy, dx).
    let dx = p1.x - p0.x;
    let dy = p1.y - p0.y;
    Vec3::new(-dy, dx, 0.0).normalized()
}

/// Subdivide one wall edge into panels, respecting apertures.
fn build_wall_panels(
    p0: Point2,
    p1: Point2,
    height: f64,
    wall_mat: usize,
    apertures: &[ApertureSpec],
) -> Vec<Surface> {
    let n = wall_normal(p0, p1);
    let v0 = Vec3::new(p0.x, p0.y, 0.0);
    let v1 = Vec3::new(p1.x, p1.y, 0.0);
    let edge = v1.sub(v0);
    let len = edge.len();
    if len <= 1e-9 {
        return vec![];
    }
    let dir = edge.scale(1.0 / len);

    // Breakpoints along the edge.
    let mut breaks: Vec<f64> = vec![0.0, len];
    for a in apertures {
        let s0 = (a.start.clamp(0.0, 1.0)) * len;
        let s1 = (a.end.clamp(0.0, 1.0)) * len;
        breaks.push(s0.min(s1));
        breaks.push(s0.max(s1));
    }
    breaks.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    breaks.dedup_by(|a, b| (*a - *b).abs() < 1e-9);

    let mut out = Vec::new();
    for w in breaks.windows(2) {
        let (sa, sb) = (w[0], w[1]);
        if sb - sa <= 1e-9 {
            continue;
        }
        let a = v0.add(dir.scale(sa));
        let b = v0.add(dir.scale(sb));
        // Vertical breakpoints (aperture bottom/top), else full height.
        let mut zbreaks: Vec<f64> = vec![0.0, height];
        for ap in apertures {
            let s0 = (ap.start.clamp(0.0, 1.0)) * len;
            let s1 = (ap.end.clamp(0.0, 1.0)) * len;
            let lo = s0.min(s1);
            let hi = s0.max(s1);
            // Does this horizontal segment fall inside the aperture?
            if sa >= lo - 1e-9 && sb <= hi + 1e-9 {
                zbreaks.push(ap.bottom.clamp(0.0, height));
                zbreaks.push(ap.top.clamp(0.0, height));
            }
        }
        zbreaks.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        zbreaks.dedup_by(|a, b| (*a - *b).abs() < 1e-9);

        for z in zbreaks.windows(2) {
            let (za, zb) = (z[0], z[1]);
            if zb - za <= 1e-9 {
                continue;
            }
            // Determine material: aperture material if this segment is inside an
            // aperture's (s, z) rectangle.
            let smid = (sa + sb) * 0.5;
            let zmid = (za + zb) * 0.5;
            let mut mat = wall_mat;
            for ap in apertures {
                let s0 = (ap.start.clamp(0.0, 1.0)) * len;
                let s1 = (ap.end.clamp(0.0, 1.0)) * len;
                let lo = s0.min(s1);
                let hi = s0.max(s1);
                if smid >= lo - 1e-9
                    && smid <= hi + 1e-9
                    && zmid >= ap.bottom.clamp(0.0, height) - 1e-9
                    && zmid <= ap.top.clamp(0.0, height) + 1e-9
                {
                    mat = ap.material;
                }
            }
            out.push(Surface {
                vertices: vec![
                    a.add(Vec3::new(0.0, 0.0, za)),
                    b.add(Vec3::new(0.0, 0.0, za)),
                    b.add(Vec3::new(0.0, 0.0, zb)),
                    a.add(Vec3::new(0.0, 0.0, zb)),
                ],
                normal: n,
                material: mat,
            });
        }
    }
    out
}

/// Build furniture box faces (5 faces: sides + top; bottom rests on the floor).
fn build_box_faces(b: &BoxSpec, height: f64) -> Vec<Surface> {
    let hw = (b.w / 2.0).max(1e-3);
    let hd = (b.d / 2.0).max(1e-3);
    let h = b.h.clamp(0.0, height);
    let (s, c) = b.rot.sin_cos();
    let corner = |lx: f64, ly: f64, z: f64| -> Vec3 {
        let x = lx * c - ly * s;
        let y = lx * s + ly * c;
        Vec3::new(b.cx + x, b.cy + y, z)
    };
    let mut faces = Vec::new();
    // bottom quad corners
    let b00 = corner(-hw, -hd, 0.0);
    let b10 = corner(hw, -hd, 0.0);
    let b11 = corner(hw, hd, 0.0);
    let b01 = corner(-hw, hd, 0.0);
    let t00 = corner(-hw, -hd, h);
    let t10 = corner(hw, -hd, h);
    let t11 = corner(hw, hd, h);
    let t01 = corner(-hw, hd, h);

    // sides (outward normals via Newell)
    let quads: [(Vec3, Vec3, Vec3, Vec3); 5] = [
        (b00, b10, t10, t00), // front
        (b10, b11, t11, t10), // right
        (b11, b01, t01, t11), // back
        (b01, b00, t00, t01), // left
        (t00, t10, t11, t01), // top
    ];
    for (p0, p1, p2, p3) in quads {
        let verts = vec![p0, p1, p2, p3];
        let n = Surface::compute_normal(&verts);
        faces.push(Surface {
            vertices: verts,
            normal: n,
            material: b.material,
        });
    }
    faces
}

impl RoomGeometry {
    /// Build the surface set and AABB from a spec.
    pub fn build(spec: &RoomSpec) -> RoomGeometry {
        let mut surfaces = Vec::new();
        let mut bounds = Aabb::empty();
        let h = spec.height.max(0.1);

        // Floor.
        let floor: Vec<Vec3> = spec
            .floorplan
            .iter()
            .map(|p| Vec3::new(p.x, p.y, 0.0))
            .collect();
        if floor.len() >= 3 {
            let n = Vec3::new(0.0, 0.0, 1.0);
            surfaces.push(Surface {
                vertices: floor.clone(),
                normal: n,
                material: spec.floor_material,
            });
            // Ceiling (wound oppositely so its normal points down into the room).
            let mut ceil = floor.clone();
            ceil.reverse();
            surfaces.push(Surface {
                vertices: ceil.iter().map(|p| Vec3::new(p.x, p.y, h)).collect(),
                normal: Vec3::new(0.0, 0.0, -1.0),
                material: spec.ceiling_material,
            });
            for p in &floor {
                bounds.include(*p);
                bounds.include(Vec3::new(p.x, p.y, h));
            }
        }

        // Walls (with apertures).
        let n = spec.floorplan.len();
        for i in 0..n {
            let p0 = spec.floorplan[i];
            let p1 = spec.floorplan[(i + 1) % n];
            let mut wall_apertures: Vec<ApertureSpec> = Vec::new();
            for a in &spec.apertures {
                if a.wall_index == i {
                    wall_apertures.push(a.clone());
                }
            }
            surfaces.extend(build_wall_panels(
                p0, p1, h, spec.wall_material, &wall_apertures,
            ));
        }

        // Furniture.
        for b in &spec.furniture {
            let faces = build_box_faces(b, h);
            for f in &faces {
                for v in &f.vertices {
                    bounds.include(*v);
                }
            }
            surfaces.extend(faces);
        }

        RoomGeometry { surfaces, bounds }
    }

    /// Total absorbing surface area (area-weighted mean absorption), used to
    /// estimate broadband RT60 via Sabine when ray tracing is disabled.
    pub fn total_area(&self) -> f64 {
        self.surfaces.iter().map(|s| s.area()).sum()
    }

    /// Volume (used by Sabine: `V = A_floor · height` for prismatic rooms; here
    /// we integrate the footprint area).
    pub fn volume(&self) -> f64 {
        // Approximate via the floor area (largest horizontal surface) × height.
        let mut floor_area = 0.0_f64;
        for s in &self.surfaces {
            if s.normal.z.abs() > 0.99 && s.normal.z > 0.0 {
                floor_area = floor_area.max(s.area());
            }
        }
        let h = self.bounds.max.z - self.bounds.min.z;
        floor_area * h.max(0.1)
    }
}

/// Air absorption coefficient (dB/m) per octave band using the ISO 9613-1
/// formula for a given temperature (°C) and relative humidity (%).
pub fn air_absorption_db_per_m(temp_c: f64, humidity: f64, band: usize) -> f64 {
    let f = crate::material::OCTAVE_BANDS_HZ[band];
    let t_k = temp_c + 273.15;
    let h_rel = humidity.clamp(0.0, 100.0);
    // Saturation pressure of water vapour (Buck approximation).
    let psat = 6.1121 * ((18.678 - temp_c / 234.5) * (temp_c / (257.14 + temp_c))).exp() * 100.0;
    let h = h_rel * psat / 101325.0; // molar fraction-ish
    let fr_o = 24.0 + 4.04e4 * h * (0.02 + h) / (0.391 + h);
    let fr_n = (t_k / 293.15).powf(-0.5) * (9.0 + 280.0 * h * (-4.170 * ((t_k / 293.15).powf(-1.0 / 3.0) - 1.0)).exp());
    let alpha = 8.686
        * f
        * f
        * (1.84e-11 * (101325.0 / 101325.0) * (t_k / 293.15).powf(0.5)
            + (t_k / 293.15).powf(-2.5)
                * (0.01275 * (-2239.1 / t_k).exp() * (fr_o + f * f / fr_o).powi(-1)
                    + 0.1068 * (-3352.0 / t_k).exp() * (fr_n + f * f / fr_n).powi(-1)));
    // dB per metre; guard against NaN for extreme inputs.
    if alpha.is_finite() && alpha >= 0.0 {
        alpha
    } else {
        0.0
    }
}

/// Sabine RT60 estimate per band (s): `0.161 · V / Σ(Sᵢ αᵢ)`.
pub fn sabine_rt60(room: &RoomGeometry, materials: &[Material], band: usize) -> f64 {
    let mut a = 0.0;
    for s in &room.surfaces {
        let m = materials.get(s.material).cloned().unwrap_or_default();
        a += s.area() * m.absorption[band.min(N_BANDS - 1)];
    }
    let v = room.volume();
    if a <= 1e-9 {
        // Purely reflective room: essentially infinite decay.
        return f64::INFINITY;
    }
    0.161 * v / a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangular_room_surface_count() {
        let spec = RoomSpec::default();
        let room = RoomGeometry::build(&spec);
        // floor + ceiling + 4 walls = 6 surfaces, no furniture/apertures.
        assert_eq!(room.surfaces.len(), 6);
        // total area = 2*(5*4) + 2*(5*2.7) + 2*(4*2.7)
        let expected = 2.0 * 20.0 + 2.0 * 13.5 + 2.0 * 10.8;
        assert!((room.total_area() - expected).abs() < 1e-6);
    }

    #[test]
    fn aperture_splits_wall() {
        let mut spec = RoomSpec::default();
        spec.apertures.push(ApertureSpec {
            wall_index: 0,
            start: 0.4,
            end: 0.6,
            bottom: 0.9,
            top: 2.0,
            material: 9, // door
        });
        let room = RoomGeometry::build(&spec);
        // wall 0 becomes 3 horizontal × 3 vertical panels = 9, others stay 1 each.
        let wall0_panels = room
            .surfaces
            .iter()
            .filter(|s| s.normal.y.abs() > 0.99 || s.normal.x.abs() > 0.99)
            .count();
        assert!(wall0_panels > 4);
        // at least one panel uses the aperture material
        assert!(room.surfaces.iter().any(|s| s.material == 9));
    }

    #[test]
    fn air_absorption_monotonic_in_frequency() {
        let lo = air_absorption_db_per_m(20.0, 50.0, 0);
        let hi = air_absorption_db_per_m(20.0, 50.0, 6);
        assert!(hi > lo);
        assert!(air_absorption_db_per_m(20.0, 50.0, 3).is_finite());
    }

    #[test]
    fn sabine_increases_with_absorption() {
        let spec = RoomSpec::default();
        let room = RoomGeometry::build(&spec);
        let reflective = vec![crate::material::preset("concrete"); 12];
        let absorptive = vec![crate::material::preset("mineral-wool"); 12];
        let rt_ref = sabine_rt60(&room, &reflective, 4);
        let rt_abs = sabine_rt60(&room, &absorptive, 4);
        assert!(rt_ref > rt_abs);
    }
}
