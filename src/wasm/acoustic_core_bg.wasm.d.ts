/* tslint:disable */
/* eslint-disable */
export const memory: WebAssembly.Memory;
export const build_room: (a: number, b: number) => [number, number];
export const early_reflections: (a: number, b: number) => [number, number];
export const fdtd_excite: (a: number, b: number) => [number, number];
export const fdtd_new: (a: number, b: number) => [number, number];
export const fdtd_record: (a: number, b: number) => [number, number];
export const fdtd_reset: () => void;
export const fdtd_step: (a: number) => [number, number];
export const metrics_json: () => [number, number];
export const rir_binaural_l: () => [number, number];
export const rir_binaural_r: () => [number, number];
export const rir_mono: () => [number, number];
export const rir_stereo_l: () => [number, number];
export const rir_stereo_r: () => [number, number];
export const rir_wav: (a: number, b: number, c: number, d: number) => [number, number];
export const room_modes_json: (a: number, b: number) => [number, number];
export const synthesize: (a: number, b: number) => [number, number];
export const trace_rays: (a: number, b: number) => [number, number];
export const version: () => [number, number];
export const __wbindgen_externrefs: WebAssembly.Table;
export const __wbindgen_malloc: (a: number, b: number) => number;
export const __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
export const __wbindgen_free: (a: number, b: number, c: number) => void;
export const __wbindgen_start: () => void;
