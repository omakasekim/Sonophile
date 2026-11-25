// Application wiring: binds the store, physics worker, audio engine + FX rack,
// interactive drag-and-drop canvas, live FDTD wave field, and audio visualizer.

import { physics } from '../physics/client';
import type { MetricsReport, SimulationRequest } from '../physics/client';
import { audioEngine } from '../audio/engine';
import type { StemKind } from '../audio/stems';
import { store } from '../state/store';
import {
  fitBounds,
  renderAudioVisualizer,
  renderViewport,
  screenToWorld,
  worldToScreen,
  type EarlyReflection,
  type RenderData,
} from '../render/viewport';

const $ = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;

const canvas = $<HTMLCanvasElement>('viewport');
const ctx = canvas.getContext('2d')!;
const vizCanvas = $<HTMLCanvasElement>('audio-viz');

let rayCache: number[][][] = [];
let earlyCache: EarlyReflection[] = [];
let fdtdInfo: { grid: Float32Array; nx: number; ny: number; dx: number; originX: number; originY: number } | null = null;
let fdtdRunning = false;
let fdtdStepping = false;
let loopEnabled = false;
let startTime = performance.now();

function resizeCanvas(): void {
  const rect = canvas.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0) return;
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.floor(rect.width * dpr);
  canvas.height = Math.floor(rect.height * dpr);
  const vrect = vizCanvas.getBoundingClientRect();
  if (vrect.width > 0 && vrect.height > 0) {
    vizCanvas.width = Math.floor(vrect.width * dpr);
    vizCanvas.height = Math.floor(vrect.height * dpr);
  }
}

function currentRenderData(audioRms = 0): RenderData {
  const mode = $<HTMLSelectElement>('view-mode').value as 'floorplan' | '3d';
  const overlay = $<HTMLSelectElement>('overlay').value as RenderData['overlay'];
  return {
    mode,
    overlay,
    fdtd: fdtdInfo ?? undefined,
    rays: rayCache,
    early: earlyCache,
    timeSec: (performance.now() - startTime) / 1000,
    audioRms,
  };
}

function render(audioRms = 0): void {
  renderViewport(ctx, canvas.width, canvas.height, store.state, currentRenderData(audioRms));
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
  return (Math.atan2(s.listener.y - s.source.y, s.listener.x - s.source.x) * 180) / Math.PI;
}

function selectedStem(): StemKind {
  return ($<HTMLSelectElement>('stem-kind')?.value as StemKind) || 'drum';
}

async function compute(playAfter = false): Promise<void> {
  const req = buildRequest();
  $('btn-run').textContent = 'Computing…';
  try {
    const { metrics, rir } = await physics.synthesize(req);
    audioEngine.setRir(rir);
    renderMetrics(metrics);
    renderFrequencyPlot(metrics);
    renderAdvisor(metrics);
    if ($<HTMLSelectElement>('overlay').value === 'fdtd') {
      await restartFdtd();
    } else {
      await loadOverlays();
    }
    if (playAfter) {
      await audioEngine.playStem(selectedStem(), loopEnabled);
    }
  } catch (err) {
    console.error(err);
    $('metrics').textContent = `Error: ${err instanceof Error ? err.message : String(err)}`;
  } finally {
    $('btn-run').textContent = '▶ Compute RIR';
  }
}

async function loadOverlays(): Promise<void> {
  const overlay = $<HTMLSelectElement>('overlay').value;
  try {
    await physics.buildRoom({ room: store.state.room, materials: store.state.materials });
    if (overlay === 'rays' || overlay === 'decay') {
      const res = await physics.traceRays({
        source: store.state.source,
        ray_count: 300,
        max_bounces: 16,
        max_time: 0.6,
        n_bins: 256,
        max_viz_rays: 220,
      });
      rayCache = JSON.parse(res).ray_paths as number[][][];
    }
    if (overlay !== 'none') {
      const er = await physics.earlyReflections({
        source: store.state.source,
        listener: store.state.listener,
        max_order: 2,
      });
      earlyCache = JSON.parse(er).reflections as EarlyReflection[];
    }
  } catch (e) {
    console.warn('[sonophile] overlay load error:', e);
  }
}

async function restartFdtd(): Promise<void> {
  fdtdRunning = false;
  try {
    await physics.buildRoom({ room: store.state.room, materials: store.state.materials });
    const summary = JSON.parse(await physics.fdtdNew({}));
    const mode = $<HTMLSelectElement>('fdtd-mode').value || 'sine';
    const freq = parseFloat($<HTMLInputElement>('fdtd-freq').value) || 260;
    await physics.fdtdExcite({
      mode,
      freq,
      x: store.state.source.x,
      y: store.state.source.y,
    });
    fdtdInfo = {
      grid: new Float32Array(summary.nx * summary.ny),
      nx: summary.nx,
      ny: summary.ny,
      dx: summary.dx,
      originX: summary.origin_x,
      originY: summary.origin_y,
    };
    fdtdRunning = true;
  } catch (e) {
    console.warn('[sonophile] FDTD start error:', e);
  }
}

