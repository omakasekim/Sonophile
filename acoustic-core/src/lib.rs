//! Sonophile acoustic core — WebAssembly API surface and orchestration.
//!
//! The crate is split into pure-Rust physics/DSP modules (compilable and
//! testable natively) and this thin `#[wasm_bindgen]` façade that exchanges
//! JSON strings and `Float32Array`s with the browser. All heavy work stays in
//! the engine (called from a Web Worker), never on the audio/UI thread.

pub mod fdtd;
pub mod fft;
pub mod filters;
pub mod geom;
pub mod image_source;
pub mod material;
pub mod metrics;
pub mod modes;
pub mod prng;
pub mod ray_tracer;
pub mod rir;
pub mod room;
pub mod wav;

use std::cell::RefCell;


use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

use fdtd::{Fdtd2D, FdtdConfig};
use geom::Vec3;
use material::{Material, N_BANDS, OCTAVE_BANDS_HZ};
use metrics::{band_rt60_from_histogram, clarity, rt60_t20_t30_edt, schroeder_edc_db};
use modes::room_modes;
use ray_tracer::{RayOptions, RayTraceResult};
use rir::{synthesize_rir, RirBuffers, RirOptions, SPEED_OF_SOUND};
use room::{sabine_rt60, RoomGeometry, RoomSpec};
use wav::{write_wav, WavFormat};

// ---------------------------------------------------------------- JSON helpers

fn jf(v: &Value, key: &str, d: f64) -> f64 {
    v.get(key).and_then(|x| x.as_f64()).unwrap_or(d)
}
fn ju(v: &Value, key: &str, d: usize) -> usize {
    v.get(key).and_then(|x| x.as_u64()).unwrap_or(d as u64) as usize
}
fn js(v: &Value, key: &str, d: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or(d).to_string()
}
fn jvec3(v: &Value, key: &str) -> Vec3 {
    let o = v.get(key).and_then(|x| x.as_object());
    match o {
        Some(o) => Vec3::new(
            o.get("x").and_then(|x| x.as_f64()).unwrap_or(0.0),
            o.get("y").and_then(|x| x.as_f64()).unwrap_or(0.0),
            o.get("z").and_then(|x| x.as_f64()).unwrap_or(1.2),
        ),
        None => Vec3::new(0.0, 0.0, 1.2),
    }
}

// -------------------------------------------------------------- engine state

/// Excitation mode for the live FDTD heatmap.
enum Excitation {
    Off,
    Impulse,
    Sine,
    Sweep,
}

struct FdtdState {
    grid: Fdtd2D,
    excitation: Excitation,
    freq: f64,
    src: (f64, f64),
    sweep_t: f64,
    sweep_duration: f64,
}

impl Default for FdtdState {
    fn default() -> Self {
        FdtdState {
            grid: Fdtd2D::new(&FdtdConfig::default()),
            excitation: Excitation::Off,
            freq: 55.0,
            src: (0.0, 0.0),
            sweep_t: 0.0,
            sweep_duration: 4.0,
        }
    }
}

struct Engine {
    room: Option<RoomGeometry>,
    materials: Vec<Material>,
    source: Vec3,
    listener: Vec3,
    fdtd: FdtdState,
    rir: Option<RirBuffers>,
    metrics: Option<String>,
}

impl Default for Engine {
    fn default() -> Self {
        Engine {
            room: None,
            materials: vec![],
            source: Vec3::new(0.0, 0.0, 1.2),
            listener: Vec3::new(2.0, 1.0, 1.2),
            fdtd: FdtdState::default(),
            rir: None,
            metrics: None,
        }
    }
}

thread_local! {
    static ENGINE: RefCell<Engine> = RefCell::new(Engine::default());
}

fn with_engine<T>(f: impl FnOnce(&mut Engine) -> T) -> T {
    ENGINE.with(|e| f(&mut e.borrow_mut()))
}

