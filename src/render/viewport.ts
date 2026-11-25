// Canvas viewport: 2-D floorplan, extruded 3-D view, smooth bilinear FDTD wave
// field with plasma-cyan colormap, animated wavefront rings, ray/reflection
// overlays, and live audio spectrum + oscilloscope visualization.

import type { ProjectState } from '../state/store';

export interface EarlyReflection {
  time: number;
  distance: number;
  order: number;
  direction: number[];
  amplitude: number[];
  surface_path: number[];
  points: number[][];
}

export interface RenderData {
  mode: 'floorplan' | '3d';
  overlay: 'none' | 'fdtd' | 'rays' | 'decay';
  fdtd?: { grid: Float32Array; nx: number; ny: number; dx: number; originX: number; originY: number };
  rays?: number[][][];
  early?: EarlyReflection[];
  timeSec?: number;
  audioRms?: number;
}

export function fitBounds(state: ProjectState): { minX: number; minY: number; maxX: number; maxY: number } {
  const pts = state.room.floorplan;
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (const p of pts) {
    minX = Math.min(minX, p.x);
    minY = Math.min(minY, p.y);
    maxX = Math.max(maxX, p.x);
    maxY = Math.max(maxY, p.y);
  }
  if (!isFinite(minX)) { minX = -2.5; minY = -2; maxX = 2.5; maxY = 2; }
  return { minX, minY, maxX, maxY };
}

export function worldToScreen(
  x: number, y: number,
  b: { minX: number; minY: number; maxX: number; maxY: number },
  w: number, h: number
): [number, number] {
  const pad = 48;
  const scale = Math.min((w - 2 * pad) / Math.max(0.1, b.maxX - b.minX), (h - 2 * pad) / Math.max(0.1, b.maxY - b.minY));
  const sx = (x - (b.minX + b.maxX) / 2) * scale + w / 2;
  const sy = h / 2 - (y - (b.minY + b.maxY) / 2) * scale;
  return [sx, sy];
}

export function screenToWorld(
  sx: number, sy: number,
  b: { minX: number; minY: number; maxX: number; maxY: number },
  w: number, h: number
): [number, number] {
  const pad = 48;
  const scale = Math.min((w - 2 * pad) / Math.max(0.1, b.maxX - b.minX), (h - 2 * pad) / Math.max(0.1, b.maxY - b.minY));
  const x = (sx - w / 2) / scale + (b.minX + b.maxX) / 2;
  const y = (h / 2 - sy) / scale + (b.minY + b.maxY) / 2;
  return [x, y];
}

let offCanvas: HTMLCanvasElement | null = null;
let offCtx: CanvasRenderingContext2D | null = null;

function ensureOffscreen(nx: number, ny: number): CanvasRenderingContext2D {
  if (!offCanvas) {
    offCanvas = document.createElement('canvas');
    offCtx = offCanvas.getContext('2d')!;
  }
  if (offCanvas.width !== nx || offCanvas.height !== ny) {
    offCanvas.width = nx;
    offCanvas.height = ny;
  }
  return offCtx!;
}

function drawGridBackground(ctx: CanvasRenderingContext2D, w: number, h: number): void {
  ctx.clearRect(0, 0, w, h);
  ctx.strokeStyle = 'rgba(56, 189, 248, 0.05)';
  ctx.lineWidth = 1;
  const step = 36;
  ctx.beginPath();
  for (let x = 0; x < w; x += step) {
    ctx.moveTo(x, 0);
    ctx.lineTo(x, h);
  }
  for (let y = 0; y < h; y += step) {
    ctx.moveTo(0, y);
    ctx.lineTo(w, y);
  }
  ctx.stroke();
}

