// Visual editor: crop (one box), redact (boxes, optional time range, OCR text detection),
// annotate (shapes on a transparent layer composited at full resolution), adjust (live
// colour preview) and the PDF page organizer. Boxes stay in preview pixels until applied.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);
const make = (tag, props) => Object.assign(document.createElement(tag), props);

const cv = $('canvas');
let ctx = cv.getContext('2d'); // swapped for an offscreen layer when exporting annotations
const RATIOS = [['free', 0], ['1:1', 1], ['4:3', 4 / 3], ['3:2', 3 / 2], ['16:9', 16 / 9], ['9:16', 9 / 16], ['4:5', 4 / 5]];
const COLORS = { blur: 'blur', black: '#000000', white: '#ffffff' };

let st = null; // EditorState from Rust
let img = null;
let k = 1; // source px per preview px
let boxes = []; // crop/redact rects (preview px)
let shapes = []; // annotate: {type, color, size, pts, text, n}
let drag = null;
let aspect = 0;
let style = 'blur';
let pen = { type: 'rect', color: '#f07b1d', size: 4, text: '' };
const NEUTRAL = { brightness: 0, contrast: 1, saturation: 1, gamma: 1, hue: 0, sharpen: 0, clarity: 0, denoise: 0, grain: 0 };
// [key, min, max, step]; gamma, sharpen, clarity, denoise and grain are not shown in the live preview
const SLIDERS = [['brightness', -1, 1, 0.02], ['contrast', 0, 3, 0.02], ['saturation', 0, 3, 0.02], ['hue', -180, 180, 1],
  ['gamma', 0.1, 3, 0.02], ['sharpen', 0, 3, 0.05], ['clarity', 0, 2, 0.05], ['denoise', 0, 20, 0.5], ['grain', 0, 60, 1]];
let adj = { ...NEUTRAL };
let pages = []; // organizer: [{t: thumbnail index, f: file, n: page (1-based), rot: degrees}]
let clips = []; // join: clips in output order
let now = 0; // timeline position (s)
let cut = { start: 0, end: 0 }; // trim range (s)
let frameTimer = 0;
/// Pause before fetching a frame while the slider moves, so scrubbing stays smooth.
const SCRUB_DEBOUNCE_MS = 120;

const adjustCss = () => `brightness(${1 + adj.brightness}) contrast(${adj.contrast}) saturate(${adj.saturation}) hue-rotate(${adj.hue}deg)`;

const lineW = () => Math.max(2, Math.round(cv.width / 320)) * pen.size / 4;

function point(e) {
  const r = cv.getBoundingClientRect();
  const x = ((e.clientX - r.left) * cv.width) / r.width;
  const y = ((e.clientY - r.top) * cv.height) / r.height;
  return [Math.max(0, Math.min(cv.width, x)), Math.max(0, Math.min(cv.height, y))];
}

function norm(x0, y0, x1, y1) {
  if (aspect && st.tool === 'crop') {
    const w = x1 - x0;
    const h = Math.sign(y1 - y0 || 1) * Math.abs(w) / aspect;
    y1 = y0 + h;
  }
  const x = Math.max(0, Math.min(x0, x1));
  const y = Math.max(0, Math.min(y0, y1));
  return { x, y, w: Math.min(cv.width, Math.max(x0, x1)) - x, h: Math.min(cv.height, Math.max(y0, y1)) - y };
}

function arrow(a, b, w, color) {
  const ang = Math.atan2(b[1] - a[1], b[0] - a[0]);
  const head = w * 4;
  ctx.strokeStyle = ctx.fillStyle = color;
  ctx.lineWidth = w;
  ctx.lineCap = 'round';
  ctx.beginPath();
  ctx.moveTo(a[0], a[1]);
  ctx.lineTo(b[0] - Math.cos(ang) * head * 0.8, b[1] - Math.sin(ang) * head * 0.8);
  ctx.stroke();
  ctx.beginPath();
  ctx.moveTo(b[0], b[1]);
  ctx.lineTo(b[0] - head * Math.cos(ang - 0.45), b[1] - head * Math.sin(ang - 0.45));
  ctx.lineTo(b[0] - head * Math.cos(ang + 0.45), b[1] - head * Math.sin(ang + 0.45));
  ctx.closePath();
  ctx.fill();
}

