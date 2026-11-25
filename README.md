# Sonophile — Real-Time Spatial Audio & Room Acoustics Studio

A production-structured, GitHub-Pages-ready **3D/2.5D spatial audio + room
acoustic physics simulator, wave-field visualizer, and real-time convolution
auralization studio**. Hybrid physics (FDTD wave solver + image-source + ray
tracing), live AudioWorklet convolution, and an interactive editor — with zero
external runtime dependencies beyond the compiled WebAssembly core.

## Architecture

```
acoustic-core/            Rust → wasm32 (wasm-bindgen)
  material.rs             octave-band absorption/scattering/transmission + presets
  geom.rs                 polygon/ray/AABB intersections, directivity patterns
  room.rs                 floorplan→surfaces (walls/apertures/furniture), air abs., Sabine
  image_source.rs         exact specular reflections (orders 1–3) + occlusion
  ray_tracer.rs           stochastic ray tracing (Fibonacci sampling, per-band energy)
  fdtd.rs                 2-D/3-D FDTD pressure solver (CFL, impedance BCs)
  rir.rs                  hybrid RIR synthesis (LR4 crossover, stereo/binaural)
  metrics.rs              RT60 (T20/T30), EDT, C50/C80, ITDG, per-band decay
  modes.rs / fft.rs / wav.rs / filters.rs / prng.rs
  lib.rs                  wasm-bindgen API + orchestration

src/
  physics/worker.ts       Web Worker owning the WASM core (heavy compute off-UI)
  physics/client.ts       promise proxy + typed models
  audio/convolution-processor.js   uniform partitioned FFT convolution worklet
  audio/engine.ts         AudioContext, worklet, mic, stem playback
  audio/stems.ts          procedural test signals (impulse/sweep/noise/drum/chord/voice)
  state/store.ts          undo/redo, A/B snapshots, JSON + URL persistence
  render/viewport.ts      2-D floorplan + extruded 3-D + FDTD/ray/early overlays
  ui/app.ts               diagnostics UI + acoustic-treatment advisor
  coi.ts + public/coi-serviceworker.js   cross-origin isolation (safe fallback)

.github/workflows/deploy.yml   build Rust/WASM + frontend → GitHub Pages
```

## Build & run

```sh
# 1. Build the acoustic core → WASM (needs wasm-pack + wasm32 target)
cd acoustic-core
wasm-pack build --target web --out-dir ../src/wasm --release

# 2. Frontend
cd ..
npm install
npm run dev          # local dev server
npm run build        # production bundle → dist/
```

## How to use

1. **Design the room**: Load a preset floorplan or draw/edit walls, apertures, and acoustic materials (octave-band absorption/scattering).
2. **Place source & listener**: Drag the sound source and listener/head orientation in the 2-D/3-D viewport.
3. **Simulate & inspect**: View the live FDTD wave-field, specular image-source reflections, stochastic rays, and room metrics (`RT60`, `EDT`, `C50`/`C80`, `ITDG`).
4. **Auralize**: Choose a test stem (impulse, sweep, drum, chord, voice) or live microphone input to hear real-time partitioned FFT convolution through the synthesized Room Impulse Response (RIR), or export the RIR as WAV.

## GitHub Pages

The Vite config uses `base: './'`, so every asset (WASM, worklet, worker, shaders)
resolves relative to the document and works under `/<repo-name>/`, at the root,
or on a custom domain. `coi-serviceworker.js` injects `COOP`/`COEP` headers on the
document to enable cross-origin isolation (and therefore `SharedArrayBuffer` /
threaded WASM), with a single guarded reload and a single-threaded fallback. The
deploy workflow builds and publishes to Pages on every push to `main`.

## Verification checklist (see project write-up for details)

- **CFL stability** — `fdtd.rs` clamps `dt = cfl·dx/(c·√2)`; a long-run unit test
  asserts the field never blows up.
- **Enclosure/soundness** — 40 native unit tests cover geometry, air absorption,
  Sabine, ray decay, metrics, FFT, filters, binaural, WAV, and the full
  `synthesize` pipeline; `cargo test` passes.
- **Frontend build** — `tsc --noEmit` + `vite build` succeed; the bundle emits the
  WASM (377 KB), worker and worklet as relative-path assets.
- **Edge cases** — degenerate floorplans rejected, zero/empty absorption yields
  infinite RT60 (guarded to a fallback), division-by-zero avoided, out-of-domain
  `ln`/`sqrt` clamped.