function drawFloorplan(
  ctx: CanvasRenderingContext2D, w: number, h: number, s: ProjectState, data: RenderData
): void {
  const b = fitBounds(s);
  drawGridBackground(ctx, w, h);

  // Room polygon path helper.
  const traceRoom = () => {
    ctx.beginPath();
    s.room.floorplan.forEach((p, i) => {
      const [sx, sy] = worldToScreen(p.x, p.y, b, w, h);
      if (i === 0) ctx.moveTo(sx, sy); else ctx.lineTo(sx, sy);
    });
    ctx.closePath();
  };

  // 1. Room floor fill.
  traceRoom();
  ctx.fillStyle = '#0d1422';
  ctx.fill();

  // 2. Clipped acoustic overlays inside the room.
  ctx.save();
  traceRoom();
  ctx.clip();

  if (data.overlay === 'fdtd' && data.fdtd && data.fdtd.grid.length > 0) {
    drawFdtdSmooth(ctx, w, h, b, data.fdtd);
  }
  if (data.overlay === 'decay' && data.rays) {
    drawRays(ctx, w, h, b, data.rays, true);
  }
  if (data.overlay === 'rays' && data.rays) {
    drawRays(ctx, w, h, b, data.rays, false);
  }
  if (data.early && data.early.length) {
    drawEarly(ctx, w, h, b, s, data.early);
  }

  // Animated wavefront pulse rings radiating from Source (enhanced by live audio RMS).
  const [sx, sy] = worldToScreen(s.source.x, s.source.y, b, w, h);
  const t = data.timeSec ?? 0;
  const rms = data.audioRms ?? 0;
  for (let k = 0; k < 3; k++) {
    const phase = (t * 0.65 + k / 3) % 1;
    const rad = 12 + phase * Math.min(w, h) * (0.28 + rms * 0.35);
    const alpha = (1 - phase) * (0.22 + rms * 0.55);
    ctx.strokeStyle = `rgba(56, 189, 248, ${alpha.toFixed(3)})`;
    ctx.lineWidth = 1.5 + rms * 2.5;
    ctx.beginPath();
    ctx.arc(sx, sy, rad, 0, Math.PI * 2);
    ctx.stroke();
  }
  ctx.restore();

  // 3. Glowing room boundary stroke + dimension labels.
  traceRoom();
  ctx.save();
  ctx.shadowColor = '#38bdf8';
  ctx.shadowBlur = 12;
  ctx.strokeStyle = '#38bdf8';
  ctx.lineWidth = 2.2;
  ctx.stroke();
  ctx.restore();

  // 4. Direct path S -> L line.
  const [lx, ly] = worldToScreen(s.listener.x, s.listener.y, b, w, h);
  ctx.save();
  ctx.setLineDash([5, 5]);
  ctx.strokeStyle = 'rgba(226, 232, 240, 0.35)';
  ctx.lineWidth = 1.2;
  ctx.beginPath();
  ctx.moveTo(sx, sy);
  ctx.lineTo(lx, ly);
  ctx.stroke();
  ctx.restore();

  // 5. Source & Listener glowing markers.
  drawMarker(ctx, sx, sy, '#fb7185', '#f43f5e', 'S', 9 + rms * 6);
  drawMarker(ctx, lx, ly, '#4ade80', '#22c55e', 'L', 9);

  // Room dimensions HUD.
  const rw = (b.maxX - b.minX).toFixed(1);
  const rd = (b.maxY - b.minY).toFixed(1);
  ctx.fillStyle = '#94a3b8';
  ctx.font = '12px ui-monospace, SFMono-Regular, monospace';
  ctx.fillText(`Room: ${rw}m × ${rd}m × ${s.room.height.toFixed(1)}m`, 14, h - 14);
}

function drawMarker(
  ctx: CanvasRenderingContext2D, x: number, y: number, fill: string, glow: string, label: string, radius: number
): void {
  ctx.save();
  ctx.shadowColor = glow;
  ctx.shadowBlur = 14;
  ctx.fillStyle = fill;
  ctx.beginPath();
  ctx.arc(x, y, radius, 0, Math.PI * 2);
  ctx.fill();
  ctx.strokeStyle = '#ffffff';
  ctx.lineWidth = 1.8;
  ctx.stroke();
  ctx.restore();
  ctx.fillStyle = '#070b12';
  ctx.font = 'bold 11px system-ui, sans-serif';
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText(label, x, y + 0.5);
  ctx.textAlign = 'start';
  ctx.textBaseline = 'alphabetic';
}

