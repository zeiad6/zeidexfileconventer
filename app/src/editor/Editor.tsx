/**
 * Visual editor: crop (one box), redact (boxes, optional time range, OCR text detection),
 * annotate (shapes on a transparent layer composited at full resolution), adjust (live colour
 * preview), the PDF page organizer and the trim / snapshot / join timeline.
 *
 * Boxes and shapes are stored in preview pixels and scaled to source pixels only when the job is
 * sent, so the numbers the user sees while dragging are the numbers that reach Rust.
 */

import { useEffect, useRef, useState, type ChangeEvent, type PointerEvent as ReactPointerEvent } from 'react';
import {
  closeEditor,
  detectFaces,
  detectText,
  editorState,
  frameAt,
  runJob,
  safeListen,
  saveAnnotation,
} from '../lib/api';
import { useI18n } from '../lib/i18n';
import type { Clip, EditorState, Point } from '../lib/types';

interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface Shape {
  type: string;
  color: string;
  size: number;
  pts: Point[];
  text?: string;
  n?: number;
}

interface PageItem {
  t: number;
  f: number;
  n: number;
  rot: number;
}

interface Adjust {
  brightness: number;
  contrast: number;
  saturation: number;
  gamma: number;
  hue: number;
  sharpen: number;
  clarity: number;
  denoise: number;
  grain: number;
}

type ColorStyle = 'blur' | 'black' | 'white';

const RATIOS: [string, number][] = [
  ['free', 0],
  ['1:1', 1],
  ['4:3', 4 / 3],
  ['3:2', 3 / 2],
  ['16:9', 16 / 9],
  ['9:16', 9 / 16],
  ['4:5', 4 / 5],
];

const COLORS: Record<ColorStyle, string> = { blur: 'blur', black: '#000000', white: '#ffffff' };

const NEUTRAL: Adjust = {
  brightness: 0,
  contrast: 1,
  saturation: 1,
  gamma: 1,
  hue: 0,
  sharpen: 0,
  clarity: 0,
  denoise: 0,
  grain: 0,
};

/** `[key, min, max, step]`; the order is the slider order the UI tests rely on. */
const SLIDERS: [keyof Adjust, number, number, number][] = [
  ['brightness', -1, 1, 0.02],
  ['contrast', 0, 3, 0.02],
  ['saturation', 0, 3, 0.02],
  ['hue', -180, 180, 1],
  ['gamma', 0.1, 3, 0.02],
  ['sharpen', 0, 3, 0.05],
  ['clarity', 0, 2, 0.05],
  ['denoise', 0, 20, 0.5],
  ['grain', 0, 60, 1],
];

/** Pause before fetching a frame while the slider moves, so scrubbing stays smooth. */
const SCRUB_DEBOUNCE_MS = 120;

const clock = (value: number) => {
  const minutes = Math.floor(value / 60);
  const seconds = (value - minutes * 60).toFixed(3).padStart(6, '0');
  return `${minutes}:${seconds}`;
};

const adjustCss = (adj: Adjust) =>
  `brightness(${1 + adj.brightness}) contrast(${adj.contrast}) saturate(${adj.saturation}) hue-rotate(${adj.hue}deg)`;

function normBox(
  width: number,
  height: number,
  x0: number,
  y0: number,
  x1: number,
  y1: number,
  aspect: number,
  cropping: boolean,
): Box {
  let right = x1;
  let bottom = y1;
  if (aspect && cropping) {
    const w = right - x0;
    const h = Math.sign(bottom - y0 || 1) * (Math.abs(w) / aspect);
    bottom = y0 + h;
  }
  const x = Math.max(0, Math.min(x0, right));
  const y = Math.max(0, Math.min(y0, bottom));
  return {
    x,
    y,
    w: Math.min(width, Math.max(x0, right)) - x,
    h: Math.min(height, Math.max(y0, bottom)) - y,
  };
}

