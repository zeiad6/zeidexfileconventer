/**
 * Test harness: a fake `window.__TAURI__` that records every IPC call and lets a test push
 * backend events and native drag-and-drop payloads, plus the backend answers the windows expect.
 *
 * The bridge is installed before a component renders, exactly like Tauri installs it before the
 * webview loads; nothing in `src/` knows this is a test.
 */

import { act, fireEvent, render } from '@testing-library/react';
import type { ReactElement } from 'react';
import { I18nProvider } from '../src/lib/i18n';
import type { Choice, Desc, EditorState, Sub } from '../src/lib/types';

export const sleep = (ms = 0) =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, ms);
  });

export interface Call {
  cmd: string;
  // The argument shapes differ per command; narrowing happens in the assertions.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  args?: any;
}

export function choice(
  kind: string,
  id: string,
  label: string,
  params: Choice['params'] = [],
  editor = false,
  children: Sub[] = [],
): Choice {
  return { kind, id, label, label_ar: label, params, editor, children };
}

export const quality: Sub[] = ['90', '80', '60'].map((value) => ({
  key: 'quality',
  value,
  label: `Quality ${value}`,
  label_ar: '',
  custom: false,
}));

/** The video description every window uses: ten formats, three of them with quality presets. */
export function videoDesc(): Desc {
  return {
    category: 'video',
    mixed: false,
    convert: ['mp4', 'mov', 'mkv', 'webm', 'avi', 'wmv', 'gif', 'mp3', 'm4a', 'wav'].map((t) =>
      choice('convert', t, t.toUpperCase(), [], false, ['webm', 'mp4', 'mov'].includes(t) ? quality : []),
    ),
    tools: [
      choice('tool', 'compress', 'Compress', [{ key: 'percent', label: 'Reduce by %', label_ar: '', default: '50' }], false, [
        { key: 'percent', value: '50', label: 'Reduce by % 50', label_ar: '', custom: false },
        { key: 'custom', value: '', label: 'Custom…', label_ar: '', custom: true },
      ]),
      choice('tool', 'crop', 'Crop', [], true),
      choice('tool', 'trim', 'Trim'),
      choice('tool', 'speed', 'Speed'),
      choice('tool', 'rename', 'Batch rename', [
        { key: 'pattern', label: 'Pattern', label_ar: '', default: '{name}-{n:2}' },
      ]),
      { ...choice('tool', 'recipe', '★ Web image'), preset: { steps: 'resize max=1600 | compress percent=60' } },
    ],
    actions: ['reveal', 'copy-path', 'move', 'terminal'].map((a) => choice('action', a, a)),
  };
}

export function editorState(tool: string): EditorState {
  if (tool === 'pages') {
    return {
      paths: ['/home/u/d.pdf'],
      tool,
      video: false,
      width: 0,
      height: 0,
      duration: 0,
      preview: '',
      pages: ['data:p1', 'data:p2', 'data:p3'],
      page_refs: [
        [1, 1],
        [1, 2],
        [1, 3],
      ],
    };
  }
  if (tool === 'merge-pdf') {
    return {
      paths: ['/u/a.pdf', '/u/b.pdf'],
      tool,
      video: false,
      width: 0,
      height: 0,
      duration: 0,
      preview: '',
      pages: ['a1', 'a2', 'b1'],
      page_refs: [
        [1, 1],
        [1, 2],
        [2, 1],
      ],
    };
  }
  if (tool === 'trim' || tool === 'snapshot') {
    return {
      paths: ['/u/clip.mp4'],
      tool,
      video: true,
      width: 1280,
      height: 720,
      duration: 10,
      preview: 'data:image/png;base64,AAAA',
      pages: [],
      page_refs: [],
      fps: 25,
      waveform: 'data:image/png;base64,WAVE',
      clips: [],
    };
  }
  if (tool === 'join') {
    return {
      paths: ['/u/a.mp4', '/u/b.mov', '/u/c.mp4'],
      tool,
      video: true,
      width: 0,
      height: 0,
      duration: 0,
      preview: '',
      pages: [],
      page_refs: [],
      fps: 0,
      waveform: '',
      clips: [
        { path: '/u/a.mp4', name: 'a.mp4', duration: 4, thumb: 'data:a' },
        { path: '/u/b.mov', name: 'b.mov', duration: 2, thumb: 'data:b' },
        { path: '/u/c.mp4', name: 'c.mp4', duration: 1.5, thumb: '' },
      ],
    };
  }
  return {
    paths: ['/home/u/p.png', '/home/u/q.png'],
    tool: tool || 'redact',
    video: false,
    width: 1600,
    height: 1200,
    duration: 0,
    preview: 'data:image/png;base64,AAAA',
    pages: [],
    page_refs: [],
  };
}