function drawFdtdSmooth(
  ctx: CanvasRenderingContext2D, w: number, h: number,
  b: { minX: number; minY: number; maxX: number; maxY: number },
  fdtd: NonNullable<RenderData['fdtd']>
): void {
  const { grid, nx, ny, dx, originX, originY } = fdtd;
  if (nx <= 0 || ny <= 0 || grid.length < nx * ny) return;

  let maxAbs = 1e-6;
  for (let i = 0; i < nx * ny; i++) {
    const a = Math.abs(grid[i]);
    if (a > maxAbs) maxAbs = a;
  }

  const octx = ensureOffscreen(nx, ny);
  const img = octx.createImageData(nx, ny);
  const data = img.data;

  // Flip Y so world +Y maps upward on canvas.
  for (let j = 0; j < ny; j++) {
    const dstRow = ny - 1 - j;
    for (let i = 0; i < nx; i++) {
      const raw = grid[j * nx + i] / maxAbs;
      // Soft nonlinear boost so subtle reflections across the room stay vivid.
      const v = Math.sign(raw) * Math.pow(Math.min(1, Math.abs(raw)), 0.62);
      const p = (dstRow * nx + i) * 4;
      if (v >= 0) {
        // Positive pressure: deep indigo -> glowing cyan -> bright white-gold crest.
        data[p] = Math.round(18 + v * 235);
        data[p + 1] = Math.round(28 + v * 195);
        data[p + 2] = Math.round(52 + v * 203);
        data[p + 3] = Math.round(60 + v * 195);
      } else {
        const a = -v;
        // Negative pressure: deep violet -> vibrant magenta-coral trough.
        data[p] = Math.round(22 + a * 220);
        data[p + 1] = Math.round(16 + a * 54);
        data[p + 2] = Math.round(48 + a * 165);
        data[p + 3] = Math.round(60 + a * 195);
      }
    }
  }
  octx.putImageData(img, 0, 0);

  const [x0, yBottom] = worldToScreen(originX, originY, b, w, h);
  const [x1, yTop] = worldToScreen(originX + nx * dx, originY + ny * dx, b, w, h);

  ctx.save();
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(offCanvas!, x0, yTop, x1 - x0, yBottom - yTop);
  ctx.restore();
}

function drawRays(
  ctx: CanvasRenderingContext2D, w: number, h: number,
  b: { minX: number; minY: number; maxX: number; maxY: number },
  rays: number[][][], colorByTime: boolean
): void {
  ctx.save();
  ctx.globalCompositeOperation = 'lighter';
  ctx.lineWidth = 1.1;
  for (const path of rays) {
    ctx.beginPath();
    path.forEach((p, i) => {
      const [sx, sy] = worldToScreen(p[0], p[1], b, w, h);
      if (i === 0) ctx.moveTo(sx, sy); else ctx.lineTo(sx, sy);
    });
    ctx.strokeStyle = colorByTime
      ? `hsla(${Math.max(0, 200 - path.length * 16)}, 95%, 60%, 0.42)`
      : 'rgba(56, 189, 248, 0.28)';
    ctx.stroke();
  }
  ctx.restore();
}

function drawEarly(
  ctx: CanvasRenderingContext2D, w: number, h: number,
  b: { minX: number; minY: number; maxX: number; maxY: number },
  s: ProjectState, early: EarlyReflection[]
): void {
  const orderColors = ['#4ade80', '#facc15', '#fb923c', '#f87171'];
  ctx.save();
  ctx.lineWidth = 1.7;
  for (const r of early) {
    ctx.strokeStyle = orderColors[Math.min(Math.max(0, r.order - 1), 3)];
    ctx.globalAlpha = 0.85;
    ctx.beginPath();
    const [sx, sy] = worldToScreen(s.source.x, s.source.y, b, w, h);
    ctx.moveTo(sx, sy);
    for (const p of r.points) {
      const [px, py] = worldToScreen(p[0], p[1], b, w, h);
      ctx.lineTo(px, py);
    }
    const [lx, ly] = worldToScreen(s.listener.x, s.listener.y, b, w, h);
    ctx.lineTo(lx, ly);
    ctx.stroke();
  }
  ctx.restore();
}

function project3d(x: number, y: number, z: number, w: number, h: number, angle = 0.55): [number, number] {
  const rx = x * Math.cos(angle) - y * Math.sin(angle);
  const ry = x * Math.sin(angle) + y * Math.cos(angle);
  const scale = Math.min(w, h) / 11.5;
  const px = w / 2 + rx * scale;
  const py = h * 0.64 + ry * 0.52 * scale - z * scale * 1.15;
  return [px, py];
}

