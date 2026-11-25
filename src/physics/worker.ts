// Web Worker that owns the WebAssembly acoustic core, keeping the heavy
// ray-tracing / FDTD / RIR synthesis off both the UI thread and the audio
// render thread. Results (including Float32Array buffers) are posted back with
// transferable array buffers.

import init, {
  build_room,
  synthesize,
  trace_rays,
  early_reflections,
  fdtd_new,
  fdtd_excite,
  fdtd_step,
  fdtd_reset,
  rir_mono,
  rir_stereo_l,
  rir_stereo_r,
  rir_binaural_l,
  rir_binaural_r,
  rir_wav,
  metrics_json,
  room_modes_json,
  version,
} from '../wasm/acoustic_core.js';

const ready = init();

self.onmessage = async (event: MessageEvent) => {
  const msg = event.data as { id: number; type: string; [k: string]: unknown };
  await ready;
  try {
    switch (msg.type) {
      case 'synthesize': {
        const summary = synthesize(JSON.stringify(msg.req));
        const rir = {
          mono: rir_mono(),
          stereoL: rir_stereo_l(),
          stereoR: rir_stereo_r(),
          binauralL: rir_binaural_l(),
          binauralR: rir_binaural_r(),
        };
        const metrics = metrics_json();
        const transfer = [
          rir.mono.buffer,
          rir.stereoL.buffer,
          rir.stereoR.buffer,
          rir.binauralL.buffer,
          rir.binauralR.buffer,
        ];
        (self as unknown as Worker).postMessage(
          { id: msg.id, summary, metrics, rir },
          transfer
        );
        break;
      }
      case 'build_room': {
        const summary = build_room(JSON.stringify(msg.spec));
        (self as unknown as Worker).postMessage({ id: msg.id, summary });
        break;
      }
      case 'trace_rays': {
        const result = trace_rays(JSON.stringify(msg.req));
        (self as unknown as Worker).postMessage({ id: msg.id, result });
        break;
      }
      case 'early_reflections': {
        const result = early_reflections(JSON.stringify(msg.req));
        (self as unknown as Worker).postMessage({ id: msg.id, result });
        break;
      }
      case 'fdtd_new': {
        const summary = fdtd_new(JSON.stringify(msg.cfg ?? {}));
        (self as unknown as Worker).postMessage({ id: msg.id, summary });
        break;
      }
      case 'fdtd_excite': {
        fdtd_excite(JSON.stringify(msg.req));
        (self as unknown as Worker).postMessage({ id: msg.id });
        break;
      }
      case 'fdtd_step': {
        const grid = fdtd_step((msg.n as number) ?? 1);
        (self as unknown as Worker).postMessage({ id: msg.id, grid }, [grid.buffer]);
        break;
      }
      case 'fdtd_reset': {
        fdtd_reset();
        (self as unknown as Worker).postMessage({ id: msg.id });
        break;
      }
      case 'room_modes': {
        const result = room_modes_json(JSON.stringify(msg.req));
        (self as unknown as Worker).postMessage({ id: msg.id, result });
        break;
      }
      case 'export_wav': {
        const bytes = rir_wav(msg.format as string, msg.channels as string);
        (self as unknown as Worker).postMessage({ id: msg.id, bytes }, [bytes.buffer]);
        break;
      }
      case 'version': {
        (self as unknown as Worker).postMessage({ id: msg.id, version: version() });
        break;
      }
      default:
        (self as unknown as Worker).postMessage({ id: msg.id, error: `unknown message ${msg.type}` });
    }
  } catch (err) {
    (self as unknown as Worker).postMessage({
      id: msg.id,
      error: err instanceof Error ? err.message : String(err),
    });
  }
};
