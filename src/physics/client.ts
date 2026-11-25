// Physics client: a promise-based proxy to the Web Worker that owns the WASM
// acoustic core. All heavy compute runs off the UI thread; RIR and FDTD buffers
// are transferred (zero-copy) back to the main thread.

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}
export interface Material {
  name: string;
  absorption: number[];
  scattering: number[];
  transmission: number[];
}
export interface ApertureSpec {
  wall_index: number;
  start: number;
  end: number;
  bottom: number;
  top: number;
  material: number;
}
export interface RoomSpec {
  floorplan: { x: number; y: number }[];
  height: number;
  wall_material: number;
  floor_material: number;
  ceiling_material: number;
  apertures: ApertureSpec[];
  furniture: unknown[];
}
export interface SimulationRequest {
  room: RoomSpec;
  materials: Material[];
  source: Vec3;
  listener: Vec3;
  image_order?: number;
  ray_count?: number;
  max_bounces?: number;
  max_time?: number;
  n_bins?: number;
  temp_c?: number;
  humidity?: number;
  seed?: number;
  sample_rate?: number;
  decay_length?: number;
  crossover_freq?: number;
  modal_duration?: number;
  source_azimuth_deg?: number;
  directivity?: string;
}
export interface MetricsReport {
  sample_rate: number;
  length: number;
  rt60_t30: number;
  rt60_t20: number;
  edt: number;
  c50: number;
  c80: number;
  itdg: number;
  band_rt60: number[];
  sabine_rt60: number[];
  band_centers: number[];
  freq_hz: number[];
  mag_db: number[];
  modes: { freq: number; nx: number; ny: number; nz: number; kind: string }[];
  sbir: { freq: number; path_diff: number; order: number }[];
  comb_filter_warning: boolean;
  early_reflection_count: number;
  ray_count: number;
}
export interface RirBuffers {
  mono: Float32Array;
  stereoL: Float32Array;
  stereoR: Float32Array;
  binauralL: Float32Array;
  binauralR: Float32Array;
}

type Pending = {
  resolve: (v: Record<string, unknown>) => void;
  reject: (e: Error) => void;
};

class PhysicsClient {
  private worker: Worker | null = null;
  private pending = new Map<number, Pending>();
  private seq = 0;

  private ensureWorker(): Worker {
    if (this.worker) return this.worker;
    this.worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
    this.worker.onmessage = (ev: MessageEvent) => {
      const data = ev.data as { id: number; [k: string]: unknown };
      const p = this.pending.get(data.id);
      if (!p) return;
      this.pending.delete(data.id);
      if (data.error) p.reject(new Error(String(data.error)));
      else p.resolve(data);
    };
    this.worker.onerror = (ev) => {
      for (const [, p] of this.pending) p.reject(new Error(ev.message || 'worker error'));
      this.pending.clear();
    };
    return this.worker;
  }

  private call(type: string, payload: Record<string, unknown>): Promise<Record<string, unknown>> {
    const worker = this.ensureWorker();
    const id = ++this.seq;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      worker.postMessage({ id, type, ...payload });
    });
  }

  version(): Promise<string> {
    return this.call('version', {}).then((r) => String(r.version));
  }

  buildRoom(spec: { room: RoomSpec; materials: Material[] }): Promise<string> {
    return this.call('build_room', { spec }).then((r) => String(r.summary));
  }

  synthesize(req: SimulationRequest): Promise<{ summary: string; metrics: MetricsReport; rir: RirBuffers }> {
    return this.call('synthesize', { req }).then((r) => ({
      summary: String(r.summary),
      metrics: JSON.parse(String(r.metrics)),
      rir: r.rir as unknown as RirBuffers,
    }));
  }

  traceRays(req: Record<string, unknown>): Promise<string> {
    return this.call('trace_rays', { req }).then((r) => String(r.result));
  }

  earlyReflections(req: Record<string, unknown>): Promise<string> {
    return this.call('early_reflections', { req }).then((r) => String(r.result));
  }

  fdtdNew(cfg: Record<string, unknown> = {}): Promise<string> {
    return this.call('fdtd_new', { cfg }).then((r) => String(r.summary));
  }

  fdtdExcite(req: Record<string, unknown>): Promise<void> {
    return this.call('fdtd_excite', { req }).then(() => undefined);
  }

  fdtdStep(n: number): Promise<Float32Array> {
    return this.call('fdtd_step', { n }).then((r) => r.grid as Float32Array);
  }

  fdtdReset(): Promise<void> {
    return this.call('fdtd_reset', {}).then(() => undefined);
  }

  exportWav(format: string, channels: string): Promise<Uint8Array> {
    return this.call('export_wav', { format, channels }).then((r) => r.bytes as Uint8Array);
  }
}

export const physics = new PhysicsClient();