/// Parse a `{ "room": RoomSpec, "materials": [Material] }` object and build the
/// geometry, storing it in the engine.
fn build_from_value(v: &Value) -> Result<(RoomGeometry, Vec<Material>), String> {
    let room_spec: RoomSpec = serde_json::from_value(v.get("room").cloned().unwrap_or(Value::Null))
        .map_err(|e| format!("invalid room: {e}"))?;
    if room_spec.floorplan.len() < 3 {
        return Err("room must have at least 3 floorplan vertices".into());
    }
    let materials: Vec<Material> =
        serde_json::from_value(v.get("materials").cloned().unwrap_or(Value::Null))
            .unwrap_or_else(|_| (0..16).map(|_| material::preset("drywall")).collect());
    let geo = RoomGeometry::build(&room_spec);
    Ok((geo, materials))
}

// ------------------------------------------------------------------- WASM API

#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Build the room and material library; returns a summary (surface count, area,
/// volume, Sabine RT60 per band).
#[wasm_bindgen]
pub fn build_room(spec_json: &str) -> String {
    let v: Value = match serde_json::from_str(spec_json) {
        Ok(v) => v,
        Err(e) => return json!({ "error": format!("bad JSON: {e}") }).to_string(),
    };
    let (geo, mats) = match build_from_value(&v) {
        Ok(x) => x,
        Err(e) => return json!({ "error": e }).to_string(),
    };
    let sabine: Vec<f64> = (0..N_BANDS).map(|b| sabine_rt60(&geo, &mats, b)).collect();
    let summary = json!({
        "surfaces": geo.surfaces.len(),
        "area": geo.total_area(),
        "volume": geo.volume(),
        "sabine_rt60": sabine,
        "bounds": {
            "min": [geo.bounds.min.x, geo.bounds.min.y, geo.bounds.min.z],
            "max": [geo.bounds.max.x, geo.bounds.max.y, geo.bounds.max.z],
        }
    });
    with_engine(|e| {
        e.room = Some(geo);
        e.materials = mats;
    });
    summary.to_string()
}

/// Compute early specular reflections (image-source). Request:
/// `{ "source": {x,y,z}, "listener": {x,y,z}, "max_order": 3 }`.
#[wasm_bindgen]
pub fn early_reflections(req_json: &str) -> String {
    let v: Value = serde_json::from_str(req_json).unwrap_or(Value::Null);
    let source = jvec3(&v, "source");
    let listener = jvec3(&v, "listener");
    let max_order = ju(&v, "max_order", 3).clamp(1, 4) as u32;
    with_engine(|e| {
        let Some(room) = &e.room else {
            return json!({ "error": "call build_room first" }).to_string();
        };
        let dv = js(&v, "directivity", "omnidirectional");
        let refl = image_source::early_reflections(room, source, listener, &e.materials, max_order, SPEED_OF_SOUND, &dv);
        let list: Vec<Value> = refl
            .iter()
            .map(|r| {
                let points: Vec<[f64; 3]> = r
                    .reflection_points
                    .iter()
                    .map(|p| [p.x, p.y, p.z])
                    .collect();
                json!({
                    "time": r.time,
                    "distance": r.distance,
                    "order": r.order,
                    "direction": [r.direction.x, r.direction.y, r.direction.z],
                    "amplitude": r.amplitude,
                    "surface_path": r.surface_path,
                    "points": points,
                })
            })
            .collect();
        e.source = source;
        e.listener = listener;
        json!({ "reflections": list }).to_string()
    })
}

/// Run the stochastic ray tracer. Returns `{ histogram, bin_duration, ray_count, ray_paths }`.
#[wasm_bindgen]
pub fn trace_rays(req_json: &str) -> String {
    let v: Value = serde_json::from_str(req_json).unwrap_or(Value::Null);
    let opts = RayOptions {
        ray_count: ju(&v, "ray_count", 20000).clamp(64, 1_000_000),
        max_bounces: ju(&v, "max_bounces", 40).clamp(1, 200),
        max_time: jf(&v, "max_time", 2.0),
        n_bins: ju(&v, "n_bins", 2048).clamp(16, 16384),
        temp_c: jf(&v, "temp_c", 20.0),
        humidity: jf(&v, "humidity", 50.0),
        seed: ju(&v, "seed", 0x5eed_5eed) as u64,
        max_viz_rays: ju(&v, "max_viz_rays", 400),
        directivity: js(&v, "directivity", "omnidirectional"),
    };
    with_engine(|e| {
        let Some(room) = &e.room else {
            return json!({ "error": "call build_room first" }).to_string();
        };
        let src = jvec3(&v, "source");
        let res = ray_tracer::trace_rays(room, src, &e.materials, &opts);
        let flat: Vec<f64> = res.histogram.iter().flat_map(|b| b.to_vec()).collect();
        let paths: Vec<Value> = res
            .ray_paths
            .iter()
            .map(|p| {
                let pts: Vec<[f64; 3]> = p.iter().map(|q| [q.x, q.y, q.z]).collect();
                json!(pts)
            })
            .collect();
        json!({
            "histogram": flat,
            "bin_duration": res.bin_duration,
            "ray_count": res.ray_count,
            "n_bins": res.histogram.len(),
            "ray_paths": paths,
        })
        .to_string()
    })
}

