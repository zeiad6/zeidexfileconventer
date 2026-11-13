// Three-ring radial menu shown under the pointer while files are dragged.
//
//   outer   modes      Convert / Tools / Actions (dwell or scroll to morph the ring)
//   middle  items      formats, tools or actions for the current mode
//   inner   children   quality presets or a tool's default options (the nested sub-ring)
//   hub     cancel     dropping here abandons the job
//
// Everything is driven by native drag events (enter/over/drop/leave) so a drag needs no click:
// hover a segment and release the button. Keyboard/click mode (Ctrl+Shift+Space) adds arrows,
// Tab, Enter, Escape and the mouse wheel.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWebview } = window.__TAURI__.webview;

const C = 240;
const MODES_R = [186, 228];
const ITEMS_R = [122, 180];
const CHILD_R = [56, 116];
const HUB_R = 52;
const MODES = ['convert', 'tools', 'actions'];
const ICON = { convert: '⇄', tools: '✎', actions: '↗' };
const DWELL_MS = 380; // hover time on a mode segment before it switches
const OPEN_MS = 260; // hover time on an item before its sub-ring opens
const LEAVE_MS = 900; // grace after the drag leaves before hiding
const NS = 'http://www.w3.org/2000/svg';

const svg = document.getElementById('wheel');
const form = document.getElementById('params');
let paths = [];
let desc = null;
let mode = 'convert';
let hover = -1; // item under the pointer
let sub = -1; // child under the pointer
let open = -1; // item whose sub-ring is open
let chip = -1; // mode segment under the pointer
let chipTimer = 0;
let openTimer = 0;
let hideTimer = 0;
let formOpen = false;
let centre = false;

const items = () => (desc ? desc[mode] : []);
const children = () => (open >= 0 && items()[open] ? items()[open].children || [] : []);
const polar = (r, a) => [C + r * Math.cos(a), C + r * Math.sin(a)];
const mid = (i, n) => -Math.PI / 2 + ((i + 0.5) * 2 * Math.PI) / n;
const short = (s, n) => (s.length > n ? s.slice(0, n - 1) + '…' : s);

function node(tag, attrs, text) {
  const e = document.createElementNS(NS, tag);
  for (const k in attrs) e.setAttribute(k, attrs[k]);
  if (text != null) e.textContent = text;
  return e;
}

/// Ring segment between `r0` and `r1`, as a donut slice.
function arc(i, n, r0, r1) {
  if (n === 1) {
    const ring = (r, sweep) => `M ${C} ${C - r} A ${r} ${r} 0 1 ${sweep} ${C} ${C + r} A ${r} ${r} 0 1 ${sweep} ${C} ${C - r} Z`;
    return ring(r1, 1) + ring(r0, 0);
  }
  const gap = n > 16 ? 0.006 : 0.012;
  const a0 = -Math.PI / 2 + (i * 2 * Math.PI) / n + gap;
  const a1 = -Math.PI / 2 + ((i + 1) * 2 * Math.PI) / n - gap;
  const large = a1 - a0 > Math.PI ? 1 : 0;
  const [x0, y0] = polar(r1, a0);
  const [x1, y1] = polar(r1, a1);
  const [x2, y2] = polar(r0, a1);
  const [x3, y3] = polar(r0, a0);
  return `M${x0} ${y0} A${r1} ${r1} 0 ${large} 1 ${x1} ${y1} L${x2} ${y2} A${r0} ${r0} 0 ${large} 0 ${x3} ${y3} Z`;
}

function ring(name, itemsList, r0, r1, hot, cls) {
  const g = node('g', { class: `ring ${name}` });
  itemsList.forEach((it, i) => {
    const d = arc(i, itemsList.length, r0, r1);
    g.append(node('path', { d, class: `${cls}${i === hot ? ' hot' : ''}`, 'fill-rule': 'evenodd' }));
  });
  return g;
}

function label(text, r, angle, hot) {
  const [x, y] = polar(r, angle);
  return node('text', { x, y, class: hot ? 'lbl hot' : 'lbl', 'text-anchor': 'middle', 'dominant-baseline': 'middle' }, text);
}