function drawShape(s) {
  const w = s.size;
  ctx.save();
  if (s.type === 'rect') {
    const r = norm(...s.pts[0], ...s.pts[s.pts.length - 1]);
    ctx.strokeStyle = s.color;
    ctx.lineWidth = w;
    ctx.strokeRect(r.x, r.y, r.w, r.h);
  } else if (s.type === 'arrow') {
    arrow(s.pts[0], s.pts[s.pts.length - 1], w, s.color);
  } else if (s.type === 'pen') {
    ctx.strokeStyle = s.color;
    ctx.globalAlpha = 0.45; // highlighter
    ctx.lineWidth = w * 4;
    ctx.lineCap = ctx.lineJoin = 'round';
    ctx.beginPath();
    s.pts.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
    ctx.stroke();
  } else if (s.type === 'text') {
    ctx.fillStyle = s.color;
    ctx.font = `600 ${w * 6}px system-ui, "Noto Sans Arabic", sans-serif`;
    ctx.textBaseline = 'top';
    ctx.fillText(s.text, ...s.pts[0]);
  } else if (s.type === 'number') {
    const [x, y] = s.pts[0];
    const r = w * 4;
    ctx.fillStyle = s.color;
    ctx.beginPath();
    ctx.arc(x, y, r, 0, 2 * Math.PI);
    ctx.fill();
    ctx.fillStyle = '#fff';
    ctx.font = `700 ${r * 1.1}px system-ui, sans-serif`;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(String(s.n), x, y + 1);
  }
  ctx.restore();
}

function draw() {
  if (!img || !st) return;
  if (st.tool === 'trim' || st.tool === 'snapshot') {
    cv.width = img.naturalWidth;
    cv.height = img.naturalHeight;
    ctx.drawImage(img, 0, 0);
    return;
  }
  if (st.tool === 'adjust') {
    ctx.filter = adjustCss();
    ctx.drawImage(img, 0, 0);
    ctx.filter = 'none';
    $('info').textContent = `${st.width} × ${st.height} px`;
    return;
  }
  ctx.drawImage(img, 0, 0);
  const live = drag && drag.box ? [...boxes, drag.box] : boxes;
  if (st.tool === 'crop' && live.length) {
    const b = live[live.length - 1];
    ctx.save();
    ctx.fillStyle = 'rgba(0,0,0,0.55)';
    ctx.beginPath();
    ctx.rect(0, 0, cv.width, cv.height);
    ctx.rect(b.x, b.y, b.w, b.h);
    ctx.fill('evenodd');
    ctx.strokeStyle = '#fff';
    ctx.lineWidth = Math.max(1, cv.width / 600);
    ctx.strokeRect(b.x, b.y, b.w, b.h);
    ctx.restore();
  } else if (st.tool === 'redact') {
    for (const b of live) {
      ctx.save();
      if (style === 'blur') {
        ctx.beginPath();
        ctx.rect(b.x, b.y, b.w, b.h);
        ctx.clip();
        ctx.filter = `blur(${Math.max(6, cv.width / 90)}px)`;
        ctx.drawImage(img, 0, 0);
      } else {
        ctx.fillStyle = COLORS[style];
        ctx.fillRect(b.x, b.y, b.w, b.h);
      }
      ctx.restore();
    }
  } else if (st.tool === 'annotate') {
    shapes.forEach(drawShape);
    if (drag && drag.shape) drawShape(drag.shape);
  }
  const b = st.tool === 'crop' ? live[live.length - 1] : null;
  $('info').textContent = b
    ? `${Math.round(b.w * k)} × ${Math.round(b.h * k)} px`
    : `${st.width} × ${st.height} px${st.paths.length > 1 ? ` · ${st.paths.length} files` : ''}`;
}

