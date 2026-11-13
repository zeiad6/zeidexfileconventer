/** Shapes the Rust side sends to the UI. Keep in sync with `app/src-tauri/src/lib.rs`. */

export type Mode = 'convert' | 'tools' | 'actions';

export interface Sub {
  key: string;
  value: string;
  label: string;
  label_ar: string;
  custom: boolean;
}

export interface Param {
  key: string;
  label: string;
  label_ar: string;
  default: string;
}

export interface Choice {
  kind: string;
  id: string;
  label: string;
  label_ar: string;
  params: Param[];
  editor: boolean;
  children: Sub[];
  preset?: Record<string, string>;
}

export interface Desc {
  category: string;
  mixed: boolean;
  convert: Choice[];
  tools: Choice[];
  actions: Choice[];
}

export interface Engine {
  name: string;
  path: string | null;
  hint: string;
}

export interface Recipe {
  name: string;
  steps: string;
}

export interface WatchRule {
  dir: string;
  exts: string;
  steps: string;
  enabled: boolean;
}

export interface Settings {
  convert: string;
  tools: string;
  actions: string;
  reveal: boolean;
  hotkey: boolean;
  recipes: Recipe[];
  watch: WatchRule[];
}

/** One file of one job, as emitted by the runner. */
export type JobEvent =
  | { type: 'start'; job: number; index: number; total: number; file: string; title: string }
  | { type: 'progress'; job: number; index: number; fraction: number }
  | { type: 'done'; job: number; index: number; output: string }
  | { type: 'failed'; job: number; index: number; error: string }
  | { type: 'text'; job: number; title: string; text: string };

/** One page of a PDF, or one clip of a join, as the editor needs it. */
export interface PageRef {
  t: number;
  f: number;
  n: number;
  rot: number;
}

export interface Clip {
  path: string;
  name: string;
  duration: number;
  thumb: string;
}

export interface EditorState {
  paths: string[];
  tool: string;
  video: boolean;
  width: number;
  height: number;
  duration: number;
  preview?: string;
  pages: string[];
  page_refs: [number, number][];
  fps?: number;
  waveform?: string;
  clips?: Clip[];
}

export interface Point {
  x: number;
  y: number;
}

export type DragPayload =
  | { type: 'enter'; paths: string[]; position: Point }
  | { type: 'over'; position: Point }
  | { type: 'drop'; paths: string[]; position: Point }
  | { type: 'leave' };