function render() {
  svg.replaceChildren();
  const list = items();
  const kids = children();

  // Outer ring: the three modes. The current one is filled, a dwelled one is outlined.
  const gModes = ring('modes', MODES.map((m) => m), MODES_R[0], MODES_R[1], chip, 'seg mode');
  MODES.forEach((m, i) => {
    gModes.append(label(`${ICON[m]} ${window.ZT(m)}`, (MODES_R[0] + MODES_R[1]) / 2 - 4, mid(i, 3), m === mode));
  });
  svg.append(gModes);

  // Middle ring: items. Dimmed while a sub-ring is open, so the eye follows the nested ring.
  const gItems = ring('items', list, ITEMS_R[0], ITEMS_R[1], hover, 'seg');
  gItems.classList.toggle('dim', open >= 0);
  list.forEach((it, i) => {
    const many = list.length;
    const midR = (ITEMS_R[0] + ITEMS_R[1]) / 2;
    const txt = short(window.ZL(it), many > 8 ? 10 : many > 5 ? 13 : 16);
    gItems.append(label(txt, midR, mid(i, many), i === hover));
    if ((it.children || []).length) {
      const [x, y] = polar(ITEMS_R[0] + 10, mid(i, many));
      gItems.append(node('circle', { cx: x, cy: y, r: 3, class: 'dot' }));
    }
  });
  svg.append(gItems);

  // Inner ring: children of the hovered item (the nested sub-ring), or a hint.
  if (kids.length) {
    const gKids = ring('children', kids, CHILD_R[0], CHILD_R[1], sub, 'seg sub');
    kids.forEach((k, i) => {
      const txt = short(window.ZL(k), kids.length > 6 ? 9 : 12);
      gKids.append(label(txt, (CHILD_R[0] + CHILD_R[1]) / 2, mid(i, kids.length), i === sub));
    });
    svg.append(gKids);
  } else {
    svg.append(node('circle', { cx: C, cy: C, r: (CHILD_R[0] + CHILD_R[1]) / 2, class: 'hintring' }));
  }

  // Hub: cancel, plus the current hovered choice as the caption.
  svg.append(node('circle', { cx: C, cy: C, r: HUB_R, class: centre ? 'hub cancel' : 'hub' }));
  svg.append(node('text', { x: C, y: C - 8, class: 'cap', 'text-anchor': 'middle' }, short(caption(), 20)));
  svg.append(node('text', { x: C, y: C + 16, class: 'x', 'text-anchor': 'middle' }, centre ? '✕' : '◎'));
}

function caption() {
  const list = items();
  if (centre) return window.ZT('cancel');
  if (sub >= 0 && children()[sub]) return window.ZL(children()[sub]);
  if (hover >= 0 && list[hover]) return window.ZL(list[hover]);
  if (desc && desc.mixed) return window.ZT('mixed');
  return window.ZT(mode);
}

/// Which part of the wheel a physical pointer position lands on.
function hit(pos) {
  const r = window.devicePixelRatio || 1; // drag positions are physical pixels
  const x = pos.x / r;
  const y = pos.y / r;
  const d = Math.hypot(x - C, y - C);
  let a = Math.atan2(y - C, x - C) + Math.PI / 2;
  if (a < 0) a += 2 * Math.PI;
  const seg = (n) => Math.floor(a / ((2 * Math.PI) / n)) % n;
  if (d <= HUB_R) return { centre: true };
  if (d >= CHILD_R[0] && d <= CHILD_R[1] && children().length) return { sub: seg(children().length) };
  if (d >= ITEMS_R[0] && d <= ITEMS_R[1] && items().length) return { item: seg(items().length) };
  if (d >= MODES_R[0] && d <= MODES_R[1]) return { chip: seg(3) };
  return {};
}

function setMode(m) {
  if (m === mode) return;
  mode = m;
  hover = -1;
  sub = -1;
  open = -1;
  clearTimeout(openTimer);
  morph();
}

/// A short scale/fade on the rings makes the change read as one wheel morphing, not a redraw.
function morph() {
  svg.classList.remove('morph');
  void svg.getBoundingClientRect();
  svg.classList.add('morph');
  render();
  setTimeout(() => svg.classList.remove('morph'), 260);
}

function over(pos) {
  const h = hit(pos);
  const nc = h.chip ?? -1;
  const nowCentre = h.centre === true;

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

  const ni = h.item ?? -1;
  // The child band keeps the parent item selected; only the item band (or leaving it) changes
  // which item owns the sub-ring.
  if (h.item != null || h.centre || h.chip != null) {
    if (ni !== hover) {
      hover = ni;
      sub = -1;
      if (open >= 0 && open !== ni) {
        open = -1;
        clearTimeout(openTimer);
      }
      // Dwelling on an item with children opens its sub-ring; moving away closes it.
      if (ni >= 0 && (items()[ni].children || []).length) {
        clearTimeout(openTimer);
        openTimer = setTimeout(() => {
          if (hover === ni) {
            open = ni;
            render();
          }
        }, OPEN_MS);
      }
      render();
    }
  }

  const ns = h.sub ?? -1;
  if (ns !== sub) {
    sub = ns;
    render();
  }
}

