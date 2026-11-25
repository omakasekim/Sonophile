// Application wiring: binds the store, physics worker, audio engine and viewport
// together; renders diagnostics and the acoustic-treatment advisor.

import { physics } from '../physics/client';
import type { MetricsReport, SimulationRequest } from '../physics/client';
import { audioEngine } from '../audio/engine';
import type { StemKind } from '../audio/stems';
import { store, defaultState } from '../state/store';
import { renderViewport, type RenderData } from '../render/viewport';

const $ = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;

const canvas = $<HTMLCanvasElement>('viewport');
const ctx = canvas.getContext('2d')!;

let currentMetrics: MetricsReport | null = null;
let rayCache: number[][][] = [];
let fdtdInfo: { grid: Float32Array; nx: number; ny: number; dx: number; originX: number; originY: number } | null = null;
let animating = false;

function resizeCanvas(): void {
  const rect = canvas.parentElement!.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.floor(rect.width * dpr);
  canvas.height = Math.floor(rect.height * dpr);
  canvas.style.width = `${rect.width}px`;
  canvas.style.height = `${rect.height}px`;
}

function currentRenderData(): RenderData {
  const mode = $<HTMLSelectElement>('view-mode').value as 'floorplan' | '3d';
  const overlay = $<HTMLSelectElement>('overlay').value as RenderData['overlay'];
  return { mode, overlay, fdtd: fdtdInfo ?? undefined, rays: rayCache, early: earlyCache };
}

let earlyCache: import('../render/viewport').EarlyReflection[] = [];

function render(): void {
  renderViewport(ctx, canvas.width, canvas.height, store.state, currentRenderData());
}

function buildRequest(): SimulationRequest {
  const s = store.state;
  return {
    room: s.room,
    materials: s.materials,
    source: s.source,
    listener: s.listener,
    image_order: 3,
    ray_count: 4000,
    max_bounces: 24,
    max_time: 1.5,
    n_bins: 1024,
    temp_c: 20,
    humidity: 50,
    seed: 0x5eed5eed,
    sample_rate: 48000,
    decay_length: 2.0,
    crossover_freq: 300,
    modal_duration: 0.35,
    source_azimuth_deg: azimuthDeg(),
    directivity: s.directivity,
  };
}

function azimuthDeg(): number {
  const s = store.state;
  const dx = s.listener.x - s.source.x;
  const dy = s.listener.y - s.source.y;
  return (Math.atan2(dy, dx) * 180) / Math.PI;
}

async function compute(playAfter = false): Promise<void> {
  const req = buildRequest();
  $('btn-run').textContent = 'Computing…';
  try {
    const { metrics, rir } = await physics.synthesize(req);
    currentMetrics = metrics;
    audioEngine.setRir(rir);
    renderMetrics(metrics);
    renderFrequencyPlot(metrics);
    renderAdvisor(metrics);
    if (playAfter) await audioEngine.playStem('drum');
  } catch (err) {
    console.error(err);
    $('metrics').textContent = `Error: ${err instanceof Error ? err.message : String(err)}`;
  } finally {
    $('btn-run').textContent = '▶ Compute RIR';
  }
}

async function loadOverlays(): Promise<void> {
  const overlay = $<HTMLSelectElement>('overlay').value;
  if (overlay === 'rays' || overlay === 'decay') {
    const res = await physics.traceRays({
      source: store.state.source,
      ray_count: 400,
      max_bounces: 20,
      max_time: 0.8,
      n_bins: 256,
      max_viz_rays: 300,
    });
    rayCache = JSON.parse(res).ray_paths as number[][][];
  }
  if (overlay !== 'none') {
    const er = await physics.earlyReflections({
      source: store.state.source,
      listener: store.state.listener,
      max_order: 2,
    });
    earlyCache = JSON.parse(er).reflections as import('../render/viewport').EarlyReflection[];
  }
  render();
}