function select(options, value, onchange) {
  const s = make('select', { onchange: (e) => onchange(e.target.value) });
  for (const [v, label] of options) s.append(make('option', { value: v, textContent: label, selected: v === value }));
  return s;
}

function controls() {
  const c = $('controls');
  c.replaceChildren();
  if (st.tool === 'crop') {
    c.append(select(RATIOS.map(([n]) => [n, n === 'free' ? window.ZT('free') : n]), 'free', (v) => {
      aspect = RATIOS.find(([n]) => n === v)[1];
    }));
  } else if (st.tool === 'redact') {
    c.append(select(Object.keys(COLORS).map((n) => [n, window.ZT(n)]), style, (v) => { style = v; draw(); }));
    if (st.video) {
      c.append(make('input', { id: 'start', placeholder: window.ZT('from'), size: 6 }));
      c.append(make('input', { id: 'end', placeholder: window.ZT('to'), size: 6 }));
    }
    const lang = make('input', { id: 'lang', value: window.ZLANG === 'ar' ? 'ara+eng' : 'eng', size: 7, title: 'Tesseract' });
    const detectFaces = make('button', {
      textContent: window.ZT('detectFaces'),
      onclick: async () => {
        $('err').textContent = '';
        try {
          const found = await invoke('detect_faces');
          if (!found.length) throw window.ZT('noFaces');
          boxes.push(...found);
          draw();
        } catch (e) {
          $('err').textContent = String(e);
        }
      },
    });
    c.append(detectFaces, lang, make('button', {
      textContent: window.ZT('detectText'),
      onclick: async () => {
        $('err').textContent = '';
        try {
          const found = await invoke('detect_text', { lang: lang.value });
          if (!found.length) throw window.ZT('noText');
          boxes.push(...found);
          draw();
        } catch (e) {
          $('err').textContent = String(e);
        }
      },
    }));
  } else if (st.tool === 'adjust') {
    for (const [key, min, max, step] of SLIDERS) {
      const l = make('label', { className: 'slider', textContent: window.ZT(key) });
      l.append(make('input', { type: 'range', min, max, step, value: adj[key], oninput: (e) => { adj[key] = +e.target.value; draw(); } }));
      c.append(l);
    }
    c.append(make('button', { textContent: window.ZT('reset'), onclick: () => { adj = { ...NEUTRAL }; controls(); draw(); } }));
  } else if (st.tool === 'pages' || st.tool === 'merge-pdf') {
    c.append(make('span', { className: 'hint', textContent: window.ZT('pagesHint') }));
  } else {
    for (const t of ['rect', 'arrow', 'text', 'number', 'pen']) {
      c.append(make('button', {
        textContent: window.ZT(t),
        className: pen.type === t ? 'on' : '',
        onclick: () => { pen.type = t; controls(); },
      }));
    }
    c.append(make('input', { type: 'color', value: pen.color, oninput: (e) => { pen.color = e.target.value; } }));
    c.append(make('input', { type: 'range', min: 1, max: 12, value: pen.size, oninput: (e) => { pen.size = +e.target.value; } }));
    if (pen.type === 'text') {
      c.append(make('input', { placeholder: window.ZT('text'), value: pen.text, oninput: (e) => { pen.text = e.target.value; } }));
    }
  }
}

cv.addEventListener('pointerdown', (e) => {
  if (!st || ['adjust', 'pages', 'merge-pdf', 'trim', 'snapshot', 'join'].includes(st.tool)) return;
  cv.setPointerCapture(e.pointerId);
  const p = point(e);
  if (st.tool === 'annotate') {
    if (pen.type === 'text') {
      if (pen.text.trim()) shapes.push({ type: 'text', color: pen.color, size: lineW(), pts: [p], text: pen.text });
      return draw();
    }
    if (pen.type === 'number') {
      const n = shapes.filter((s) => s.type === 'number').length + 1;
      shapes.push({ type: 'number', color: pen.color, size: lineW(), pts: [p], n });
      return draw();
    }
    drag = { shape: { type: pen.type, color: pen.color, size: lineW(), pts: [p] } };
  } else {
    drag = { start: p, box: null };
  }
});