async function load(p) {
  if (desc && p.join('\n') === paths.join('\n')) return;
  paths = p;
  desc = await invoke('describe', { paths: p });
  if (!items().length) mode = MODES.find((m) => desc[m].length) || 'convert';
  hover = -1;
  sub = -1;
  open = -1;
  render();
}

function closeForm() {
  formOpen = false;
  form.hidden = true;
}

function choose(it) {
  open = -1;
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

/// Applies a nested-ring choice: quality presets run immediately, "Custom…" opens the form.
function chooseSub(it, child) {
  if (child.custom) {
    open = -1;
    return choose(it);
  }
  open = -1;
  const opts = { ...(it.preset || {}), [child.key]: child.value };
  return invoke('run_job', { paths, kind: it.kind, id: it.id, opts });
}

async function drop(p, pos) {
  clearTimeout(chipTimer);
  clearTimeout(openTimer);
  await load(p);
  const h = hit(pos);
  if (h.centre) return invoke('hide_wheel');
  if (h.sub != null) {
    if (!children().length || open < 0) return; // the sub-ring has not opened yet
    return chooseSub(items()[open], children()[h.sub]);
  }
  if (h.item != null && items()[h.item]) return choose(items()[h.item]);
  if (h.chip != null) return setMode(MODES[h.chip]);
  return invoke('hide_wheel');
}

function reset() {
  clickMode = false;
  closeForm();
  paths = [];
  desc = null;
  hover = -1;
  sub = -1;
  open = -1;
  chip = -1;
  centre = false;
  clearTimeout(openTimer);
  clearTimeout(chipTimer);
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
    sub = -1;
    render();
    hideTimer = setTimeout(() => invoke('hide_wheel'), LEAVE_MS);
  }
});

// Click/keyboard mode (Ctrl+Shift+Space on a selection): no drag, so mouse moves, clicks, the
// mouse wheel and the keys drive the wheel.
let clickMode = false;
const toPhysical = (e) => ({ x: e.clientX * (window.devicePixelRatio || 1), y: e.clientY * (window.devicePixelRatio || 1) });

listen('wheel-open', async (e) => {
  clickMode = true;
  closeForm();
  desc = null;
  await load(e.payload);
  hover = items().length ? 0 : -1;
  render();
});

document.addEventListener('mousemove', (e) => {
  if (clickMode && !formOpen) over(toPhysical(e));
});
document.addEventListener('click', (e) => {
  if (!clickMode || formOpen || e.target.closest?.('#params')) return;
  drop(paths, toPhysical(e));
});
window.addEventListener('blur', () => {
  if (clickMode && !formOpen) invoke('hide_wheel');
});

// The wheel follows the mode the global hook reports (Shift / Ctrl / Alt, or remapped keys).
listen('wheel-mode', (e) => {
  if (!formOpen) setMode(e.payload);
});
listen('wheel-hidden', reset);

// Scrolling morphs the rings: forward to the next mode that has choices, backward to the
// previous one. During a drag Windows sends the wheel to the focused window, so this is the
// click/keyboard path; the modifier keys cover the drag path.
document.addEventListener('wheel', (e) => {
  if (formOpen || !desc) return;
  const filled = MODES.filter((m) => desc[m].length);
  if (filled.length < 2) return;
  e.preventDefault();
  const at = filled.indexOf(mode);
  const next = e.deltaY > 0 ? (at + 1) % filled.length : (at - 1 + filled.length) % filled.length;
  setMode(filled[next]);
}, { passive: false });

document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    if (open >= 0) {
      open = -1;
      sub = -1;
      render();
      return;
    }
    return invoke('hide_wheel');
  }
  if (formOpen || !desc) return;
  const n = items().length;
  const k = Math.max(children().length, 1);
  if (e.key === 'ArrowDown' || e.key === 'ArrowRight') {
    if (open >= 0) sub = (sub + 1 + k) % k;
    else if (n) hover = (hover + 1 + n) % n;
  } else if (e.key === 'ArrowUp' || e.key === 'ArrowLeft') {
    if (open >= 0) sub = (sub - 1 + k) % k;
    else if (n) hover = (hover - 1 + n) % n;
  } else if (e.key === 'Tab') {
    e.preventDefault?.();
    const next = MODES.slice(MODES.indexOf(mode) + 1).concat(MODES).find((m) => desc[m].length);
    setMode(next || mode);
    hover = items().length ? 0 : -1;
  } else if ((e.key === 'Enter' || e.key === ' ') && hover >= 0) {
    const it = items()[hover];
    if (open >= 0 && children()[sub]) return chooseSub(it, children()[sub]);
    if ((it.children || []).length) {
      open = hover;
      sub = 0;
      render();
      return;
    }
    return choose(it);
  } else {
    return;
  }
  render();
});

render();