function arrow(ctx: CanvasRenderingContext2D, a: Point, b: Point, w: number, color: string) {
  const angle = Math.atan2(b.y - a.y, b.x - a.x);
  const head = w * 4;
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = w;
  ctx.lineCap = 'round';
  ctx.beginPath();
  ctx.moveTo(a.x, a.y);
  ctx.lineTo(b.x - Math.cos(angle) * head * 0.8, b.y - Math.sin(angle) * head * 0.8);
  ctx.stroke();
  ctx.beginPath();
  ctx.moveTo(b.x, b.y);
  ctx.lineTo(b.x - head * Math.cos(angle - 0.45), b.y - head * Math.sin(angle - 0.45));
  ctx.lineTo(b.x - head * Math.cos(angle + 0.45), b.y - head * Math.sin(angle + 0.45));
  ctx.closePath();
  ctx.fill();
}

function drawShape(ctx: CanvasRenderingContext2D, shape: Shape) {
  const w = shape.size;
  const first = shape.pts[0];
  const last = shape.pts[shape.pts.length - 1];
  ctx.save();
  if (shape.type === 'rect') {
    const r = normBox(Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER, first.x, first.y, last.x, last.y, 0, false);
    ctx.strokeStyle = shape.color;
    ctx.lineWidth = w;
    ctx.strokeRect(r.x, r.y, r.w, r.h);
  } else if (shape.type === 'arrow') {
    arrow(ctx, first, last, w, shape.color);
  } else if (shape.type === 'pen') {
    ctx.strokeStyle = shape.color;
    ctx.globalAlpha = 0.45; // highlighter
    ctx.lineWidth = w * 4;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.beginPath();
    shape.pts.forEach((p, i) => (i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y)));
    ctx.stroke();
  } else if (shape.type === 'text') {
    ctx.fillStyle = shape.color;
    ctx.font = `600 ${w * 6}px system-ui, "Noto Sans Arabic", sans-serif`;
    ctx.textBaseline = 'top';
    ctx.fillText(shape.text ?? '', first.x, first.y);
  } else if (shape.type === 'number') {
    const r = w * 4;
    ctx.fillStyle = shape.color;
    ctx.beginPath();
    ctx.arc(first.x, first.y, r, 0, 2 * Math.PI);
    ctx.fill();
    ctx.fillStyle = '#fff';
    ctx.font = `700 ${r * 1.1}px system-ui, sans-serif`;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(String(shape.n ?? ''), first.x, first.y + 1);
  }
  ctx.restore();
}

/** Preview pixel under the pointer, clamped to the canvas. */
function point(event: { clientX: number; clientY: number }, canvas: HTMLCanvasElement): Point {
  const rect = canvas.getBoundingClientRect();
  const x = ((event.clientX - rect.left) * canvas.width) / (rect.width || 1);
  const y = ((event.clientY - rect.top) * canvas.height) / (rect.height || 1);
  return { x: Math.max(0, Math.min(canvas.width, x)), y: Math.max(0, Math.min(canvas.height, y)) };
}

interface Drag {
  start?: Point;
  box: Box | null;
  shape?: Shape;
}