cv.addEventListener('pointermove', (e) => {
  if (!drag) return;
  const p = point(e);
  if (drag.shape) drag.shape.pts.push(p);
  else drag.box = norm(...drag.start, ...p);
  draw();
});

cv.addEventListener('pointerup', () => {
  if (!drag) return;
  if (drag.shape && drag.shape.pts.length > 1) shapes.push(drag.shape);
  if (drag.box && drag.box.w > 2 && drag.box.h > 2) {
    if (st.tool === 'crop') boxes = [drag.box];
    else boxes.push(drag.box);
  }
  drag = null;
  draw();
});

const src = (b) => [b.x, b.y, b.w, b.h].map((v) => Math.round(v * k)).join(',');

async function apply() {
  if (!st) return;
  $('err').textContent = '';
  try {
    if (st.tool === 'annotate') {
      if (!shapes.length) throw window.ZT('drawSomething');
      // Only the annotation layer goes to Rust; it is scaled onto the full-size original.
      const layer = document.createElement('canvas');
      layer.width = cv.width;
      layer.height = cv.height;
      const screen = ctx;
      ctx = layer.getContext('2d');
      shapes.forEach(drawShape);
      ctx = screen;
      await invoke('save_annotation', { src: st.paths[0], data: layer.toDataURL('image/png'), width: st.width, height: st.height });
    } else if (st.tool === 'trim') {
      const opts = { start: cut.start.toFixed(3) };
      if (cut.end < st.duration - 0.001) opts.end = cut.end.toFixed(3);
      await invoke('run_job', { paths: st.paths, kind: 'tool', id: 'trim', opts });
    } else if (st.tool === 'snapshot') {
      await invoke('run_job', { paths: st.paths, kind: 'tool', id: 'snapshot', opts: { at: now.toFixed(3) } });
    } else if (st.tool === 'join') {
      if (clips.length < 2) throw window.ZT('twoClips');
      await invoke('run_job', { paths: clips.map((c) => c.path), kind: 'tool', id: 'join', opts: {} });
    } else if (st.tool === 'adjust') {
      const opts = Object.fromEntries(Object.entries(adj).map(([key, v]) => [key, String(v)]));
      await invoke('run_job', { paths: st.paths, kind: 'tool', id: 'adjust', opts });
    } else if (st.tool === 'pages' || st.tool === 'merge-pdf') {
      if (!pages.length) throw window.ZT('noPages');
      const rotate = pages.map((p, i) => (p.rot ? `${i + 1}:${p.rot}` : '')).filter(Boolean).join(',');
      if (st.paths.length > 1) {
        const items = pages.map((p) => `${p.f}:${p.n}`).join(',');
        await invoke('run_job', { paths: st.paths, kind: 'tool', id: 'arrange', opts: { items, rotate } });
      } else {
        const spec = pages.map((p) => p.n).join(',');
        await invoke('run_job', { paths: st.paths, kind: 'tool', id: 'pages', opts: { spec, rotate } });
      }
    } else {
      if (!boxes.length) throw window.ZT('drawBox');
      const opts = st.tool === 'crop'
        ? { box: src(boxes[0]) }
        : { boxes: boxes.map(src).join(';'), style: COLORS[style], start: $('start')?.value || '', end: $('end')?.value || '' };
      await invoke('run_job', { paths: st.paths, kind: 'tool', id: st.tool, opts });
    }
    close();
  } catch (e) {
    $('err').textContent = String(e);
  }
}

function close() {
  clearTimeout(frameTimer);
  img = null;
  st = null;
  ctx.clearRect(0, 0, cv.width, cv.height);
  invoke('close_editor');
}

