/* tslint:disable */
/* eslint-disable */

/**
 * Build the room and material library; returns a summary (surface count, area,
 * volume, Sabine RT60 per band).
 */
export function build_room(spec_json: string): string;

/**
 * Compute early specular reflections (image-source). Request:
 * `{ "source": {x,y,z}, "listener": {x,y,z}, "max_order": 3 }`.
 */
export function early_reflections(req_json: string): string;

/**
 * Configure the live excitation: `{ "mode": "sine"|"sweep"|"impulse"|"off", "freq": 55, "x":.., "y":.. }`.
 */
export function fdtd_excite(req_json: string): string;

/**
 * Create the persistent FDTD grid (heatmap). Config JSON optional.
 */
export function fdtd_new(cfg_json: string): string;

/**
 * Record the low-frequency modal response at the listener: creates a temporary
 * grid, injects an impulse at the source, and records the pressure at the
 * listener for `duration` seconds. Returns the recorded signal (Float32Array);
 * its sample rate is `1 / fdtd_dt()`.
 */
export function fdtd_record(req_json: string): Float32Array;

export function fdtd_reset(): void;

/**
 * Advance the heatmap grid by `n` steps, applying the configured excitation,
 * and return the pressure field (row-major).
 */
export function fdtd_step(n: number): Float32Array;

/**
 * Latest metrics JSON (set by `synthesize`).
 */
export function metrics_json(): string;

export function rir_binaural_l(): Float32Array;

export function rir_binaural_r(): Float32Array;

export function rir_mono(): Float32Array;

export function rir_stereo_l(): Float32Array;

export function rir_stereo_r(): Float32Array;

/**
 * Export the active RIR as a WAV. `format` ∈ {float32, pcm24, pcm16},
 * `channels` ∈ {mono, stereo, binaural}.
 */
export function rir_wav(format: string, channels: string): Uint8Array;

/**
 * Room mode frequencies for arbitrary dimensions: `{ "dims": [lx,ly,lz], "c": 343 }`.
 */
export function room_modes_json(req_json: string): string;

/**
 * Full simulation: build room (or reuse), compute early + ray + FDTD modal,
 * synthesize the RIR and metrics. Returns a JSON summary; buffers are fetched
 * via the `rir_*` accessors.
 */
export function synthesize(req_json: string): string;

/**
 * Run the stochastic ray tracer. Returns `{ histogram, bin_duration, ray_count, ray_paths }`.
 */
export function trace_rays(req_json: string): string;

export function version(): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly build_room: (a: number, b: number) => [number, number];
    readonly early_reflections: (a: number, b: number) => [number, number];
    readonly fdtd_excite: (a: number, b: number) => [number, number];
    readonly fdtd_new: (a: number, b: number) => [number, number];
    readonly fdtd_record: (a: number, b: number) => [number, number];
    readonly fdtd_reset: () => void;
    readonly fdtd_step: (a: number) => [number, number];
    readonly metrics_json: () => [number, number];
    readonly rir_binaural_l: () => [number, number];
    readonly rir_binaural_r: () => [number, number];
    readonly rir_mono: () => [number, number];
    readonly rir_stereo_l: () => [number, number];
    readonly rir_stereo_r: () => [number, number];
    readonly rir_wav: (a: number, b: number, c: number, d: number) => [number, number];
    readonly room_modes_json: (a: number, b: number) => [number, number];
    readonly synthesize: (a: number, b: number) => [number, number];
    readonly trace_rays: (a: number, b: number) => [number, number];
    readonly version: () => [number, number];
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