function stopFdtd(): void {
  fdtdRunning = false;
  fdtdInfo = null;
  void physics.fdtdReset().catch(() => {});
}

function startRenderLoop(): void {
  const tick = () => {
    const viz = audioEngine.getVisualizerData();
    renderAudioVisualizer(vizCanvas, viz);
    const lvl = $('audio-level');
    if (lvl) {
      lvl.textContent = viz && viz.rms > 0.002 ? `${(20 * Math.log10(viz.rms)).toFixed(1)} dBFS` : 'idle';
    }
    render(viz?.rms ?? 0);

    if (fdtdRunning && !fdtdStepping && $<HTMLSelectElement>('overlay').value === 'fdtd') {
      fdtdStepping = true;
      physics
        .fdtdStep(3)
        .then((grid) => {
          if (fdtdInfo) fdtdInfo.grid = grid;
        })
        .catch(() => {})
        .finally(() => {
          fdtdStepping = false;
        });
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
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
  g.strokeStyle = '#243044';
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
  g.strokeStyle = '#38bdf8';
  g.lineWidth = 1.6;
  g.stroke();
}

function renderAdvisor(m: MetricsReport): void {
  const hints: { text: string; ok: boolean }[] = [];
  if (m.comb_filter_warning) {
    hints.push({ text: 'Flutter echo detected — add angled or diffusive surfaces.', ok: false });
  }
  const dips = m.sbir.filter((d) => d.freq < 400);
  if (dips.length) {
    hints.push({ text: `SBIR dip near ${dips[0].freq.toFixed(0)} Hz — treat nearest boundary.`, ok: false });
  }
  const axial = m.modes.filter((mo) => mo.kind === 'Axial' && mo.freq < 120);
  if (axial.length >= 2) {
    hints.push({ text: `Low-frequency modes at ${axial.map((mo) => mo.freq.toFixed(0)).join(', ')} Hz — add bass traps.`, ok: false });
  }
  if (isFinite(m.rt60_t30) && m.rt60_t30 > 1.5) {
    hints.push({ text: `RT60 ${m.rt60_t30.toFixed(2)} s is long — add broadband absorption.`, ok: false });
  }
  if (isFinite(m.rt60_t30) && m.rt60_t30 > 0.3 && m.rt60_t30 < 1.2) {
    hints.push({ text: 'Reverberation time is balanced for a control/mixing room.', ok: true });
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

function bindCanvasDrag(): void {
  let dragging: 'source' | 'listener' | null = null;
  const getPos = (e: PointerEvent) => {
    const rect = canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    return [(e.clientX - rect.left) * dpr, (e.clientY - rect.top) * dpr] as [number, number];
  };
  canvas.addEventListener('pointerdown', (e) => {
    if ($<HTMLSelectElement>('view-mode').value !== 'floorplan') return;
    const [px, py] = getPos(e);
    const b = fitBounds(store.state);
    const [sx, sy] = worldToScreen(store.state.source.x, store.state.source.y, b, canvas.width, canvas.height);
    const [lx, ly] = worldToScreen(store.state.listener.x, store.state.listener.y, b, canvas.width, canvas.height);
    if (Math.hypot(px - sx, py - sy) < 28) dragging = 'source';
    else if (Math.hypot(px - lx, py - ly) < 28) dragging = 'listener';
  });
  canvas.addEventListener('pointermove', (e) => {
    if (!dragging) return;
    const [px, py] = getPos(e);
    const b = fitBounds(store.state);
    const [wx, wy] = screenToWorld(px, py, b, canvas.width, canvas.height);
    const cx = Math.max(b.minX + 0.15, Math.min(b.maxX - 0.15, wx));
    const cy = Math.max(b.minY + 0.15, Math.min(b.maxY - 0.15, wy));
    store.commit((s) => {
      if (dragging === 'source') { s.source.x = +cx.toFixed(2); s.source.y = +cy.toFixed(2); }
      else { s.listener.x = +cx.toFixed(2); s.listener.y = +cy.toFixed(2); }
    });
  });
  window.addEventListener('pointerup', () => {
    if (dragging === 'source' && fdtdRunning) {
      const mode = $<HTMLSelectElement>('fdtd-mode').value || 'sine';
      const freq = parseFloat($<HTMLInputElement>('fdtd-freq').value) || 260;
      void physics.fdtdExcite({ mode, freq, x: store.state.source.x, y: store.state.source.y });
    }
    dragging = null;
  });
}

function bindUI(): void {
  store.subscribe((s) => {
    $<HTMLInputElement>('src-x').value = String(s.source.x);
    $<HTMLInputElement>('src-y').value = String(s.source.y);
    $<HTMLInputElement>('lst-x').value = String(s.listener.x);
    $<HTMLInputElement>('lst-y').value = String(s.listener.y);
    $<HTMLInputElement>('room-w').value = String(s.room.floorplan[1].x - s.room.floorplan[0].x);
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
      const d = s.room.floorplan[2].y - s.room.floorplan[0].y;
      s.room.floorplan = [
        { x: -v / 2, y: -d / 2 }, { x: v / 2, y: -d / 2 },
        { x: v / 2, y: d / 2 }, { x: -v / 2, y: d / 2 },
      ];
    });
  });
  range('room-d', (v) => {
    store.commit((s) => {
      const w = s.room.floorplan[1].x - s.room.floorplan[0].x;
      s.room.floorplan = [
        { x: -w / 2, y: -v / 2 }, { x: w / 2, y: -v / 2 },
        { x: w / 2, y: v / 2 }, { x: -w / 2, y: v / 2 },
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

  // Convolution params
  range('drywet', (v) => audioEngine.setParams({ dry: 1 - v, wet: v }));
  range('predelay', (v) => audioEngine.setParams({ predelayMs: v }));
  range('damping', (v) => audioEngine.setParams({ damping: v }));
  range('width', (v) => audioEngine.setParams({ width: v }));

  // Audio FX Rack
  range('fx-eq-low', (v) => audioEngine.setFx({ eqLowDb: v }));
  range('fx-eq-mid', (v) => audioEngine.setFx({ eqMidDb: v }));
  range('fx-eq-high', (v) => audioEngine.setFx({ eqHighDb: v }));
  range('fx-delay-mix', (v) => audioEngine.setFx({ delayMix: v }));
  range('fx-delay-time', (v) => audioEngine.setFx({ delayTimeMs: v }));
  range('fx-delay-fb', (v) => audioEngine.setFx({ delayFeedback: v }));
  range('fx-chorus', (v) => audioEngine.setFx({ chorusDepth: v }));
  range('fx-warmth', (v) => audioEngine.setFx({ warmth: v }));

  // Push initial FX rack values
  audioEngine.setParams({ dry: 0.45, wet: 0.55, width: 1.15 });
  audioEngine.setFx({ delayMix: 0.15, delayTimeMs: 240, delayFeedback: 0.32, chorusDepth: 0.2, warmth: 0.15 });

  // FDTD live controls
  $('fdtd-mode').addEventListener('change', () => void restartFdtd());
  range('fdtd-freq', (v) => {
    $('fdtd-freq-val').textContent = `${Math.round(v)} Hz`;
    if (fdtdRunning) {
      void physics.fdtdExcite({
        mode: $<HTMLSelectElement>('fdtd-mode').value || 'sine',
        freq: v,
        x: store.state.source.x,
        y: store.state.source.y,
      });
    }
  });
  $('btn-pulse').addEventListener('click', async () => {
    $<HTMLSelectElement>('overlay').value = 'fdtd';
    await restartFdtd();
    await physics.fdtdExcite({ mode: 'impulse', freq: 260, x: store.state.source.x, y: store.state.source.y });
  });

  // Toolbar actions
  $('btn-run').addEventListener('click', () => void compute());
  $('btn-play').addEventListener('click', async () => {
    await audioEngine.ensure();
    await audioEngine.playStem(selectedStem(), loopEnabled);
  });
  $('btn-loop').addEventListener('click', () => {
    loopEnabled = !loopEnabled;
    $('btn-loop').classList.toggle('active', loopEnabled);
  });
  $('btn-stop').addEventListener('click', () => audioEngine.stopStem());
  $('btn-mic').addEventListener('click', async () => {
    const on = await audioEngine.toggleMic();
    $('btn-mic').classList.toggle('active', on);
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
    store.loadSnapshot('A');
    await compute();
    store.loadSnapshot('B');
    await compute();
  });
  $('btn-undo').addEventListener('click', () => store.undo());
  $('btn-redo').addEventListener('click', () => store.redo());

  $('overlay').addEventListener('change', async () => {
    stopFdtd();
    if ($<HTMLSelectElement>('overlay').value === 'fdtd') await restartFdtd();
    else await loadOverlays();
  });

  bindCanvasDrag();
  window.addEventListener('resize', () => resizeCanvas());
}

export async function initApp(): Promise<void> {
  resizeCanvas();
  populateMaterials();
  bindUI();
  startRenderLoop();
  // Auto-compute initial RIR & launch FDTD wave field immediately on startup.
  await compute(false);
}