fn fdtd_config_for_room(geo: &RoomGeometry, c: f64) -> FdtdConfig {
    let w = (geo.bounds.max.x - geo.bounds.min.x).max(1.0);
    let h = (geo.bounds.max.y - geo.bounds.min.y).max(1.0);
    let nx = 64usize;
    let dx = w / nx as f64;
    let ny = ((h / dx).round() as usize).clamp(8, 128);
    FdtdConfig {
        nx,
        ny,
        dx,
        c,
        cfl: 0.9,
        admittance: [0.02; 4],
        origin_x: geo.bounds.min.x,
        origin_y: geo.bounds.min.y,
    }
}

/// Create the persistent FDTD grid (heatmap). Config JSON optional.
#[wasm_bindgen]
pub fn fdtd_new(cfg_json: &str) -> String {
    let v: Value = serde_json::from_str(cfg_json).unwrap_or(Value::Null);
    with_engine(|e| {
        let c = jf(&v, "c", 343.0);
        let cfg = if let Some(geo) = &e.room {
            let mut c = fdtd_config_for_room(geo, c);
            if let Some(a) = v.get("admittance").and_then(|x| x.as_array()) {
                for (i, x) in a.iter().take(4).enumerate() {
                    c.admittance[i] = x.as_f64().unwrap_or(0.02);
                }
            }
            c
        } else {
            FdtdConfig {
                nx: ju(&v, "nx", 64),
                ny: ju(&v, "ny", 48),
                dx: jf(&v, "dx", 0.1),
                c,
                cfl: jf(&v, "cfl", 0.9),
                admittance: jarr7_4(&v, "admittance", [0.02; 4]),
                ..Default::default()
            }
        };
        e.fdtd.grid = Fdtd2D::new(&cfg);
        json!({ "dt": e.fdtd.grid.dt, "nx": e.fdtd.grid.nx, "ny": e.fdtd.grid.ny, "dx": e.fdtd.grid.dx, "origin_x": cfg.origin_x, "origin_y": cfg.origin_y }).to_string()
    })
}

fn jarr7_4(v: &Value, key: &str, d: [f64; 4]) -> [f64; 4] {
    match v.get(key).and_then(|x| x.as_array()) {
        Some(a) => {
            let mut out = d;
            for (i, x) in a.iter().take(4).enumerate() {
                out[i] = x.as_f64().unwrap_or(d[i]);
            }
            out
        }
        None => d,
    }
}

/// Configure the live excitation: `{ "mode": "sine"|"sweep"|"impulse"|"off", "freq": 55, "x":.., "y":.. }`.
#[wasm_bindgen]
pub fn fdtd_excite(req_json: &str) -> String {
    let v: Value = serde_json::from_str(req_json).unwrap_or(Value::Null);
    let mode = js(&v, "mode", "sine");
    with_engine(|e| {
        e.fdtd.excitation = match mode.as_str() {
            "impulse" => Excitation::Impulse,
            "sweep" => Excitation::Sweep,
            "off" => Excitation::Off,
            _ => Excitation::Sine,
        };
        e.fdtd.freq = jf(&v, "freq", 55.0);
        e.fdtd.src = (jf(&v, "x", 0.0), jf(&v, "y", 0.0));
        e.fdtd.sweep_t = 0.0;
        json!({ "ok": true }).to_string()
    })
}