/** Answers the backend gives when a test does not override them. */
export function defaultResponses(): Record<string, unknown> {
  return {
    describe: videoDesc(),
    doctor: [
      { id: 'ffmpeg', name: 'FFmpeg', path: '/usr/bin/ffmpeg', hint: '' },
      { id: 'vips', name: 'libvips', path: null, hint: 'apt' },
    ],
    hook_ok: true,
    take_pending: ['/home/u/clip.mp4'],
    get_settings: {
      convert: 'Shift',
      tools: 'Alt',
      actions: 'Control',
      reveal: true,
      hotkey: true,
      recipes: [{ name: 'Web image', steps: 'resize max=1600 | strip' }],
      watch: [{ dir: '/home/u/in', exts: 'heic', steps: 'convert to=jpg', enabled: true }],
    },
    pick_folder: '/home/u/new',
    pick_files: ['/home/u/picked.png'],
    detect_text: [{ x: 5, y: 6, w: 70, h: 20 }],
    detect_faces: [{ x: 100, y: 50, w: 60, h: 60 }],
    frame_at: 'data:image/png;base64,FRAME',
    float_list: ['/home/u/a.txt', '/home/u/b.png'],
    drag_icon: 'data:image/png;base64,AAAA',
    drag_icon_path: 'data:image/png;base64,AAAA',
    editor_state: editorState('redact'),
  };
}

export interface Harness {
  calls: Call[];
  emit: (name: string, payload?: unknown) => Promise<void>;
  drag: (payload: unknown) => Promise<void>;
  key: (key: string) => Promise<void>;
  mouse: (type: 'mousemove' | 'click', x: number, y: number) => Promise<void>;
  wheel: (deltaY: number) => Promise<void>;
  /** Sleeps inside act, so a timer-driven state change is flushed like a user's. */
  wait: (ms: number) => Promise<void>;
  /** Clicks an element and lets any promises the handler started settle. */
  click: (el: Element | null | undefined) => Promise<void>;
  /** Presses the pointer on an element (resize strips act on pointerdown). */
  press: (el: Element | null | undefined) => Promise<void>;
  /** A pointer event on an arbitrary element, used by the orb's press-and-drag gesture. */
  pointerOn: (el: Element, type: 'pointerdown' | 'pointermove' | 'pointerup', x: number, y: number) => Promise<void>;
  /** Changes an input/select value and lets React commit it. */
  input: (el: Element | null | undefined, value: string, event?: 'input' | 'change') => Promise<void>;
  /** Points at a position and presses, moves and releases (one box drag). */
  dragBox: (canvas: HTMLCanvasElement, from: [number, number], to: [number, number]) => Promise<void>;
  pointer: (canvas: HTMLCanvasElement, type: 'pointerdown' | 'pointermove' | 'pointerup', fx: number, fy: number) => Promise<void>;
}