function undo() {
  if (st && st.tool === 'annotate') shapes.pop();
  else boxes.pop();
  draw();
}

function renderPages() {
  const grid = $('pages');
  grid.replaceChildren(...pages.map((p, i) => {
    const card = make('div', { className: 'page' });
    const thumb = make('img', { src: st.pages[p.t], alt: `${p.n}` });
    thumb.style.transform = `rotate(${p.rot}deg)`;
    const move = (to) => { const [it] = pages.splice(i, 1); pages.splice(to, 0, it); renderPages(); };
    const btn = (text, title, fn) => make('button', { textContent: text, title: window.ZT(title), onclick: fn });
    const tools = make('div', { className: 'page-tools' });
    tools.append(
      btn('◀', 'moveLeft', () => i > 0 && move(i - 1)),
      btn('⟳', 'rotate', () => { p.rot = (p.rot + 90) % 360; renderPages(); }),
      btn('⧉', 'duplicate', () => { pages.splice(i + 1, 0, { ...p }); renderPages(); }),
      btn('✕', 'remove', () => { pages.splice(i, 1); renderPages(); }),
      btn('▶', 'moveRight', () => i < pages.length - 1 && move(i + 1)),
    );
    const origin = st.paths.length > 1 ? `${st.paths[p.f - 1].split(/[\\/]/).pop()} · ` : '';
    card.append(thumb, make('div', { className: 'page-n', textContent: `${i + 1} · ${origin}p${p.n}` }), tools);
    return card;
  }));
  $('info').textContent = `${pages.length} ${window.ZT('pagesCount')}`;
}

// ---------- timeline (trim, save frames) ----------

const clock = (t) => {
  const m = Math.floor(t / 60);
  const s = (t - m * 60).toFixed(3).padStart(6, '0');
  return `${m}:${s}`;
};

function paintTimeline() {
  const d = st.duration || 1;
  $('scrub').value = String(now);
  $('clock').textContent = clock(now);
  $('head').style.left = `${(now / d) * 100}%`;
  const r = $('range');
  r.style.left = `${(cut.start / d) * 100}%`;
  r.style.width = `${((cut.end - cut.start) / d) * 100}%`;
  r.hidden = st.tool !== 'trim';
  $('span').textContent = st.tool === 'trim' ? `${clock(cut.start)} → ${clock(cut.end)} (${(cut.end - cut.start).toFixed(2)} s)` : '';
}

function seek(t) {
  now = Math.min(Math.max(0, t), st.duration);
  paintTimeline();
  if (!st.video) return;
  clearTimeout(frameTimer);
  frameTimer = setTimeout(async () => {
    const want = st;
    try {
      const url = await invoke('frame_at', { t: now });
      if (st !== want) return; // closed or reopened meanwhile
      const next = new Image();
      next.onload = () => { if (st === want) { img = next; draw(); } };
      next.src = url;
    } catch (e) {
      $('err').textContent = String(e);
    }
  }, SCRUB_DEBOUNCE_MS);
}

function openTimeline() {
  $('timeline').hidden = false;
  cv.hidden = !st.video;
  $('wave').hidden = !st.waveform;
  if (st.waveform) $('wave').src = st.waveform;
  const scrub = $('scrub');
  scrub.max = String(st.duration);
  scrub.step = String(1 / (st.fps || 25));
  cut = { start: 0, end: st.duration };
  now = 0;
  const trim = st.tool === 'trim';
  $('setStart').hidden = !trim;
  $('setEnd').hidden = !trim;
  $('saveFrame').hidden = trim || !st.video;
  paintTimeline();
}