/// Advance the heatmap grid by `n` steps, applying the configured excitation,
/// and return the pressure field (row-major).
#[wasm_bindgen]
pub fn fdtd_step(n: usize) -> Vec<f32> {
    with_engine(|e| {
        let n = n.clamp(1, 4096);
        for _ in 0..n {
            let (src_x, src_y) = e.fdtd.src;
            let t = e.fdtd.grid.current_time();
            let val = match e.fdtd.excitation {
                Excitation::Off => 0.0,
                Excitation::Impulse => {
                    if e.fdtd.grid.current_time() == 0.0 {
                        1.0
                    } else {
                        0.0
                    }
                }
                Excitation::Sine => (2.0 * std::f64::consts::PI * e.fdtd.freq * t).sin(),
                Excitation::Sweep => {
                    // Logarithmic sweep 20 Hz → 20 kHz, looped.
                    let f0 = 20.0_f64;
                    let f1 = 20000.0_f64;
                    let d = e.fdtd.sweep_duration;
                    let phase = 2.0 * std::f64::consts::PI * f0 * d / (f1 / f0).ln()
                        * (((f1 / f0).powf(e.fdtd.sweep_t / d)) - 1.0);
                    (phase).sin()
                }
            };
            if val != 0.0 {
                e.fdtd.grid.inject(src_x, src_y, val);
            }
            if let Excitation::Sweep = e.fdtd.excitation {
                e.fdtd.sweep_t += e.fdtd.grid.dt;
                if e.fdtd.sweep_t >= e.fdtd.sweep_duration {
                    e.fdtd.sweep_t = 0.0;
                }
            }
            e.fdtd.grid.step();
        }
        e.fdtd.grid.grid().iter().map(|&x| x as f32).collect()
    })
}

#[wasm_bindgen]
pub fn fdtd_reset() {
    with_engine(|e| e.fdtd.grid.reset());
}

/// Record the low-frequency modal response at the listener: creates a temporary
/// grid, injects an impulse at the source, and records the pressure at the
/// listener for `duration` seconds. Returns the recorded signal (Float32Array);
/// its sample rate is `1 / fdtd_dt()`.
#[wasm_bindgen]
pub fn fdtd_record(req_json: &str) -> Vec<f32> {
    let v: Value = serde_json::from_str(req_json).unwrap_or(Value::Null);
    let duration = jf(&v, "duration", 1.2);
    let src = jvec3(&v, "source");
    let listener = jvec3(&v, "listener");
    with_engine(|e| {
        let c = SPEED_OF_SOUND;
        let cfg = match &e.room {
            Some(geo) => fdtd_config_for_room(geo, c),
            None => FdtdConfig { nx: 64, ny: 48, ..Default::default() },
        };
        let mut grid = Fdtd2D::new(&cfg);
        let dt = grid.dt;
        let steps = ((duration / dt) as usize).clamp(1, 200_000);
        grid.inject(src.x, src.y, 1.0);
        let mut rec = Vec::with_capacity(steps);
        for _ in 0..steps {
            rec.push(grid.probe(listener.x, listener.y));
            grid.step();
        }
        // Store dt for the RIR stage.
        e.fdtd.grid.dt = dt;
        rec.iter().map(|&x| x as f32).collect()
    })
}

