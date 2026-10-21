// Visual editor for the tools that need a region: crop (one box), redact (many boxes,
// optional time range for video) and annotate (shapes drawn onto the image, saved as PNG).
// Boxes are kept in preview pixels and scaled to source pixels only when applied.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);
const make = (tag, props) => Object.assign(document.createElement(tag), props);

const cv = $('canvas');
const ctx = cv.getContext('2d');
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
  if (!img) return;
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
  if (!st) return;
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
  $('err').textContent = '';
  try {
    if (st.tool === 'annotate') {
      draw();
      await invoke('save_annotation', { src: st.paths[0], data: cv.toDataURL('image/png') });
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
  $('title').textContent = window.ZT(st.tool);
  $('apply').textContent = window.ZT(st.tool === 'annotate' ? 'save' : 'apply');
  controls();
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
  if ((e.ctrlKey || e.metaKey) && e.key === 'z') undo();
});
listen('editor-open', open);
open();