const frame = () => 1 / (st.fps || 25);
$('scrub').oninput = (e) => seek(+e.target.value);
$('back1f').onclick = () => seek(now - frame());
$('fwd1f').onclick = () => seek(now + frame());
$('back1s').onclick = () => seek(now - 1);
$('fwd1s').onclick = () => seek(now + 1);
$('setStart').onclick = () => { cut.start = Math.min(now, cut.end - frame()); paintTimeline(); };
$('setEnd').onclick = () => { cut.end = Math.max(now, cut.start + frame()); paintTimeline(); };
$('saveFrame').onclick = async () => {
  $('err').textContent = '';
  try {
    await invoke('run_job', { paths: st.paths, kind: 'tool', id: 'snapshot', opts: { at: now.toFixed(3) } });
  } catch (e) {
    $('err').textContent = String(e);
  }
};

// ---------- join: clip order ----------

function renderClips() {
  const list = $('clips');
  list.replaceChildren(...clips.map((c, i) => {
    const card = make('div', { className: 'clip' });
    const thumb = c.thumb ? make('img', { src: c.thumb, alt: '' }) : make('div', { className: 'clip-audio', textContent: '♪' });
    const move = (to) => { const [it] = clips.splice(i, 1); clips.splice(to, 0, it); renderClips(); };
    const btn = (text, title, fn) => make('button', { textContent: text, title: window.ZT(title), onclick: fn });
    const tools = make('div', { className: 'page-tools' });
    tools.append(
      btn('◀', 'moveLeft', () => i > 0 && move(i - 1)),
      btn('✕', 'remove', () => { clips.splice(i, 1); renderClips(); }),
      btn('▶', 'moveRight', () => i < clips.length - 1 && move(i + 1)),
    );
    card.append(thumb, make('div', { className: 'page-n', textContent: `${i + 1} · ${c.name}` }),
      make('div', { className: 'page-n', textContent: clock(c.duration) }), tools);
    return card;
  }));
  const total = clips.reduce((a, c) => a + c.duration, 0);
  $('info').textContent = `${clips.length} · ${clock(total)}`;
}

async function open() {
  $('err').textContent = '';
  try {
    st = await invoke('editor_state');
  } catch {
    return;
  }
  boxes = [];
  shapes = [];
  aspect = 0;
  adj = { ...NEUTRAL };
  $('title').textContent = window.ZT(st.tool);
  $('apply').textContent = window.ZT(st.tool === 'annotate' ? 'save' : 'apply');
  controls();
  const organizer = st.tool === 'pages' || st.tool === 'merge-pdf';
  const timeline = st.tool === 'trim' || st.tool === 'snapshot';
  const joining = st.tool === 'join';
  $('pages').hidden = !organizer;
  $('clips').hidden = !joining;
  $('timeline').hidden = !timeline;
  cv.hidden = organizer || joining;
  $('undo').hidden = organizer || joining || timeline || st.tool === 'adjust';
  if (joining) {
    clips = st.clips.slice();
    img = null;
    return renderClips();
  }
  if (timeline) openTimeline();
  if (organizer) {
    pages = st.page_refs.map(([f, n], t) => ({ t, f, n, rot: 0 }));
    img = null;
    return renderPages();
  }
  if (!st.preview) {
    img = null;
    return;
  }
  img = new Image();
  img.onload = () => {
    cv.width = img.naturalWidth;
    cv.height = img.naturalHeight;
    k = st.width / img.naturalWidth;
    draw();
  };
  img.src = st.preview;
}

$('apply').onclick = apply;
$('cancel').onclick = close;
$('undo').onclick = undo;
document.addEventListener('keydown', (e) => {
  if (e.target.tagName === 'INPUT') return;
  if (e.key === 'Escape') close();
  if (e.key === 'Enter') apply();
  if (st && (st.tool === 'trim' || st.tool === 'snapshot')) {
    if (e.key === 'ArrowLeft') seek(now - (e.shiftKey ? 1 : frame()));
    if (e.key === 'ArrowRight') seek(now + (e.shiftKey ? 1 : frame()));
    if (e.key === 'i' && st.tool === 'trim') $('setStart').onclick();
    if (e.key === 'o' && st.tool === 'trim') $('setEnd').onclick();
  }
  if ((e.ctrlKey || e.metaKey) && e.key === 'z') undo();
});
listen('editor-open', open);
open();