export function Editor() {
  const { t, ar, toggle } = useI18n();
  const cv = useRef<HTMLCanvasElement>(null);
  const frameTimer = useRef<number | undefined>(undefined);
  const langInput = useRef<HTMLInputElement>(null);
  const startInput = useRef<HTMLInputElement>(null);
  const endInput = useRef<HTMLInputElement>(null);
  const dragRef = useRef<Drag | null>(null);

  const [st, setSt] = useState<EditorState | null>(null);
  const [img, setImg] = useState<HTMLImageElement | null>(null);
  const [boxes, setBoxes] = useState<Box[]>([]);
  const [shapes, setShapes] = useState<Shape[]>([]);
  const [pages, setPages] = useState<PageItem[]>([]);
  const [clips, setClips] = useState<Clip[]>([]);
  const [adj, setAdj] = useState<Adjust>({ ...NEUTRAL });
  const [aspect, setAspect] = useState(0);
  const [style, setStyle] = useState<ColorStyle>('blur');
  const [pen, setPen] = useState({ type: 'rect', color: '#f07b1d', size: 4, text: '' });
  const [drag, setDrag] = useState<Drag | null>(null);
  const [now, setNow] = useState(0);
  const [cut, setCut] = useState({ start: 0, end: 0 });
  const [err, setErr] = useState('');
  const [info, setInfo] = useState('');

  // Every callback below reads the freshest state through this ref, so a debounced frame fetch or
  // a key press can never act on a stale closure.
  const latest = useRef({
    st,
    boxes,
    shapes,
    pages,
    clips,
    adj,
    aspect,
    style,
    now,
    cut,
  });
  latest.current = { st, boxes, shapes, pages, clips, adj, aspect, style, now, cut };

  const organizer = st?.tool === 'pages' || st?.tool === 'merge-pdf';
  const timeline = st?.tool === 'trim' || st?.tool === 'snapshot';
  const joining = st?.tool === 'join';

  const frame = () => 1 / (st?.fps || 25);
  const lineW = () => {
    const width = cv.current?.width ?? 0;
    return (Math.max(2, Math.round(width / 320)) * pen.size) / 4;
  };
  const scale = () => {
    const canvas = cv.current;
    if (!st || !canvas?.width) return 1;
    return st.width / canvas.width;
  };

  // ---------- drawing ----------

  useEffect(() => {
    const canvas = cv.current;
    const ctx = canvas?.getContext('2d');
    if (!canvas || !ctx || !img || !st) return;
    const width = img.naturalWidth || canvas.width;
    const height = img.naturalHeight || canvas.height;
    if (canvas.width !== width) canvas.width = width;
    if (canvas.height !== height) canvas.height = height;

    if (st.tool === 'trim' || st.tool === 'snapshot') {
      ctx.drawImage(img, 0, 0);
      return;
    }
    if (st.tool === 'adjust') {
      ctx.filter = adjustCss(adj);
      ctx.drawImage(img, 0, 0);
      ctx.filter = 'none';
      setInfo(`${st.width} × ${st.height} px`);
      return;
    }

    ctx.drawImage(img, 0, 0);
    const live = drag?.box ? [...boxes, drag.box] : boxes;
    if (st.tool === 'crop' && live.length) {
      const b = live[live.length - 1];
      ctx.save();
      ctx.fillStyle = 'rgba(0,0,0,0.55)';
      ctx.beginPath();
      ctx.rect(0, 0, canvas.width, canvas.height);
      ctx.rect(b.x, b.y, b.w, b.h);
      ctx.fill('evenodd');
      ctx.strokeStyle = '#fff';
      ctx.lineWidth = Math.max(1, canvas.width / 600);
      ctx.strokeRect(b.x, b.y, b.w, b.h);
      ctx.restore();
    } else if (st.tool === 'redact') {
      for (const b of live) {
        ctx.save();
        if (style === 'blur') {
          ctx.beginPath();
          ctx.rect(b.x, b.y, b.w, b.h);
          ctx.clip();
          ctx.filter = `blur(${Math.max(6, canvas.width / 90)}px)`;
          ctx.drawImage(img, 0, 0);
        } else {
          ctx.fillStyle = COLORS[style];
          ctx.fillRect(b.x, b.y, b.w, b.h);
        }
        ctx.restore();
      }
    } else if (st.tool === 'annotate') {
      shapes.forEach((shape) => drawShape(ctx, shape));
      if (drag?.shape) drawShape(ctx, drag.shape);
    }

    const b = st.tool === 'crop' ? live[live.length - 1] : null;
    setInfo(
      b
        ? `${Math.round(b.w * scale())} × ${Math.round(b.h * scale())} px`
        : `${st.width} × ${st.height} px${st.paths.length > 1 ? ` · ${st.paths.length} files` : ''}`,
    );
    // `scale` and `lineW` read refs, so they are intentionally not dependencies.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [st, img, boxes, shapes, drag, adj, style]);

  // Organizer and join summaries share the info line with the canvas tools.
  useEffect(() => {
    if (st?.tool === 'pages' || st?.tool === 'merge-pdf') setInfo(`${pages.length} ${t('pagesCount')}`);
  }, [pages, st, t]);
  useEffect(() => {
    if (st?.tool !== 'join') return;
    const total = clips.reduce((sum, clip) => sum + clip.duration, 0);
    setInfo(`${clips.length} · ${clock(total)}`);
  }, [clips, st]);

  // ---------- opening ----------

  const open = async () => {
    setErr('');
    let next: EditorState;
    try {
      next = await editorState();
    } catch {
      return;
    }
    setSt(next);
    setBoxes([]);
    setShapes([]);
    setPages([]);
    setClips([]);
    setAspect(0);
    setAdj({ ...NEUTRAL });
    setStyle('blur');
    setPen({ type: 'rect', color: '#f07b1d', size: 4, text: '' });
    setNow(0);
    setCut({ start: 0, end: next.duration });
    setInfo('');
    dragRef.current = null;
    setDrag(null);

    if (next.tool === 'join') {
      setClips((next.clips ?? []).map((clip) => ({ ...clip })));
      setImg(null);
      return;
    }
    if (next.tool === 'pages' || next.tool === 'merge-pdf') {
      setPages(next.page_refs.map(([f, n], index) => ({ t: index, f, n, rot: 0 })));
      setImg(null);
      return;
    }
    if (!next.preview) {
      setImg(null);
      return;
    }
    const image = new Image();
    image.onload = () => setImg(image);
    image.src = next.preview;
  };
  const openRef = useRef(open);
  openRef.current = open;

  useEffect(() => {
    void openRef.current();
    return safeListen('editor-open', () => {
      void openRef.current();
    });
  }, []);

  useEffect(() => () => window.clearTimeout(frameTimer.current), []);

  // ---------- pointer: boxes and shapes ----------

  const updateDrag = (next: Drag | null) => {
    dragRef.current = next;
    setDrag(next);
  };

  const onPointerDown = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    const s = latest.current.st;
    const canvas = cv.current;
    if (!s || !canvas) return;
    if (['adjust', 'pages', 'merge-pdf', 'trim', 'snapshot', 'join'].includes(s.tool)) return;
    canvas.setPointerCapture?.(event.pointerId);
    const p = point(event, canvas);
    if (s.tool === 'annotate') {
      if (pen.type === 'text') {
        if (pen.text.trim()) {
          setShapes((list) => [...list, { type: 'text', color: pen.color, size: lineW(), pts: [p], text: pen.text }]);
        }
        return;
      }
      if (pen.type === 'number') {
        setShapes((list) => {
          const n = list.filter((shape) => shape.type === 'number').length + 1;
          return [...list, { type: 'number', color: pen.color, size: lineW(), pts: [p], n }];
        });
        return;
      }
      updateDrag({ shape: { type: pen.type, color: pen.color, size: lineW(), pts: [p] }, box: null });
      return;
    }
    updateDrag({ start: p, box: null });
  };

  const onPointerMove = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    const current = dragRef.current;
    const canvas = cv.current;
    const s = latest.current.st;
    if (!current || !canvas || !s) return;
    const p = point(event, canvas);
    if (current.shape) {
      updateDrag({ ...current, shape: { ...current.shape, pts: [...current.shape.pts, p] } });
      return;
    }
    if (!current.start) return;
    const b = normBox(canvas.width, canvas.height, current.start.x, current.start.y, p.x, p.y, aspect, s.tool === 'crop');
    updateDrag({ ...current, box: b });
  };

  const onPointerUp = () => {
    const current = dragRef.current;
    if (!current) return;
    updateDrag(null);
    if (current.shape && current.shape.pts.length > 1) {
      const shape = current.shape;
      setShapes((list) => [...list, shape]);
    }
    if (current.box && current.box.w > 2 && current.box.h > 2) {
      const box = current.box;
      if (latest.current.st?.tool === 'crop') setBoxes([box]);
      else setBoxes((list) => [...list, box]);
    }
  };

  // ---------- timeline ----------

  const seek = (value: number) => {
    const s = latest.current.st;
    if (!s) return;
    const next = Math.min(Math.max(0, value), s.duration);
    setNow(next);
    if (!s.video) return;
    window.clearTimeout(frameTimer.current);
    const want = s;
    frameTimer.current = window.setTimeout(async () => {
      try {
        const url = await frameAt(next);
        if (latest.current.st !== want) return;
        const image = new Image();
        image.onload = () => {
          if (latest.current.st === want) setImg(image);
        };
        image.src = url;
      } catch (e) {
        setErr(String(e));
      }
    }, SCRUB_DEBOUNCE_MS);
  };

  const setStart = () => {
    const s = latest.current.st;
    if (!s) return;
    setCut((range) => ({ ...range, start: Math.min(latest.current.now, range.end - frame()) }));
  };

  const setEnd = () => {
    const s = latest.current.st;
    if (!s) return;
    setCut((range) => ({ ...range, end: Math.max(latest.current.now, range.start + frame()) }));
  };

  const saveFrame = async () => {
    const s = latest.current.st;
    if (!s) return;
    setErr('');
    try {
      await runJob(s.paths, 'tool', 'snapshot', { at: latest.current.now.toFixed(3) });
    } catch (e) {
      setErr(String(e));
    }
  };

  // ---------- detection ----------

  const detectBoxes = async (kind: 'text' | 'pii' | 'faces') => {
    setErr('');
    try {
      const lang = langInput.current?.value || (ar ? 'ara+eng' : 'eng');
      const found =
        kind === 'faces' ? await detectFaces() : await detectText(kind === 'pii' ? `pii:${lang}` : lang);
      if (!found.length) throw t(kind === 'text' ? 'noText' : kind === 'pii' ? 'noPii' : 'noFaces');
      setBoxes((list) => [...list, ...found]);
    } catch (e) {
      setErr(String(e));
    }
  };

  // ---------- applying ----------

  const close = () => {
    window.clearTimeout(frameTimer.current);
    setImg(null);
    setSt(null);
    const canvas = cv.current;
    const ctx = canvas?.getContext('2d');
    if (canvas && ctx) ctx.clearRect(0, 0, canvas.width, canvas.height);
    void closeEditor().catch(() => {});
  };

  const apply = async () => {
    const s = latest.current.st;
    if (!s) return;
    setErr('');
    try {
      if (s.tool === 'annotate') {
        const list = latest.current.shapes;
        if (!list.length) throw t('drawSomething');
        // Only the annotation layer goes to Rust; it is scaled onto the full-size original.
        const layer = document.createElement('canvas');
        layer.width = cv.current?.width ?? s.width;
        layer.height = cv.current?.height ?? s.height;
        const ctx = layer.getContext('2d');
        if (ctx) list.forEach((shape) => drawShape(ctx, shape));
        await saveAnnotation({
          src: s.paths[0],
          data: layer.toDataURL('image/png'),
          width: s.width,
          height: s.height,
        });
      } else if (s.tool === 'trim') {
        const range = latest.current.cut;
        const opts: Record<string, string> = { start: range.start.toFixed(3) };
        if (range.end < s.duration - 0.001) opts.end = range.end.toFixed(3);
        await runJob(s.paths, 'tool', 'trim', opts);
      } else if (s.tool === 'snapshot') {
        await runJob(s.paths, 'tool', 'snapshot', { at: latest.current.now.toFixed(3) });
      } else if (s.tool === 'join') {
        const list = latest.current.clips;
        if (list.length < 2) throw t('twoClips');
        await runJob(
          list.map((clip) => clip.path),
          'tool',
          'join',
          {},
        );
      } else if (s.tool === 'adjust') {
        const opts = Object.fromEntries(Object.entries(latest.current.adj).map(([key, value]) => [key, String(value)]));
        await runJob(s.paths, 'tool', 'adjust', opts);
      } else if (s.tool === 'pages' || s.tool === 'merge-pdf') {
        const list = latest.current.pages;
        if (!list.length) throw t('noPages');
        const rotate = list
          .map((page, i) => (page.rot ? `${i + 1}:${page.rot}` : ''))
          .filter(Boolean)
          .join(',');
        if (s.paths.length > 1) {
          const items = list.map((page) => `${page.f}:${page.n}`).join(',');
          await runJob(s.paths, 'tool', 'arrange', { items, rotate });
        } else {
          const spec = list.map((page) => page.n).join(',');
          await runJob(s.paths, 'tool', 'pages', { spec, rotate });
        }
      } else {
        const list = latest.current.boxes;
        if (!list.length) throw t('drawBox');
        const src = (b: Box) =>
          [b.x, b.y, b.w, b.h]
            .map((value) => Math.round(value * scale()))
            .join(',');
        const opts: Record<string, string> =
          s.tool === 'crop'
            ? { box: src(list[0]) }
            : {
                boxes: list.map(src).join(';'),
                style: COLORS[latest.current.style],
                start: startInput.current?.value || '',
                end: endInput.current?.value || '',
              };
        await runJob(s.paths, 'tool', s.tool, opts);
      }
      close();
    } catch (e) {
      setErr(String(e));
    }
  };
  const applyRef = useRef(apply);
  applyRef.current = apply;
  const closeRef = useRef(close);
  closeRef.current = close;

  const undo = () => {
    if (latest.current.st?.tool === 'annotate') setShapes((list) => list.slice(0, -1));
    else setBoxes((list) => list.slice(0, -1));
  };

  // Everything below reads refs, so one stable listener always sees the current editor.
  const keyHandler = (event: KeyboardEvent) => {
    const target = event.target as HTMLElement | null;
    if (target && ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName)) return;
    const s = latest.current.st;
    if (event.key === 'Escape') {
      closeRef.current();
      return;
    }
    if (event.key === 'Enter') {
      void applyRef.current();
      return;
    }
    if (s && (s.tool === 'trim' || s.tool === 'snapshot')) {
      const step = event.shiftKey ? 1 : 1 / (s.fps || 25);
      if (event.key === 'ArrowLeft') seek(latest.current.now - step);
      if (event.key === 'ArrowRight') seek(latest.current.now + step);
      if (event.key === 'i' && s.tool === 'trim') setStart();
      if (event.key === 'o' && s.tool === 'trim') setEnd();
    }
    if ((event.ctrlKey || event.metaKey) && event.key === 'z') {
      event.preventDefault();
      undo();
    }
  };
  const keyRef = useRef(keyHandler);
  keyRef.current = keyHandler;
  useEffect(() => {
    const listener = (event: KeyboardEvent) => keyRef.current(event);
    document.addEventListener('keydown', listener);
    return () => document.removeEventListener('keydown', listener);
  }, []);

  // ---------- pages and clips ----------

  const movePage = (from: number, to: number) => {
    setPages((list) => {
      if (to < 0 || to >= list.length) return list;
      const next = [...list];
      const [item] = next.splice(from, 1);
      next.splice(to, 0, item);
      return next;
    });
  };

  const moveClip = (from: number, to: number) => {
    setClips((list) => {
      if (to < 0 || to >= list.length) return list;
      const next = [...list];
      const [item] = next.splice(from, 1);
      next.splice(to, 0, item);
      return next;
    });
  };

  // ---------- controls ----------

  const controls = () => {
    if (!st) return null;
    if (st.tool === 'crop') {
      return (
        <select
          defaultValue="free"
          onChange={(e: ChangeEvent<HTMLSelectElement>) => {
            setAspect(RATIOS.find(([name]) => name === e.target.value)?.[1] ?? 0);
          }}
        >
          {RATIOS.map(([name]) => (
            <option key={name} value={name}>
              {name === 'free' ? t('free') : name}
            </option>
          ))}
        </select>
      );
    }
    if (st.tool === 'redact') {
      return (
        <>
          <select value={style} onChange={(e) => setStyle(e.target.value as ColorStyle)}>
            {(Object.keys(COLORS) as ColorStyle[]).map((name) => (
              <option key={name} value={name}>
                {t(name)}
              </option>
            ))}
          </select>
          {st.video && (
            <>
              <input id="start" ref={startInput} placeholder={t('from')} size={6} />
              <input id="end" ref={endInput} placeholder={t('to')} size={6} />
            </>
          )}
          <button type="button" onClick={() => void detectBoxes('faces')}>
            {t('detectFaces')}
          </button>
          <input id="lang" ref={langInput} defaultValue={ar ? 'ara+eng' : 'eng'} size={7} title="Tesseract" />
          <button type="button" onClick={() => void detectBoxes('text')}>
            {t('detectText')}
          </button>
          <button type="button" onClick={() => void detectBoxes('pii')}>
            {t('detectPii')}
          </button>
        </>
      );
    }
    if (st.tool === 'adjust') {
      return (
        <>
          {SLIDERS.map(([key, min, max, step]) => (
            <label key={key} className="slider">
              {t(key)}
              <input
                type="range"
                min={min}
                max={max}
                step={step}
                value={adj[key]}
                onChange={(e) => setAdj((current) => ({ ...current, [key]: Number(e.target.value) }))}
              />
            </label>
          ))}
          <button
            type="button"
            onClick={() => {
              setAdj({ ...NEUTRAL });
            }}
          >
            {t('reset')}
          </button>
        </>
      );
    }
    if (st.tool === 'pages' || st.tool === 'merge-pdf') {
      return <span className="hint">{t('pagesHint')}</span>;
    }
    return (
      <>
        {['rect', 'arrow', 'text', 'number', 'pen'].map((kind) => (
          <button
            key={kind}
            type="button"
            className={pen.type === kind ? 'on' : ''}
            onClick={() => setPen((current) => ({ ...current, type: kind }))}
          >
            {t(kind)}
          </button>
        ))}
        <input
          type="color"
          value={pen.color}
          onChange={(e) => setPen((current) => ({ ...current, color: e.target.value }))}
        />
        <input
          type="range"
          min={1}
          max={12}
          value={pen.size}
          onChange={(e) => setPen((current) => ({ ...current, size: Number(e.target.value) }))}
        />
        {pen.type === 'text' && (
          <input
            placeholder={t('text')}
            value={pen.text}
            onChange={(e) => setPen((current) => ({ ...current, text: e.target.value }))}
          />
        )}
      </>
    );
  };

  const undoHidden = organizer || joining || timeline || st?.tool === 'adjust';
  const duration = st?.duration || 1;

  return (
    <>
      <header className="ebar">
        <strong id="title">{st ? t(st.tool) : ''}</strong>
        <div id="controls" className="controls">
          {controls()}
        </div>
        <span className="spacer" />
        <button type="button" className="btn-subtle" id="langToggle" title="Switch language" onClick={toggle}>
          {t('langSwitch')}
        </button>
        <button id="undo" hidden={undoHidden} onClick={undo}>
          {t('undo')}
        </button>
        <button id="cancel" onClick={close}>
          {t('cancel')}
        </button>
        <button id="apply" className="primary" onClick={() => void apply()}>
          {t(st?.tool === 'annotate' ? 'save' : 'apply')}
        </button>
      </header>

      <p className="err" id="err" role="alert">
        {err}
      </p>

        <div className="editor-stage">
          <canvas
          id="canvas"
          ref={cv}
          className="editor-canvas"
          hidden={!st || organizer || joining}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
        />
        <div id="pages" className="pages" hidden={!organizer}>
          {pages.map((page, i) => {
            const origin = st && st.paths.length > 1 ? `${(st.paths[page.f - 1] ?? '').split(/[\\/]/).pop()} · ` : '';
            return (
              <div className="page-card" key={`${page.f}-${page.n}-${i}`}>
                <img src={st?.pages[page.t]} alt={String(page.n)} style={{ transform: `rotate(${page.rot}deg)` }} />
                <div className="page-n">{`${i + 1} · ${origin}p${page.n}`}</div>
                <div className="page-tools">
                  <button type="button" title={t('moveLeft')} onClick={() => movePage(i, i - 1)}>
                    ◀
                  </button>
                  <button
                    type="button"
                    title={t('rotate')}
                    onClick={() =>
                      setPages((list) => list.map((item, at) => (at === i ? { ...item, rot: (item.rot + 90) % 360 } : item)))
                    }
                  >
                    ⟳
                  </button>
                  <button
                    type="button"
                    title={t('duplicate')}
                    onClick={() =>
                      setPages((list) => [...list.slice(0, i + 1), { ...page }, ...list.slice(i + 1)])
                    }
                  >
                    ⧉
                  </button>
                  <button type="button" title={t('remove')} onClick={() => setPages((list) => list.filter((_, at) => at !== i))}>
                    ✕
                  </button>
                  <button type="button" title={t('moveRight')} onClick={() => movePage(i, i + 1)}>
                    ▶
                  </button>
                </div>
              </div>
            );
          })}
        </div>
        <div id="clips" className="clips" hidden={!joining}>
          {clips.map((clip, i) => (
            <div className="clip-card" key={`${clip.path}-${i}`}>
              {clip.thumb ? <img src={clip.thumb} alt="" /> : <div className="clip-audio">♪</div>}
              <div className="page-n">{`${i + 1} · ${clip.name}`}</div>
              <div className="page-n">{clock(clip.duration)}</div>
              <div className="page-tools">
                <button type="button" title={t('moveLeft')} onClick={() => moveClip(i, i - 1)}>
                  ◀
                </button>
                <button type="button" title={t('remove')} onClick={() => setClips((list) => list.filter((_, at) => at !== i))}>
                  ✕
                </button>
                <button type="button" title={t('moveRight')} onClick={() => moveClip(i, i + 1)}>
                  ▶
                </button>
              </div>
            </div>
          ))}
        </div>
      </div>

      <div id="timeline" className="timeline" hidden={!timeline}>
        <div className="wave">
          <img id="wave" alt="" src={st?.waveform || undefined} hidden={!st?.waveform} />
          <div
            id="range"
            className="range"
            hidden={st?.tool !== 'trim'}
            style={{
              left: `${(cut.start / duration) * 100}%`,
              width: `${((cut.end - cut.start) / duration) * 100}%`,
            }}
          />
          <div id="head" className="head" style={{ left: `${(now / duration) * 100}%` }} />
        </div>
        <input
          id="scrub"
          type="range"
          min={0}
          max={st?.duration ?? 0}
          step={st?.fps ? 1 / st.fps : 1 / 25}
          value={now}
          aria-label="time"
          onChange={(e) => seek(Number(e.target.value))}
        />
        <div className="tl-row">
          <button id="back1s" type="button" title="-1 s" onClick={() => seek(latest.current.now - 1)}>
            ⏪
          </button>
          <button id="back1f" type="button" title="-1 frame" onClick={() => seek(latest.current.now - frame())}>
            ◀
          </button>
          <strong id="clock">{clock(now)}</strong>
          <button id="fwd1f" type="button" title="+1 frame" onClick={() => seek(latest.current.now + frame())}>
            ▶
          </button>
          <button id="fwd1s" type="button" title="+1 s" onClick={() => seek(latest.current.now + 1)}>
            ⏩
          </button>
          <span className="spacer" />
          <button id="setStart" type="button" hidden={st?.tool !== 'trim'} onClick={setStart}>
            {t('setStart')}
          </button>
          <button id="setEnd" type="button" hidden={st?.tool !== 'trim'} onClick={setEnd}>
            {t('setEnd')}
          </button>
          <button id="saveFrame" type="button" hidden={st?.tool === 'trim' || !st?.video} onClick={() => void saveFrame()}>
            {t('saveFrame')}
          </button>
          <span id="span" className="hint">
            {st?.tool === 'trim'
              ? `${clock(cut.start)} → ${clock(cut.end)} (${(cut.end - cut.start).toFixed(2)} s)`
              : ''}
          </span>
        </div>
      </div>

      <p className="hint" id="info">
        {info}
      </p>
    </>
  );
}
