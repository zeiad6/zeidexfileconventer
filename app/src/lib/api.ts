/**
 * The only place that talks to Tauri. Every command name and argument shape here matches
 * `app/src-tauri/src/lib.rs`; components never touch `window.__TAURI__` directly, so tests can
 * install a fake bridge before rendering and assert on the exact IPC traffic.
 */

import type { Desc, DragPayload, EditorState, Engine, Settings } from './types';

export interface ResizeDirection {
  toString(): string;
}

type Unlisten = () => void;
type Handler<T> = (payload: T) => void;

export interface TauriWindow {
  close(): Promise<void>;
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  startResizeDragging(direction: string): Promise<void>;
}

export interface TauriBridge {
  core: { invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> };
  event: { listen<T>(name: string, handler: Handler<{ payload: T }>): Promise<Unlisten> };
  window: { getCurrentWindow(): TauriWindow };
  webview: { getCurrentWebview(): { onDragDropEvent(h: Handler<{ payload: DragPayload }>): Promise<Unlisten> } };
  drag?: { startDrag(opts: { item: string[]; icon: string }, cb: (e: { result: string }) => void): Promise<void> };
}

declare global {
  interface Window {
    __TAURI__?: TauriBridge;
  }
}

export function bridge(): TauriBridge {
  const api = window.__TAURI__;
  if (!api) throw new Error('Tauri bridge is not available: open this window from the Zeidex app');
  return api;
}

export const invoke = async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> =>
  bridge().core.invoke<T>(cmd, args);

/** Subscribes to a backend event and returns the unlisten function. */
export const listen = <T>(name: string, handler: (payload: T) => void): Promise<Unlisten> =>
  bridge().event.listen<T>(name, (e) => handler(e.payload));

export const appWindow = (): TauriWindow => bridge().window.getCurrentWindow();

export const currentWebview = () => bridge().webview.getCurrentWebview();

/**
 * Subscribes to a backend event without assuming the bridge exists. A browser preview of the UI
 * (vite dev, Storybook, the test harness) has no Tauri, and no listener is a better outcome there
 * than a thrown error. Returns the unlisten function.
 */
export function safeListen<T>(name: string, handler: (payload: T) => void): Unlisten {
  let off: Unlisten | undefined;
  let dead = false;
  try {
    listen<T>(name, handler)
      .then((fn) => {
        if (dead) fn();
        else off = fn;
      })
      .catch(() => {});
  } catch {
    /* no bridge: nothing to unsubscribe */
  }
  return () => {
    dead = true;
    off?.();
  };
}

/** Native file drag-and-drop events for this webview, or nothing outside Tauri. */
export function onDragDrop(handler: (payload: DragPayload) => void): Unlisten {
  let off: Unlisten | undefined;
  let dead = false;
  try {
    currentWebview()
      .onDragDropEvent((event) => handler(event.payload))
      .then((fn) => {
        if (dead) fn();
        else off = fn;
      })
      .catch(() => {});
  } catch {
    /* no bridge */
  }
  return () => {
    dead = true;
    off?.();
  };
}


/** Opens an http(s) link in the system browser, falling back to a browser tab in dev. */
export async function openExternal(url: string): Promise<void> {
  try {
    await invoke('open_url', { url });
  } catch {
    window.open(url, '_blank');
  }
}

export async function copyText(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const area = document.createElement('textarea');
  area.value = text;
  area.style.position = 'fixed';
  area.style.opacity = '0';
  document.body.append(area);
  area.select();
  document.execCommand('copy');
  area.remove();
}

// Typed command wrappers: one line each, so a renamed command is a compile error here and
// nowhere else.

export const describe = (paths: string[]) => invoke<Desc>('describe', { paths });
export const runJob = (paths: string[], kind: string, id: string, opts: Record<string, string>) =>
  invoke<void>('run_job', { paths, kind, id, opts });
export const keepWheel = () => invoke<void>('keep_wheel');
export const hideWheel = () => invoke<void>('hide_wheel');
export const wheelAtShelf = (paths: string[] = []) => invoke<void>('wheel_at_shelf', { paths });
export const openEditor = (paths: string[], tool: string) => invoke<void>('open_editor', { paths, tool });
export const takePending = () => invoke<string[]>('take_pending');
export const doctor = () => invoke<Engine[]>('doctor');
/** Opens a terminal running the engine's install command (winget / brew / apt). */
export const installEngine = (id: string) => invoke<void>('install_engine', { id });
export const hookOk = () => invoke<boolean>('hook_ok');
export const integrateShell = () => invoke<string[]>('integrate_shell');
export const pickFiles = () => invoke<string[]>('pick_files');
export const pickFolder = () => invoke<string | null>('pick_folder');
export const openInHome = (paths: string[]) => invoke<void>('open_in_home', { paths });
export const showHome = () => invoke<void>('show_home');
export const getSettings = () => invoke<Settings>('get_settings');
export const setSettings = (settings: Settings) => invoke<void>('set_settings', { settings });
export const floatAdd = (paths: string[]) => invoke<void>('float_add', { paths });
export const floatList = () => invoke<string[]>('float_list');
export const floatClear = () => invoke<void>('float_clear');
export const dragIcon = () => invoke<string>('drag_icon');
export const reveal = (path: string) => invoke<void>('reveal', { path });
export const downloadDraggedUrl = (url: string) => invoke<string | null>('download_dragged_url', { url });
export const editorState = () => invoke<EditorState>('editor_state');
export const saveAnnotation = (args: { src: string; data: string; width: number; height: number }) =>
  invoke<void>('save_annotation', args);
export const closeEditor = () => invoke<void>('close_editor');
export const frameAt = (t: number) => invoke<string>('frame_at', { t });
export const detectText = (lang: string) =>
  invoke<{ x: number; y: number; w: number; h: number }[]>('detect_text', { lang });
export const localdropCancel = () => invoke<void>('localdrop_cancel');
export const detectFaces = () => invoke<{ x: number; y: number; w: number; h: number }[]>('detect_faces');