/// Full simulation: build room (or reuse), compute early + ray + FDTD modal,
/// synthesize the RIR and metrics. Returns a JSON summary; buffers are fetched
/// via the `rir_*` accessors.
#[wasm_bindgen]
pub fn synthesize(req_json: &str) -> String {
    let v: Value = match serde_json::from_str(req_json) {
        Ok(v) => v,
        Err(e) => return json!({ "error": format!("bad JSON: {e}") }).to_string(),
    };

    // Build room if provided.
    if v.get("room").is_some() {
        if let Err(e) = build_from_value(&v).map(|(geo, mats)| {
            with_engine(|eng| {
                eng.room = Some(geo);
                eng.materials = mats;
            });
        }) {
            return json!({ "error": e }).to_string();
        }
    }

    let source = jvec3(&v, "source");
    let listener = jvec3(&v, "listener");
    let image_order = ju(&v, "image_order", 3).clamp(1, 4) as u32;

    let ray_opts = RayOptions {
        ray_count: ju(&v, "ray_count", 20000).clamp(64, 1_000_000),
        max_bounces: ju(&v, "max_bounces", 40).clamp(1, 200),
        max_time: jf(&v, "max_time", 2.0),
        n_bins: ju(&v, "n_bins", 2048).clamp(16, 16384),
        temp_c: jf(&v, "temp_c", 20.0),
        humidity: jf(&v, "humidity", 50.0),
        seed: ju(&v, "seed", 0x5eed_5eed) as u64,
        max_viz_rays: 0,
        directivity: js(&v, "directivity", "omnidirectional"),
    };
    let rir_opts = RirOptions {
        sample_rate: jf(&v, "sample_rate", 48000.0),
        decay_length: jf(&v, "decay_length", 3.0).clamp(0.2, 5.0),
        crossover_freq: jf(&v, "crossover_freq", 300.0).clamp(100.0, 500.0),
        seed: ju(&v, "rir_seed", 0xc0ffee) as u64,
        late_energy_ratio: jf(&v, "late_energy_ratio", 2.0),
        normalize: true,
        source_azimuth_deg: jf(&v, "source_azimuth_deg", 0.0),
    };
    let modal_duration = jf(&v, "modal_duration", 1.2).clamp(0.3, 2.0);

    with_engine(|e| {
        let Some(room) = &e.room else {
            return json!({ "error": "call build_room first" }).to_string();
        };
        let materials = e.materials.clone();
        let dv = js(&v, "directivity", "omnidirectional");
        let refl = image_source::early_reflections(room, source, listener, &materials, image_order, SPEED_OF_SOUND, &dv);
        let ray = ray_tracer::trace_rays(room, source, &materials, &ray_opts);

        // FDTD modal recording.
        let cfg = fdtd_config_for_room(room, SPEED_OF_SOUND);
        let mut grid = Fdtd2D::new(&cfg);
        let fdtd_dt = grid.dt;
        let steps = ((modal_duration / fdtd_dt) as usize).clamp(1, 200_000);
        grid.inject(source.x, source.y, 1.0);
        let mut modal = Vec::with_capacity(steps);
        for _ in 0..steps {
            modal.push(grid.probe(listener.x, listener.y));
            grid.step();
        }

        let rir = synthesize_rir(room, &materials, source, listener, &refl, &ray, &modal, fdtd_dt, &rir_opts);

        let metrics = compute_metrics_json(&rir, &refl, &ray, room, &materials, source, listener, rir_opts.sample_rate);
        e.source = source;
        e.listener = listener;
        e.rir = Some(rir);
        e.metrics = Some(metrics.to_string());
        metrics.to_string()
    })
}

