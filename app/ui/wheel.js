// Radial menu shown under the pointer while files are dragged. Everything is driven by native
// drag events (enter/over/drop/leave), so no click is needed: hover a segment, release.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWebview } = window.__TAURI__.webview;

const C = 210;
const R_OUT = 200;
const R_IN = 66;
const MODES = ['convert', 'tools', 'actions'];
const ICON = { convert: '⇄', tools: '✎', actions: '↗' };
const DWELL_MS = 400; // hover time on a mode chip before it switches
const LEAVE_MS = 900; // grace after the drag leaves before hiding
const NS = 'http://www.w3.org/2000/svg';

const svg = document.getElementById('wheel');
const form = document.getElementById('params');
let paths = [];
let desc = null;
let mode = 'convert';
let hover = -1;
let chip = -1;
let chipTimer = 0;
let hideTimer = 0;
let formOpen = false;
let centre = false; // pointer over the hub: dropping there floats the files on the orb

const items = () => (desc ? desc[mode] : []);
const polar = (r, a) => [C + r * Math.cos(a), C + r * Math.sin(a)];
const chipPos = (i) => [C + (i - 1) * 36, C - 30];
const short = (s, n) => (s.length > n ? s.slice(0, n - 1) + '…' : s);

function node(tag, attrs, text) {
  const e = document.createElementNS(NS, tag);
  for (const k in attrs) e.setAttribute(k, attrs[k]);
  if (text != null) e.textContent = text;
  return e;
}

function arc(i, n) {
  if (n === 1) {
    const ring = (r, sweep) => `M ${C} ${C - r} A ${r} ${r} 0 1 ${sweep} ${C} ${C + r} A ${r} ${r} 0 1 ${sweep} ${C} ${C - r} Z`;
    return ring(R_OUT, 1) + ring(R_IN, 0);
  }
  const gap = 0.01;
  const a0 = -Math.PI / 2 + (i * 2 * Math.PI) / n + gap;
  const a1 = -Math.PI / 2 + ((i + 1) * 2 * Math.PI) / n - gap;
  const large = a1 - a0 > Math.PI ? 1 : 0;
  const [x0, y0] = polar(R_OUT, a0);
  const [x1, y1] = polar(R_OUT, a1);
  const [x2, y2] = polar(R_IN, a1);
  const [x3, y3] = polar(R_IN, a0);
  return `M${x0} ${y0} A${R_OUT} ${R_OUT} 0 ${large} 1 ${x1} ${y1} L${x2} ${y2} A${R_IN} ${R_IN} 0 ${large} 0 ${x3} ${y3} Z`;
}

function render() {
  svg.replaceChildren();
  const list = items();
  const n = list.length;
  list.forEach((it, i) => {
    const hot = i === hover;
    svg.append(node('path', { d: arc(i, n), class: hot ? 'seg hot' : 'seg', 'fill-rule': 'evenodd', role: 'menuitem', 'aria-label': window.ZL(it) }));
    const mid = -Math.PI / 2 + ((i + 0.5) * 2 * Math.PI) / n;
    const [x, y] = n === 1 ? [C, C - (R_IN + R_OUT) / 2] : polar((R_IN + R_OUT) / 2 + 8, mid);
    svg.append(node('text', { x, y, class: hot ? 'lbl hot' : 'lbl', 'text-anchor': 'middle', 'dominant-baseline': 'middle' }, short(window.ZL(it), n > 10 ? 9 : 13)));
  });
  svg.append(node('circle', { cx: C, cy: C, r: R_IN - 4, class: 'hub' }));
  MODES.forEach((m, i) => {
    const [x, y] = chipPos(i);
    const cls = m === mode ? 'chip on' : i === chip ? 'chip near' : 'chip';
    svg.append(node('circle', { cx: x, cy: y, r: 14, class: cls }));
    svg.append(node('text', { x, y, class: 'chipt', 'text-anchor': 'middle', 'dominant-baseline': 'central' }, ICON[m]));
  });
  const caption = hover >= 0 && list[hover] ? window.ZL(list[hover]) : centre ? window.ZT('float') : n ? window.ZT(mode) : window.ZT('nothing');
  svg.append(node('text', { x: C, y: C + 10, class: 'cap', 'text-anchor': 'middle' }, short(caption, 18)));
  svg.append(node('text', { x: C, y: C + 36, class: 'x', 'text-anchor': 'middle' }, '◎'));
}

function hit(pos) {
  const r = window.devicePixelRatio || 1; // drag positions are physical pixels
  const x = pos.x / r;
  const y = pos.y / r;
  const d = Math.hypot(x - C, y - C);
  if (d < R_IN) {
    return { chip: MODES.findIndex((_, k) => Math.hypot(x - chipPos(k)[0], y - chipPos(k)[1]) < 16) };
  }
  const n = items().length;
  if (d > R_OUT || !n) return {};
  let a = Math.atan2(y - C, x - C) + Math.PI / 2;
  if (a < 0) a += 2 * Math.PI;
  return { seg: Math.floor(a / ((2 * Math.PI) / n)) % n };
}

function setMode(m) {
  mode = m;
  hover = -1;
  render();
}

function over(pos) {
  const h = hit(pos);
  const nh = h.seg ?? -1;
  const nc = h.chip ?? -1;
  const nowCentre = h.chip === -1;
  if (nowCentre !== centre) {
    centre = nowCentre;
    render();
  }
  if (nc !== chip) {
    clearTimeout(chipTimer);
    chip = nc;
    if (nc >= 0 && MODES[nc] !== mode) chipTimer = setTimeout(() => setMode(MODES[nc]), DWELL_MS);
    render();
  }
  if (nh !== hover) {
    hover = nh;
    render();
  }
}

async function load(p) {
  if (desc && p.join('\n') === paths.join('\n')) return;
  paths = p;
  desc = await invoke('describe', { paths: p });
  if (!items().length) mode = MODES.find((m) => desc[m].length) || 'convert';
  render();
}

function closeForm() {
  formOpen = false;
  form.hidden = true;
}

function choose(it) {
  if (it.editor) return invoke('open_editor', { paths, tool: it.id });
  if (!it.params.length) return invoke('run_job', { paths, kind: it.kind, id: it.id, opts: it.preset || {} });
  formOpen = true;
  invoke('keep_wheel');
  window.ZForm(form, it, async (opts) => {
    await invoke('run_job', { paths, kind: it.kind, id: it.id, opts });
    closeForm();
  }, () => {
    closeForm();
    invoke('hide_wheel');
  });
}

async function drop(p, pos) {
  clearTimeout(chipTimer);
  await load(p);
  const h = hit(pos);
  const it = h.seg != null ? items()[h.seg] : null;
  if (it) choose(it);
  else if (h.chip === -1) invoke('float_add', { paths: p });
  else invoke('hide_wheel');
}

function reset() {
  closeForm();
  paths = [];
  desc = null;
  hover = -1;
  chip = -1;
  centre = false;
  render();
}

getCurrentWebview().onDragDropEvent(async (e) => {
  const p = e.payload;
  if (formOpen) return;
  if (p.type === 'enter') {
    clearTimeout(hideTimer);
    await load(p.paths);
    over(p.position);
  } else if (p.type === 'over') {
    over(p.position);
  } else if (p.type === 'drop') {
    await drop(p.paths, p.position);
  } else {
    hover = -1;
    render();
    hideTimer = setTimeout(() => invoke('hide_wheel'), LEAVE_MS);
  }
});

listen('wheel-mode', (e) => { if (!formOpen) setMode(e.payload); });
listen('wheel-hidden', reset);
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') invoke('hide_wheel');
});
render();
