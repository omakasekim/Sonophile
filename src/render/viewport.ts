// Canvas viewport: 2-D floorplan, extruded 3-D view, and the visual overlays
// (FDTD wave field, ray paths / early reflections, energy decay colouring).

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
}

function fitBounds(state: ProjectState): { minX: number; minY: number; maxX: number; maxY: number } {
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

function worldToScreen(
  x: number, y: number,
  b: { minX: number; minY: number; maxX: number; maxY: number },
  w: number, h: number
): [number, number] {
  const pad = 40;
  const scale = Math.min((w - 2 * pad) / (b.maxX - b.minX), (h - 2 * pad) / (b.maxY - b.minY));
  const sx = (x - (b.minX + b.maxX) / 2) * scale + w / 2;
  const sy = h / 2 - (y - (b.minY + b.maxY) / 2) * scale;
  return [sx, sy];
}

function drawFloorplan(
  ctx: CanvasRenderingContext2D, w: number, h: number, s: ProjectState, data: RenderData
): void {
  const b = fitBounds(s);
  ctx.clearRect(0, 0, w, h);

  // Room floor first so wave/ray overlays render visibly on top of it.
  ctx.beginPath();
  s.room.floorplan.forEach((p, i) => {
    const [sx, sy] = worldToScreen(p.x, p.y, b, w, h);
    if (i === 0) ctx.moveTo(sx, sy); else ctx.lineTo(sx, sy);
  });
  ctx.closePath();
  ctx.fillStyle = '#1a2028';
  ctx.fill();

  // FDTD heatmap & decay overlays on top of floor.
  if (data.overlay === 'fdtd' && data.fdtd) {
    drawFdtd(ctx, w, h, b, data.fdtd);
  }
  if (data.overlay === 'decay' && data.rays) {
    drawRays(ctx, w, h, b, data.rays, true);
  }

  // Room boundary stroke on top of overlays.
  ctx.beginPath();
  s.room.floorplan.forEach((p, i) => {
    const [sx, sy] = worldToScreen(p.x, p.y, b, w, h);
    if (i === 0) ctx.moveTo(sx, sy); else ctx.lineTo(sx, sy);
  });
  ctx.closePath();
  ctx.strokeStyle = '#4cc2ff';
  ctx.lineWidth = 2;
  ctx.stroke();

  // Ray overlay.
  if (data.overlay === 'rays' && data.rays) {
    drawRays(ctx, w, h, b, data.rays, false);
  }
  // Early reflections.
  if (data.early) {
    drawEarly(ctx, w, h, b, s, data.early);
  }

  // Source & listener markers.
  const [sx, sy] = worldToScreen(s.source.x, s.source.y, b, w, h);
  const [lx, ly] = worldToScreen(s.listener.x, s.listener.y, b, w, h);
  ctx.fillStyle = '#ff5c5c';
  ctx.beginPath(); ctx.arc(sx, sy, 7, 0, Math.PI * 2); ctx.fill();
  ctx.fillStyle = '#7ee787';
  ctx.beginPath(); ctx.arc(lx, ly, 7, 0, Math.PI * 2); ctx.fill();
  ctx.fillStyle = '#fff';
  ctx.font = '11px sans-serif';
  ctx.fillText('S', sx + 9, sy + 4);
  ctx.fillText('L', lx + 9, ly + 4);
}

function drawFdtd(
  ctx: CanvasRenderingContext2D, w: number, h: number,
  b: { minX: number; minY: number; maxX: number; maxY: number },
  fdtd: NonNullable<RenderData['fdtd']>
): void {
  const { grid, nx, ny, dx, originX, originY } = fdtd;
  let maxAbs = 1e-9;
  for (let i = 0; i < grid.length; i++) maxAbs = Math.max(maxAbs, Math.abs(grid[i]));
  const cell = Math.min(w, h) / Math.max(nx, ny);
  for (let j = 0; j < ny; j++) {
    for (let i = 0; i < nx; i++) {
      const v = grid[j * nx + i] / maxAbs; // -1..1
      const [sx, sy] = worldToScreen(originX + (i + 0.5) * dx, originY + (j + 0.5) * dx, b, w, h);
      ctx.fillStyle = pressureColor(v);
      ctx.fillRect(sx - cell / 2, sy - cell / 2, cell, cell);
    }
  }
}

function pressureColor(v: number): string {
  const t = Math.max(-1, Math.min(1, v));
  // Blue (negative) → black (0) → red/yellow (positive).
  if (t < 0) {
    const a = -t;
    return `rgb(${Math.round(a * 60)}, ${Math.round(a * 100)}, ${Math.round(120 + a * 135)})`;
  }
  return `rgb(${Math.round(80 + t * 175)}, ${Math.round(20 + t * 30)}, ${Math.round(20)})`;
}

function drawRays(
  ctx: CanvasRenderingContext2D, w: number, h: number,
  b: { minX: number; minY: number; maxX: number; maxY: number },
  rays: number[][][], colorByTime: boolean
): void {
  ctx.lineWidth = 1;
  for (const path of rays) {
    ctx.beginPath();
    path.forEach((p, i) => {
      const [sx, sy] = worldToScreen(p[0], p[1], b, w, h);
      if (i === 0) ctx.moveTo(sx, sy); else ctx.lineTo(sx, sy);
    });
    ctx.strokeStyle = colorByTime
      ? `hsla(${Math.max(0, 210 - (path.length * 18))}, 90%, 60%, 0.55)`
      : 'rgba(120, 180, 255, 0.35)';
    ctx.stroke();
  }
}

function drawEarly(
  ctx: CanvasRenderingContext2D, w: number, h: number,
  b: { minX: number; minY: number; maxX: number; maxY: number },
  s: ProjectState, early: EarlyReflection[]
): void {
  const orderColors = ['#22c55e', '#eab308', '#f97316', '#ef4444'];
  ctx.lineWidth = 1.5;
  for (const r of early) {
    const color = orderColors[Math.min(r.order - 1, 3)];
    ctx.strokeStyle = color;
    ctx.globalAlpha = 0.9;
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
    ctx.globalAlpha = 1;
  }
}

function project3d(x: number, y: number, z: number, w: number, h: number): [number, number] {
  const c = Math.cos(Math.PI / 6), s = Math.sin(Math.PI / 6);
  const scale = Math.min(w, h) / 14;
  const px = w / 2 + (x - y) * c * scale;
  const py = h * 0.62 + (x + y) * s * scale - z * scale * 1.4;
  return [px, py];
}

function draw3D(
  ctx: CanvasRenderingContext2D, w: number, h: number, s: ProjectState, _data: RenderData
): void {
  ctx.clearRect(0, 0, w, h);
  ctx.fillStyle = '#0a0d11';
  ctx.fillRect(0, 0, w, h);
  const pts = s.room.floorplan;
  const z0 = 0;
  const z1 = s.room.height;

  // Build wall quads.
  interface Quad { corners: [number, number, number][]; depth: number }
  const quads: Quad[] = [];
  for (let i = 0; i < pts.length; i++) {
    const a = pts[i], b = pts[(i + 1) % pts.length];
    const corners: [number, number, number][] = [
      [a.x, a.y, z0], [b.x, b.y, z0], [b.x, b.y, z1], [a.x, a.y, z1],
    ];
    const depth = corners.reduce((acc, c) => acc + c[0] + c[1], 0) / 4;
    quads.push({ corners, depth });
  }
  // Floor + ceiling.
  const floor = pts.map((p) => [p.x, p.y, z0] as [number, number, number]);
  const ceil = pts.map((p) => [p.x, p.y, z1] as [number, number, number]);

  // Painter's algorithm: sort walls far→near.
  quads.sort((a, b) => a.depth - b.depth);

  const poly = (corners: [number, number, number][], fill: string, stroke: string) => {
    ctx.beginPath();
    corners.forEach((c, i) => {
      const [px, py] = project3d(c[0], c[1], c[2], w, h);
      if (i === 0) ctx.moveTo(px, py); else ctx.lineTo(px, py);
    });
    ctx.closePath();
    ctx.fillStyle = fill;
    ctx.fill();
    ctx.strokeStyle = stroke;
    ctx.lineWidth = 1;
    ctx.stroke();
  };

  poly(floor, '#161d26', '#2a313c');
  for (const q of quads) poly(q.corners, '#1d2733', '#3b4656');
  poly(ceil, '#10151c', '#2a313c');

  // Source / listener markers projected at their z.
  const [sx, sy] = project3d(s.source.x, s.source.y, s.source.z, w, h);
  const [lx, ly] = project3d(s.listener.x, s.listener.y, s.listener.z, w, h);
  ctx.fillStyle = '#ff5c5c';
  ctx.beginPath(); ctx.arc(sx, sy, 5, 0, Math.PI * 2); ctx.fill();
  ctx.fillStyle = '#7ee787';
  ctx.beginPath(); ctx.arc(lx, ly, 5, 0, Math.PI * 2); ctx.fill();
}

export function renderViewport(
  ctx: CanvasRenderingContext2D, w: number, h: number, s: ProjectState, data: RenderData
): void {
  if (data.mode === '3d') draw3D(ctx, w, h, s, data);
  else drawFloorplan(ctx, w, h, s, data);
}