/// Build the metrics JSON from the RIR and the solvers' outputs.
#[allow(clippy::too_many_arguments)]
fn compute_metrics_json(
    rir: &RirBuffers,
    refl: &[image_source::EarlyReflection],
    ray: &RayTraceResult,
    room: &RoomGeometry,
    materials: &[Material],
    source: Vec3,
    listener: Vec3,
    sample_rate: f64,
) -> Value {
    let mono: Vec<f64> = rir.mono.iter().map(|&v| v as f64).collect();
    let edc = schroeder_edc_db(&mono);
    let (rt60_t30, rt60_t20, edt) = rt60_t20_t30_edt(&edc, sample_rate);
    let (c50, c80) = clarity(&mono, sample_rate);
    let direct_t = source.sub(listener).len() / SPEED_OF_SOUND;
    let itdg = refl.first().map(|r| (r.time - direct_t).max(0.0)).unwrap_or(0.0);

    let band_rt = band_rt60_from_histogram(&ray.histogram, ray.bin_duration);
    let sabine: Vec<f64> = (0..N_BANDS).map(|b| sabine_rt60(room, materials, b)).collect();

    // Magnitude frequency response (downsampled).
    let (freqs, mags) = fft::magnitude_spectrum_db(&mono, sample_rate);
    let step = (freqs.len() / 512).max(1);
    let freq_hz: Vec<f64> = freqs.iter().step_by(step).cloned().collect();
    let mag_db: Vec<f64> = mags.iter().step_by(step).cloned().collect();

    // Room modes (bounding-box approximation).
    let lx = room.bounds.max.x - room.bounds.min.x;
    let ly = room.bounds.max.y - room.bounds.min.y;
    let lz = room.bounds.max.z - room.bounds.min.z;
    let modes = room_modes((lx.max(0.1), ly.max(0.1), lz.max(0.1)), SPEED_OF_SOUND, 300.0);

    // SBIR dips from first-order reflections.
    let direct_dist = source.sub(listener).len();
    let mut sbir: Vec<Value> = Vec::new();
    for r in refl.iter().filter(|r| r.order == 1).take(4) {
        let path_diff = (r.distance - direct_dist).abs();
        if path_diff > 0.05 {
            for n in 1..=4 {
                let f = (2 * n - 1) as f64 * SPEED_OF_SOUND / (2.0 * path_diff);
                if f <= 1000.0 {
                    sbir.push(json!({ "freq": f, "path_diff": path_diff, "order": r.order }));
                }
            }
        }
    }

    // Comb / flutter-echo warning: near-uniform reflection spacing.
    let mut intervals: Vec<f64> = Vec::new();
    for w in refl.windows(2) {
        intervals.push((w[1].time - w[0].time) * 1000.0);
    }
    let comb_warning = intervals.len() >= 3
        && intervals.windows(3).any(|w| (w[0] - w[1]).abs() < w[0] * 0.08 && (w[1] - w[2]).abs() < w[1] * 0.08);

    json!({
        "sample_rate": sample_rate,
        "length": rir.length,
        "rt60_t30": rt60_t30,
        "rt60_t20": rt60_t20,
        "edt": edt,
        "c50": c50,
        "c80": c80,
        "itdg": itdg,
        "band_rt60": band_rt,
        "sabine_rt60": sabine,
        "band_centers": OCTAVE_BANDS_HZ,
        "freq_hz": freq_hz,
        "mag_db": mag_db,
        "modes": modes,
        "sbir": sbir,
        "comb_filter_warning": comb_warning,
        "early_reflection_count": refl.len(),
        "ray_count": ray.ray_count,
    })
}

// -------------------------------------------------------- RIR buffer accessors

fn rir_with<T>(f: impl FnOnce(&RirBuffers) -> T) -> Option<T> {
    ENGINE.with(|e| e.borrow().rir.as_ref().map(f))
}

#[wasm_bindgen]
pub fn rir_mono() -> Vec<f32> {
    rir_with(|r| r.mono.clone()).unwrap_or_default()
}
#[wasm_bindgen]
pub fn rir_stereo_l() -> Vec<f32> {
    rir_with(|r| r.stereo_l.clone()).unwrap_or_default()
}
#[wasm_bindgen]
pub fn rir_stereo_r() -> Vec<f32> {
    rir_with(|r| r.stereo_r.clone()).unwrap_or_default()
}
#[wasm_bindgen]
pub fn rir_binaural_l() -> Vec<f32> {
    rir_with(|r| r.binaural_l.clone()).unwrap_or_default()
}
#[wasm_bindgen]
pub fn rir_binaural_r() -> Vec<f32> {
    rir_with(|r| r.binaural_r.clone()).unwrap_or_default()
}

/// Export the active RIR as a WAV. `format` ∈ {float32, pcm24, pcm16},
/// `channels` ∈ {mono, stereo, binaural}.
#[wasm_bindgen]
pub fn rir_wav(format: &str, channels: &str) -> Vec<u8> {
    let fmt = WavFormat::from_str(format);
    rir_with(|r| {
        let sr = r.sample_rate as u32;
        match channels {
            "stereo" => {
                let mut inter = Vec::with_capacity(r.length * 2);
                for i in 0..r.length {
                    inter.push(r.stereo_l[i]);
                    inter.push(r.stereo_r[i]);
                }
                write_wav(&inter, sr, 2, fmt)
            }
            "binaural" => {
                let mut inter = Vec::with_capacity(r.length * 2);
                for i in 0..r.length {
                    inter.push(r.binaural_l[i]);
                    inter.push(r.binaural_r[i]);
                }
                write_wav(&inter, sr, 2, fmt)
            }
            _ => write_wav(&r.mono, sr, 1, fmt),
        }
    })
    .unwrap_or_default()
}