function draw3D(
  ctx: CanvasRenderingContext2D, w: number, h: number, s: ProjectState, data: RenderData
): void {
  drawGridBackground(ctx, w, h);
  const pts = s.room.floorplan;
  const z0 = 0;
  const z1 = s.room.height;
  const angle = 0.55 + Math.sin((data.timeSec ?? 0) * 0.25) * 0.12;

  const poly = (corners: [number, number, number][], fill: string, stroke: string) => {
    ctx.beginPath();
    corners.forEach((c, i) => {
      const [px, py] = project3d(c[0], c[1], c[2], w, h, angle);
      if (i === 0) ctx.moveTo(px, py); else ctx.lineTo(px, py);
    });
    ctx.closePath();
    ctx.fillStyle = fill;
    ctx.fill();
    ctx.strokeStyle = stroke;
    ctx.lineWidth = 1.2;
    ctx.stroke();
  };

  const floor = pts.map((p) => [p.x, p.y, z0] as [number, number, number]);
  const ceil = pts.map((p) => [p.x, p.y, z1] as [number, number, number]);
  poly(floor, 'rgba(20, 30, 48, 0.9)', '#38bdf8');

  // Render live FDTD wave ripples projected onto the 3D floor!
  if (data.overlay === 'fdtd' && data.fdtd && data.fdtd.grid.length > 0) {
    const { grid, nx, ny, dx, originX, originY } = data.fdtd;
    let maxAbs = 1e-6;
    for (let i = 0; i < grid.length; i++) maxAbs = Math.max(maxAbs, Math.abs(grid[i]));
    const step = 2;
    for (let j = 0; j < ny; j += step) {
      for (let i = 0; i < nx; i += step) {
        const v = grid[j * nx + i] / maxAbs;
        if (Math.abs(v) < 0.08) continue;
        const wx = originX + (i + 0.5) * dx;
        const wy = originY + (j + 0.5) * dx;
        const [px, py] = project3d(wx, wy, 0.05 + v * 0.35, w, h, angle);
        ctx.fillStyle = v > 0
          ? `rgba(56, 189, 248, ${Math.min(0.9, Math.abs(v))})`
          : `rgba(217, 70, 239, ${Math.min(0.9, Math.abs(v))})`;
        ctx.fillRect(px - 2.5, py - 2.5, 5, 5);
      }
    }
  }

  // Translucent extruded walls.
  for (let i = 0; i < pts.length; i++) {
    const a = pts[i], b = pts[(i + 1) % pts.length];
    poly([[a.x, a.y, z0], [b.x, b.y, z0], [b.x, b.y, z1], [a.x, a.y, z1]], 'rgba(30, 45, 70, 0.25)', 'rgba(56, 189, 248, 0.55)');
  }
  poly(ceil, 'rgba(15, 23, 42, 0.12)', 'rgba(148, 163, 184, 0.35)');

  const [sx, sy] = project3d(s.source.x, s.source.y, s.source.z, w, h, angle);
  const [lx, ly] = project3d(s.listener.x, s.listener.y, s.listener.z, w, h, angle);
  drawMarker(ctx, sx, sy, '#fb7185', '#f43f5e', 'S', 8 + (data.audioRms ?? 0) * 6);
  drawMarker(ctx, lx, ly, '#4ade80', '#22c55e', 'L', 8);
}

export function renderAudioVisualizer(
  canvas: HTMLCanvasElement,
  viz: { freq: Uint8Array; time: Uint8Array; rms: number } | null
): void {
  const g = canvas.getContext('2d');
  if (!g) return;
  const W = canvas.width;
  const H = canvas.height;
  g.clearRect(0, 0, W, H);
  g.fillStyle = '#060910';
  g.fillRect(0, 0, W, H);

  if (!viz) {
    g.strokeStyle = 'rgba(56, 189, 248, 0.2)';
    g.beginPath();
    g.moveTo(0, H / 2);
    g.lineTo(W, H / 2);
    g.stroke();
    return;
  }

  // Frequency spectrum bars (gradient cyan -> violet)
  const bars = Math.min(64, viz.freq.length);
  const bw = W / bars;
  const grad = g.createLinearGradient(0, H, 0, 0);
  grad.addColorStop(0, '#0284c7');
  grad.addColorStop(0.55, '#38bdf8');
  grad.addColorStop(1, '#c084fc');
  g.fillStyle = grad;
  for (let i = 0; i < bars; i++) {
    const val = viz.freq[i] / 255;
    const bh = val * (H - 8);
    g.fillRect(i * bw + 1, H - bh, Math.max(1, bw - 2), bh);
  }

  // Live oscilloscope waveform overlay
  g.strokeStyle = 'rgba(248, 250, 252, 0.85)';
  g.lineWidth = 1.6;
  g.beginPath();
  for (let i = 0; i < viz.time.length; i++) {
    const x = (i / (viz.time.length - 1)) * W;
    const y = (viz.time[i] / 255) * H;
    if (i === 0) g.moveTo(x, y); else g.lineTo(x, y);
  }
  g.stroke();
}

export function renderViewport(
  ctx: CanvasRenderingContext2D, w: number, h: number, s: ProjectState, data: RenderData
): void {
  if (data.mode === '3d') draw3D(ctx, w, h, s, data);
  else drawFloorplan(ctx, w, h, s, data);
}