/** Installs the fake bridge for one test. Call before `mount`. */
export function installTauri(overrides: Record<string, unknown> = {}): Harness {
  const responses = { ...defaultResponses(), ...overrides };
  const calls: Call[] = [];
  const listeners: Record<string, Array<(event: { payload: unknown }) => void>> = {};
  const drops: Array<(event: { payload: unknown }) => void> = [];

  const api = {
    core: {
      invoke: async (cmd: string, args?: Record<string, unknown>) => {
        calls.push({ cmd, args });
        if (cmd === 'set_settings') {
          // Mirrors the Rust validation the settings form relies on.
          const s = args?.settings as { convert: string; tools: string; actions: string };
          if (s.convert === s.tools || s.tools === s.actions || s.convert === s.actions) {
            throw 'each wheel needs its own key';
          }
        }
        return responses[cmd];
      },
    },
    event: {
      listen: async (name: string, cb: (event: { payload: unknown }) => void) => {
        (listeners[name] ||= []).push(cb);
        return () => {
          listeners[name] = (listeners[name] ?? []).filter((fn) => fn !== cb);
        };
      },
    },
    window: {
      getCurrentWindow: () => ({
        close: async () => {
          calls.push({ cmd: 'winClose' });
        },
        minimize: async () => {
          calls.push({ cmd: 'winMinimize' });
        },
        toggleMaximize: async () => {
          calls.push({ cmd: 'winMaximize' });
        },
        startResizeDragging: async (dir: string) => {
          calls.push({ cmd: 'winResize', args: dir });
        },
      }),
    },
    webview: {
      getCurrentWebview: () => ({
        onDragDropEvent: async (handler: (event: { payload: unknown }) => void) => {
          drops.push(handler);
          return () => {
            const at = drops.indexOf(handler);
            if (at >= 0) drops.splice(at, 1);
          };
        },
      }),
    },
    drag: {
      startDrag: async (opts: unknown, cb: (event: { result: string }) => void) => {
        calls.push({ cmd: 'startDrag', args: opts });
        cb({ result: 'Dropped' });
      },
    },
  };

  (window as unknown as { __TAURI__?: unknown }).__TAURI__ = api;

  const settle = async (fn: () => void) => {
    await act(async () => {
      fn();
      await sleep(8);
    });
  };

  return {
    calls,
    emit: (name, payload) => settle(() => (listeners[name] ?? []).forEach((cb) => cb({ payload }))),
    drag: (payload) => settle(() => drops.forEach((cb) => cb({ payload }))),
    key: (key) =>
      settle(() => {
        document.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
      }),
    mouse: (type, x, y) =>
      settle(() => {
        document.dispatchEvent(new MouseEvent(type, { clientX: x, clientY: y, bubbles: true, cancelable: true }));
      }),
    wheel: (deltaY) =>
      settle(() => {
        document.dispatchEvent(new WheelEvent('wheel', { deltaY, bubbles: true, cancelable: true }));
      }),
    wait: (ms) => act(async () => sleep(ms)),
    click: (el) => {
      if (!el) throw new Error('click on a missing element');
      return settle(() => fireEvent.click(el));
    },
    press: (el) => {
      if (!el) throw new Error('press on a missing element');
      return settle(() => fireEvent.pointerDown(el, { pointerId: 1, bubbles: true, cancelable: true }));
    },
    pointerOn: (el, type, x, y) =>
      settle(() =>
        ({
          pointerdown: fireEvent.pointerDown,
          pointermove: fireEvent.pointerMove,
          pointerup: fireEvent.pointerUp,
        })[type](el, { pointerId: 1, clientX: x, clientY: y, bubbles: true }),
      ),
    input: (el, value, event = 'input') => {
      if (!el) throw new Error('input on a missing element');
      return settle(() => fireEvent[event](el, { target: { value } }));
    },
    pointer: (canvas, type, fx, fy) =>
      settle(() =>
        ({
          pointerdown: fireEvent.pointerDown,
          pointermove: fireEvent.pointerMove,
          pointerup: fireEvent.pointerUp,
        })[type](canvas, {
          pointerId: 1,
          clientX: fx * canvas.width,
          clientY: fy * canvas.height,
          bubbles: true,
        }),
      ),
    dragBox: async (canvas, from, to) => {
      await settle(() =>
        fireEvent.pointerDown(canvas, {
          pointerId: 1,
          clientX: from[0] * canvas.width,
          clientY: from[1] * canvas.height,
          bubbles: true,
        }),
      );
      await settle(() =>
        fireEvent.pointerMove(canvas, {
          pointerId: 1,
          clientX: to[0] * canvas.width,
          clientY: to[1] * canvas.height,
          bubbles: true,
        }),
      );
      await settle(() =>
        fireEvent.pointerUp(canvas, {
          pointerId: 1,
          clientX: to[0] * canvas.width,
          clientY: to[1] * canvas.height,
          bubbles: true,
        }),
      );
    },
  };
}

/** Renders one window's component inside the language provider. */
export function mount(node: ReactElement): ReturnType<typeof render> {
  return render(<I18nProvider>{node}</I18nProvider>);
}

/** Waits long enough for a fetch started on mount to land and render. */
export const flush = () => act(async () => sleep(12));

/** Commands the window sent, in order. */
export const commands = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd);
export const firstCall = (calls: Call[], cmd: string) => commands(calls, cmd)[0];
export const lastCall = (calls: Call[], cmd: string) => commands(calls, cmd).at(-1);
