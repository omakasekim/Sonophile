// Procedural test signals and dry stems — the app is fully testable with zero
// external audio files.

export type StemKind =
  | 'impulse'
  | 'sweep'
  | 'noise'
  | 'drum'
  | 'chord'
  | 'voice';

export function stemDuration(kind: StemKind): number {
  switch (kind) {
    case 'impulse':
      return 0.5;
    case 'drum':
      return 2.0;
    case 'chord':
      return 3.0;
    case 'voice':
      return 2.5;
    default:
      return 3.0;
  }
}

/** Generate a mono stem. */
export function generateStem(kind: StemKind, sampleRate: number, duration?: number): Float32Array {
  const dur = duration ?? stemDuration(kind);
  const n = Math.floor(sampleRate * dur);
  const out = new Float32Array(n);
  switch (kind) {
    case 'impulse':
      out[0] = 1.0;
      break;
    case 'sweep':
      fillLogSweep(out, sampleRate, 20, 20000);
      break;
    case 'noise':
      fillPinkNoise(out);
      break;
    case 'drum':
      fillDrum(out, sampleRate);
      break;
    case 'chord':
      fillChord(out, sampleRate);
      break;
    case 'voice':
      fillVoice(out, sampleRate);
      break;
  }
  return out;
}

function fillLogSweep(buf: Float32Array, sr: number, f0: number, f1: number): void {
  const n = buf.length;
  const t = n / sr;
  let phase = 0;
  for (let i = 0; i < n; i++) {
    const tt = (i / n) * t;
    // Instantaneous frequency f(τ) = f0·(f1/f0)^(τ/t).
    const f = f0 * Math.pow(f1 / f0, tt / t);
    phase += (2 * Math.PI * f) / sr;
    buf[i] = 0.6 * Math.sin(phase);
  }
}

function fillPinkNoise(buf: Float32Array): void {
  // Paul Kellet's economical pink-noise filter.
  let b0 = 0, b1 = 0, b2 = 0, b3 = 0, b4 = 0, b5 = 0, b6 = 0;
  for (let i = 0; i < buf.length; i++) {
    const white = Math.random() * 2 - 1;
    b0 = 0.99886 * b0 + white * 0.0555179;
    b1 = 0.99332 * b1 + white * 0.0750759;
    b2 = 0.969 * b2 + white * 0.153852;
    b3 = 0.8665 * b3 + white * 0.3104856;
    b4 = 0.55 * b4 + white * 0.5329522;
    b5 = -0.7616 * b5 - white * 0.016898;
    const pink = b0 + b1 + b2 + b3 + b4 + b5 + b6 + white * 0.5362;
    b6 = white * 0.115926;
    buf[i] = pink * 0.11;
  }
}

function fillDrum(buf: Float32Array, sr: number): void {
  const n = buf.length;
  // Kick: exponentially-decaying sine with pitch drop.
  let phase = 0;
  const kickLen = Math.min(n, Math.floor(sr * 0.6));
  for (let i = 0; i < kickLen; i++) {
    const t = i / sr;
    const f = 55 + 90 * Math.exp(-t * 30);
    phase += (2 * Math.PI * f) / sr;
    buf[i] += Math.sin(phase) * Math.exp(-t * 8);
  }
  // Snare: filtered noise burst at 0.25 s.
  const snareStart = Math.floor(sr * 0.25);
  let b0 = 0;
  for (let i = snareStart; i < Math.min(n, snareStart + Math.floor(sr * 0.2)); i++) {
    const t = (i - snareStart) / sr;
    const white = Math.random() * 2 - 1;
    b0 = 0.85 * b0 + 0.15 * white; // high-ish emphasis
    buf[i] += b0 * Math.exp(-t * 30) * 0.5;
  }
  // Normalize.
  normalize(buf);
}

function fillChord(buf: Float32Array, sr: number): void {
  const n = buf.length;
  // C major-ish pad with a few partials per note.
  const notes = [130.81, 164.81, 196.0, 261.63]; // C3 E3 G3 C4
  const partials = [1, 2, 3];
  for (let i = 0; i < n; i++) {
    const t = i / sr;
    const env = Math.min(1, t * 5) * Math.exp(-t * 0.9);
    let s = 0;
    for (const f0 of notes) {
      for (const p of partials) {
        s += Math.sin(2 * Math.PI * f0 * p * t) / (p * p);
      }
    }
    buf[i] = s * env * 0.12;
  }
}

function fillVoice(buf: Float32Array, sr: number): void {
  // Vowel-like formant synthesis: a glottal-ish saw buzz through three formant
  // resonances (approximated with summed resonant band-passes via biquad-like
  // two-pole resonators).
  const n = buf.length;
  const f0 = 140; // fundamental (pitch)
  const formants = [700, 1220, 2600]; // /a/-like vowel
  let phase = 0;
  // Two-pole resonator state per formant.
  const st = formants.map(() => ({ y1: 0, y2: 0 }));
  for (let i = 0; i < n; i++) {
    const t = i / sr;
    phase += (2 * Math.PI * f0) / sr;
    // Glottal source: sum of harmonics.
    let source = 0;
    for (let h = 1; h <= 8; h++) source += Math.sin(phase * h) / h;
    source *= 0.15;
    // Resonators.
    let v = source;
    for (let k = 0; k < formants.length; k++) {
      const f = formants[k];
      const r = Math.exp(-Math.PI * 80 / sr);
      const c = 2 * r * Math.cos(2 * Math.PI * f / sr);
      const y = v + c * st[k].y1 - r * r * st[k].y2;
      st[k].y2 = st[k].y1;
      st[k].y1 = y;
      v = y * (1 - r);
    }
    const env = Math.min(1, t * 8) * Math.exp(-t * 0.6);
    buf[i] = v * env;
  }
  normalize(buf);
}

function normalize(buf: Float32Array): void {
  let m = 0;
  for (let i = 0; i < buf.length; i++) m = Math.max(m, Math.abs(buf[i]));
  if (m > 0.001) {
    const k = 0.9 / m;
    for (let i = 0; i < buf.length; i++) buf[i] *= k;
  }
}