/// Latest metrics JSON (set by `synthesize`).
#[wasm_bindgen]
pub fn metrics_json() -> String {
    ENGINE.with(|e| e.borrow().metrics.clone().unwrap_or_else(|| "{}".into()))
}

/// Room mode frequencies for arbitrary dimensions: `{ "dims": [lx,ly,lz], "c": 343 }`.
#[wasm_bindgen]
pub fn room_modes_json(req_json: &str) -> String {
    let v: Value = serde_json::from_str(req_json).unwrap_or(Value::Null);
    let dims = v.get("dims").and_then(|x| x.as_array());
    let (lx, ly, lz) = match dims {
        Some(a) => (
            a.get(0).and_then(|x| x.as_f64()).unwrap_or(5.0),
            a.get(1).and_then(|x| x.as_f64()).unwrap_or(4.0),
            a.get(2).and_then(|x| x.as_f64()).unwrap_or(2.7),
        ),
        None => (5.0, 4.0, 2.7),
    };
    let c = jf(&v, "c", SPEED_OF_SOUND);
    let modes = room_modes((lx, ly, lz), c, jf(&v, "max_freq", 300.0));
    serde_json::to_string(&modes).unwrap_or_else(|_| "[]".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_materials() -> Vec<Material> {
        (0..16).map(|_| material::preset("drywall")).collect()
    }

    #[test]
    fn version_works() {
        assert!(!version().is_empty());
    }

    #[test]
    fn build_room_returns_summary() {
        let spec = RoomSpec::default();
        let req = json!({ "room": spec, "materials": default_materials() });
        let out = build_room(&req.to_string());
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(v.get("surfaces").unwrap().as_u64().unwrap() >= 6);
        assert!(v.get("volume").unwrap().as_f64().unwrap() > 0.0);
    }

    #[test]
    fn build_room_rejects_degenerate() {
        let spec = RoomSpec { floorplan: vec![], ..RoomSpec::default() };
        let req = json!({ "room": spec });
        let out = build_room(&req.to_string());
        assert!(out.contains("error"));
    }

    #[test]
    fn full_synthesize_pipeline() {
        let spec = RoomSpec::default();
        let req = json!({
            "room": spec,
            "materials": default_materials(),
            "source": {"x": 0.0, "y": 0.0, "z": 1.2},
            "listener": {"x": 2.0, "y": 1.0, "z": 1.2},
            "image_order": 2,
            "ray_count": 2000,
            "max_bounces": 15,
            "max_time": 1.0,
            "n_bins": 256,
            "sample_rate": 48000.0,
            "decay_length": 0.6,
            "crossover_freq": 300.0,
            "modal_duration": 0.4,
        });
        let out = synthesize(&req.to_string());
        assert!(!out.contains("error"), "synthesize failed: {out}");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(v.get("rt60_t30").is_some());
        assert!(v.get("c50").is_some());
        // RIR buffers must be non-empty.
        let m = rir_mono();
        assert!(m.len() > 1000);
        // WAV export produces a valid header.
        let w = rir_wav("float32", "mono");
        assert_eq!(&w[0..4], b"RIFF");
    }

    #[test]
    fn early_and_ray_stages() {
        let spec = RoomSpec::default();
        let req = json!({ "room": spec, "materials": default_materials() });
        build_room(&req.to_string());
        let er = early_reflections(r#"{"source":{"x":0,"y":0,"z":1.2},"listener":{"x":2,"y":1,"z":1.2},"max_order":2}"#);
        let er_v: Value = serde_json::from_str(&er).unwrap();
        assert!(er_v["reflections"].as_array().unwrap().len() > 0);
        let tr = trace_rays(r#"{"ray_count":500,"max_bounces":5,"max_time":0.5,"n_bins":64}"#);
        let tr_v: Value = serde_json::from_str(&tr).unwrap();
        assert!(tr_v["histogram"].as_array().unwrap().len() > 0);
    }

    #[test]
    fn fdtd_step_produces_field() {
        fdtd_new("{}");
        fdtd_excite(r#"{"mode":"impulse"}"#);
        let g = fdtd_step(10);
        assert!(!g.is_empty());
        assert!(g.iter().any(|&v| v != 0.0));
        fdtd_reset();
    }
}
