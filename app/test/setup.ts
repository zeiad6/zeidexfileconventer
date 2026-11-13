/**
 * Vitest setup for the UI: jsdom knows nothing about canvases and images, so the two browser APIs
 * the editor genuinely uses are given deterministic stubs here instead of being mocked per test.
 */

import '@testing-library/jest-dom/vitest';
import { beforeEach } from 'vitest';

/** A 2-D context that accepts every call and remembers every assigned property. */
function fakeContext() {
  const state: Record<string, unknown> = {};
  return new Proxy(state, {
    get(target, key) {
      if (typeof key === 'symbol') return undefined;
      if (key in target) return target[key];
      const fn = () => {};
      target[key] = fn;
      return fn;
    },
    set(target, key, value) {
      if (typeof key !== 'symbol') target[key] = value;
      return true;
    },
  }) as unknown as CanvasRenderingContext2D;
}

const contexts = new WeakMap<HTMLCanvasElement, CanvasRenderingContext2D>();

HTMLCanvasElement.prototype.getContext = function getContext(this: HTMLCanvasElement) {
  let ctx = contexts.get(this);
  if (!ctx) {
    ctx = fakeContext();
    contexts.set(this, ctx);
  }
  return ctx;
} as unknown as HTMLCanvasElement['getContext'];

HTMLCanvasElement.prototype.toDataURL = () => `data:image/png;base64,${'A'.repeat(2048)}`;

// The old harness used 800 × 600 previews of a 1600 × 1200 source: a 2× scale factor keeps the
// preview-pixel → source-pixel assertions easy to read.
const IMAGE_WIDTH = 800;
const IMAGE_HEIGHT = 600;

class FakeImage {
  naturalWidth = IMAGE_WIDTH;
  naturalHeight = IMAGE_HEIGHT;
  width = IMAGE_WIDTH;
  height = IMAGE_HEIGHT;
  onload: (() => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  private source = '';

  get src() {
    return this.source;
  }

  set src(value: string) {
    this.source = value;
    window.setTimeout(() => this.onload?.(), 0);
  }
}

Object.defineProperty(window, 'Image', { configurable: true, writable: true, value: FakeImage });

// The editor maps pointer coordinates through the canvas rect, so the rect must follow the
// canvas size the way a real laid-out canvas would.
HTMLCanvasElement.prototype.getBoundingClientRect = function getBoundingClientRect(this: HTMLCanvasElement) {
  return {
    x: 0,
    y: 0,
    left: 0,
    top: 0,
    right: this.width,
    bottom: this.height,
    width: this.width,
    height: this.height,
    toJSON: () => ({}),
  } as DOMRect;
};

// jsdom has no PointerEvent: the editor's drag handlers read clientX/clientY, which MouseEvent
// already provides, so a MouseEvent subclass is enough to exercise them faithfully.
if (!('PointerEvent' in window)) {
  class PointerEvent extends MouseEvent {
    pointerId: number;
    pointerType: string;
    constructor(type: string, init: MouseEventInit & { pointerId?: number; pointerType?: string } = {}) {
      super(type, init);
      this.pointerId = init.pointerId ?? 0;
      this.pointerType = init.pointerType ?? 'mouse';
    }
  }
  Object.defineProperty(window, 'PointerEvent', { configurable: true, writable: true, value: PointerEvent });
}

if (!window.matchMedia) {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    }),
  });
}

beforeEach(() => {
  window.localStorage.clear();
  document.documentElement.dir = 'ltr';
  document.documentElement.lang = 'en';
});