async function startFdtd(): Promise<void> {
  if (animating) return;
  animating = true;
  const summary = JSON.parse(await physics.fdtdNew({}));
  await physics.fdtdExcite({ mode: 'sine', freq: 55, x: store.state.source.x, y: store.state.source.y });
  fdtdInfo = {
    grid: new Float32Array(0),
    nx: summary.nx,
    ny: summary.ny,
    dx: summary.dx,
    originX: summary.origin_x,
    originY: summary.origin_y,
  };
  const loop = async () => {
    if (!animating) return;
    fdtdInfo!.grid = await physics.fdtdStep(4);
    render();
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
}

function stopFdtd(): void {
  animating = false;
  fdtdInfo = null;
  physics.fdtdReset();
}

function renderMetrics(m: MetricsReport): void {
  const fmt = (v: number, unit = '') => (isFinite(v) ? `${v.toFixed(2)}${unit}` : '∞');
  $('metrics').innerHTML = `
    <table>
      <tr><td>RT60 (T30)</td><td>${fmt(m.rt60_t30, ' s')}</td></tr>
      <tr><td>RT60 (T20)</td><td>${fmt(m.rt60_t20, ' s')}</td></tr>
      <tr><td>EDT</td><td>${fmt(m.edt, ' s')}</td></tr>
      <tr><td>C50</td><td>${fmt(m.c50, ' dB')}</td></tr>
      <tr><td>C80</td><td>${fmt(m.c80, ' dB')}</td></tr>
      <tr><td>ITDG</td><td>${fmt(m.itdg * 1000, ' ms')}</td></tr>
      <tr><td>Early reflections</td><td>${m.early_reflection_count}</td></tr>
      <tr><td>Rays traced</td><td>${m.ray_count}</td></tr>
    </table>`;
}

function renderFrequencyPlot(m: MetricsReport): void {
  const c = $<HTMLCanvasElement>('freq-plot');
  const g = c.getContext('2d')!;
  g.clearRect(0, 0, c.width, c.height);
  const W = c.width, H = c.height;
  g.strokeStyle = '#2a313c';
  g.strokeRect(0, 0, W, H);
  if (!m.freq_hz.length) return;
  let minDb = Infinity, maxDb = -Infinity;
  for (const v of m.mag_db) { minDb = Math.min(minDb, v); maxDb = Math.max(maxDb, v); }
  const range = Math.max(1, maxDb - minDb);
  const logMin = Math.log10(Math.max(20, m.freq_hz[0]));
  const logMax = Math.log10(Math.min(20000, m.freq_hz[m.freq_hz.length - 1]));
  g.beginPath();
  for (let i = 0; i < m.freq_hz.length; i++) {
    const f = m.freq_hz[i];
    if (f < 20 || f > 20000) continue;
    const x = ((Math.log10(f) - logMin) / (logMax - logMin)) * W;
    const y = H - ((m.mag_db[i] - minDb) / range) * (H - 10) - 5;
    if (i === 0) g.moveTo(x, y); else g.lineTo(x, y);
  }
  g.strokeStyle = '#4cc2ff';
  g.stroke();
}

function renderAdvisor(m: MetricsReport): void {
  const hints: { text: string; ok: boolean }[] = [];
  if (m.comb_filter_warning) {
    hints.push({ text: 'Flutter echo detected — add angled or diffusive surfaces to break up parallel reflections.', ok: false });
  }
  const dips = m.sbir.filter((d) => d.freq < 400);
  if (dips.length) {
    const f = dips[0].freq.toFixed(0);
    hints.push({ text: `SBIR dip near ${f} Hz — treat the first-reflection point on the nearest boundary.`, ok: false });
  }
  const axial = m.modes.filter((mo) => mo.kind === 'Axial' && mo.freq < 120);
  if (axial.length >= 2) {
    const freqs = axial.map((mo) => mo.freq.toFixed(0)).join(', ');
    hints.push({ text: `Low-frequency modal buildup at ${freqs} Hz — consider corner bass traps.`, ok: false });
  }
  if (isFinite(m.rt60_t30) && m.rt60_t30 > 1.5) {
    hints.push({ text: `RT60 ${m.rt60_t30.toFixed(2)} s is long — add broadband absorption for a tighter room.`, ok: false });
  }
  if (isFinite(m.c50) && m.c50 < 0) {
    hints.push({ text: 'Speech clarity is low — absorb first reflections near source and listener.', ok: false });
  }
  if (isFinite(m.rt60_t30) && m.rt60_t30 > 0.3 && m.rt60_t30 < 1.2) {
    hints.push({ text: 'Reverberation time is in a good range for a mixing/control room.', ok: true });
  }
  $('advisor').innerHTML = hints
    .map((h) => `<div class="hint${h.ok ? ' ok' : ''}">${h.text}</div>`)
    .join('');
}

function populateMaterials(): void {
  const names = store.state.materials.map((m) => m.name);
  for (const id of ['mat-wall', 'mat-floor', 'mat-ceiling']) {
    const sel = $(id);
    sel.innerHTML = names.map((n, i) => `<option value="${i}">${n}</option>`).join('');
  }
  $<HTMLSelectElement>('mat-wall').value = String(store.state.room.wall_material);
  $<HTMLSelectElement>('mat-floor').value = String(store.state.room.floor_material);
  $<HTMLSelectElement>('mat-ceiling').value = String(store.state.room.ceiling_material);
}

function bindUI(): void {
  store.subscribe((s) => {
    // Reflect slider values.
    $<HTMLInputElement>('src-x').value = String(s.source.x);
    $<HTMLInputElement>('src-y').value = String(s.source.y);
    $<HTMLInputElement>('lst-x').value = String(s.listener.x);
    $<HTMLInputElement>('lst-y').value = String(s.listener.y);
    $<HTMLInputElement>('room-w').value = String(s.room.floorplan[1].x - s.room.floorplan[0].x);
    render();
  });

  const range = (id: string, fn: (v: number) => void) => {
    $(id).addEventListener('input', (e) => fn(parseFloat((e.target as HTMLInputElement).value)));
  };

  range('src-x', (v) => store.commit((s) => { s.source.x = v; }));
  range('src-y', (v) => store.commit((s) => { s.source.y = v; }));
  range('lst-x', (v) => store.commit((s) => { s.listener.x = v; }));
  range('lst-y', (v) => store.commit((s) => { s.listener.y = v; }));

  range('room-w', (v) => {
    store.commit((s) => {
      const w = v, d = s.room.floorplan[2].y - s.room.floorplan[0].y;
      s.room.floorplan = [
        { x: -w / 2, y: -d / 2 }, { x: w / 2, y: -d / 2 },
        { x: w / 2, y: d / 2 }, { x: -w / 2, y: d / 2 },
      ];
    });
  });
  range('room-d', (v) => {
    store.commit((s) => {
      const w = s.room.floorplan[1].x - s.room.floorplan[0].x, d = v;
      s.room.floorplan = [
        { x: -w / 2, y: -d / 2 }, { x: w / 2, y: -d / 2 },
        { x: w / 2, y: d / 2 }, { x: -w / 2, y: d / 2 },
      ];
    });
  });
  range('room-h', (v) => store.commit((s) => { s.room.height = v; }));

  $<HTMLSelectElement>('mat-wall').addEventListener('change', (e) =>
    store.commit((s) => { s.room.wall_material = parseInt((e.target as HTMLSelectElement).value); }));
  $<HTMLSelectElement>('mat-floor').addEventListener('change', (e) =>
    store.commit((s) => { s.room.floor_material = parseInt((e.target as HTMLSelectElement).value); }));
  $<HTMLSelectElement>('mat-ceiling').addEventListener('change', (e) =>
    store.commit((s) => { s.room.ceiling_material = parseInt((e.target as HTMLSelectElement).value); }));

  $('directivity').addEventListener('change', (e) =>
    store.commit((s) => { s.directivity = (e.target as HTMLSelectElement).value; }));

  // Convolution params.
  const convMap: Record<string, keyof import('../audio/engine').ConvParams> = {
    drywet: 'wet', damping: 'damping', width: 'width',
  };
  for (const [id, key] of Object.entries(convMap)) {
    range(id, (v) => {
      audioEngine.setParams({ [key]: v } as Partial<import('../audio/engine').ConvParams>);
      store.commit((s) => { (s.conv as unknown as Record<string, number>)[key] = v; });
    });
  }
  range('predelay', (v) => audioEngine.setParams({ predelayMs: v }));

  $('btn-run').addEventListener('click', () => compute());
  $('btn-play').addEventListener('click', async () => {
    await audioEngine.ensure();
    await compute(true);
  });
  $('btn-mic').addEventListener('click', async () => {
    await audioEngine.toggleMic();
  });
  $('btn-export').addEventListener('click', async () => {
    const bytes = await physics.exportWav('float32', 'stereo');
    const blob = new Blob([bytes as unknown as ArrayBuffer], { type: 'audio/wav' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = 'sonophile-rir.wav';
    a.click();
  });

  $('btn-snapshot-a').addEventListener('click', () => store.captureSnapshot('A'));
  $('btn-snapshot-b').addEventListener('click', () => store.captureSnapshot('B'));
  $('btn-compare').addEventListener('click', async () => {
    // A/B: load snapshot A, compute, then B (user toggles via buttons).
    store.loadSnapshot('A');
    await compute();
    store.loadSnapshot('B');
    await compute();
  });
  $('btn-undo').addEventListener('click', () => store.undo());
  $('btn-redo').addEventListener('click', () => store.redo());

  $('view-mode').addEventListener('change', () => render());
  $('overlay').addEventListener('change', async () => {
    stopFdtd();
    if ($<HTMLSelectElement>('overlay').value === 'fdtd') await startFdtd();
    else await loadOverlays();
  });

  window.addEventListener('resize', () => { resizeCanvas(); render(); });
}

export async function initApp(): Promise<void> {
  resizeCanvas();
  populateMaterials();
  bindUI();
  store.subscribe(() => {});
  render();
  try {
    const v = await physics.version();
    console.log('[sonophile] acoustic core version', v);
  } catch (e) {
    console.warn('[sonophile] worker init pending:', e);
  }
}
