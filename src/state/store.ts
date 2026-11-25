// Application state: undo/redo history, A/B snapshots, and JSON / URL-encoded
// persistence for room blueprints and the full project.

import type { Material, RoomSpec, Vec3 } from '../physics/client';

export interface ConvState {
  dry: number;
  wet: number;
  predelayMs: number;
  damping: number;
  width: number;
}

export interface ProjectState {
  room: RoomSpec;
  materials: Material[];
  source: Vec3;
  listener: Vec3;
  directivity: string;
  conv: ConvState;
}

export const defaultMaterials = (): Material[] => {
  const presets: [string, number[], number[], number[]][] = [
    ['concrete', [0.01, 0.01, 0.02, 0.02, 0.02, 0.03, 0.03], [0.05, 0.05, 0.05, 0.05, 0.05, 0.05, 0.05], [0, 0, 0, 0, 0, 0, 0]],
    ['brick', [0.03, 0.03, 0.03, 0.04, 0.05, 0.07, 0.07], [0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1], [0, 0, 0, 0, 0, 0, 0]],
    ['drywall', [0.08, 0.09, 0.1, 0.08, 0.06, 0.05, 0.04], [0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1], [0, 0, 0, 0, 0, 0, 0]],
    ['glass', [0.18, 0.06, 0.04, 0.03, 0.02, 0.02, 0.02], [0.05, 0.05, 0.05, 0.05, 0.05, 0.05, 0.05], [0, 0, 0, 0, 0, 0, 0]],
    ['hardwood', [0.15, 0.11, 0.1, 0.07, 0.06, 0.06, 0.05], [0.2, 0.2, 0.2, 0.2, 0.2, 0.2, 0.2], [0, 0, 0, 0, 0, 0, 0]],
    ['carpet', [0.08, 0.24, 0.57, 0.69, 0.71, 0.73, 0.73], [0.6, 0.6, 0.6, 0.6, 0.6, 0.6, 0.6], [0, 0, 0, 0, 0, 0, 0]],
    ['mineral-wool', [0.16, 0.43, 0.74, 0.96, 1, 1, 1], [0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5], [0, 0, 0, 0, 0, 0, 0]],
    ['diffuser', [0.1, 0.1, 0.12, 0.14, 0.16, 0.18, 0.2], [0.9, 0.9, 0.9, 0.9, 0.9, 0.9, 0.9], [0, 0, 0, 0, 0, 0, 0]],
    ['bass-trap', [0.9, 0.95, 0.98, 0.99, 1, 1, 1], [0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5], [0, 0, 0, 0, 0, 0, 0]],
    ['door', [0.1, 0.12, 0.14, 0.16, 0.18, 0.2, 0.2], [0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1], [0.35, 0.35, 0.35, 0.35, 0.35, 0.35, 0.35]],
    ['open-aperture', [0.05, 0.05, 0.05, 0.05, 0.05, 0.05, 0.05], [0, 0, 0, 0, 0, 0, 0], [1, 1, 1, 1, 1, 1, 1]],
  ];
  return presets.map(([name, a, s, t]) => ({ name, absorption: a, scattering: s, transmission: t }));
};

export function defaultState(): ProjectState {
  const w = 5, d = 4;
  return {
    room: {
      floorplan: [
        { x: -w / 2, y: -d / 2 },
        { x: w / 2, y: -d / 2 },
        { x: w / 2, y: d / 2 },
        { x: -w / 2, y: d / 2 },
      ],
      height: 2.7,
      wall_material: 2,
      floor_material: 4,
      ceiling_material: 2,
      apertures: [],
      furniture: [],
    },
    materials: defaultMaterials(),
    source: { x: 0, y: 0, z: 1.2 },
    listener: { x: 2, y: 1, z: 1.2 },
    directivity: 'omnidirectional',
    conv: { dry: 0.5, wet: 0.5, predelayMs: 0, damping: 0.5, width: 1 },
  };
}

type Listener = (s: ProjectState) => void;

export class Store {
  state: ProjectState = defaultState();
  private undoStack: ProjectState[] = [];
  private redoStack: ProjectState[] = [];
  private snapshotA: ProjectState | null = null;
  private snapshotB: ProjectState | null = null;
  private listeners: Listener[] = [];

  subscribe(fn: Listener): void {
    this.listeners.push(fn);
    fn(this.state);
  }

  private emit(): void {
    const s = structuredClone(this.state);
    for (const fn of this.listeners) fn(s);
  }

  commit(mutator: (s: ProjectState) => void): void {
    this.undoStack.push(structuredClone(this.state));
    if (this.undoStack.length > 100) this.undoStack.shift();
    this.redoStack = [];
    mutator(this.state);
    this.emit();
  }

  setState(next: ProjectState): void {
    this.state = next;
    this.emit();
  }

  undo(): void {
    const prev = this.undoStack.pop();
    if (!prev) return;
    this.redoStack.push(structuredClone(this.state));
    this.state = prev;
    this.emit();
  }

  redo(): void {
    const next = this.redoStack.pop();
    if (!next) return;
    this.undoStack.push(structuredClone(this.state));
    this.state = next;
    this.emit();
  }

  captureSnapshot(slot: 'A' | 'B'): void {
    if (slot === 'A') this.snapshotA = structuredClone(this.state);
    else this.snapshotB = structuredClone(this.state);
  }

  loadSnapshot(slot: 'A' | 'B'): void {
    const s = slot === 'A' ? this.snapshotA : this.snapshotB;
    if (s) this.setState(structuredClone(s));
  }

  toJSON(): string {
    return JSON.stringify(this.state, null, 2);
  }

  fromJSON(text: string): boolean {
    try {
      const parsed = JSON.parse(text) as ProjectState;
      if (!parsed.room || !Array.isArray(parsed.room.floorplan)) return false;
      this.setState(parsed);
      return true;
    } catch {
      return false;
    }
  }

  toURL(): string {
    const json = JSON.stringify(this.state);
    const b64 = btoa(unescape(encodeURIComponent(json)));
    const url = new URL(location.href);
    url.searchParams.set('state', b64);
    return url.toString();
  }

  fromURL(): boolean {
    const params = new URLSearchParams(location.search);
    const b64 = params.get('state');
    if (!b64) return false;
    try {
      const json = decodeURIComponent(escape(atob(b64)));
      return this.fromJSON(json);
    } catch {
      return false;
    }
  }
}

export const store = new Store();
