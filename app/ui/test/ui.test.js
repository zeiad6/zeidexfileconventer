// Runs the real UI scripts against a tiny fake DOM and a mocked window.__TAURI__.
// Zero dependencies: `node app/ui/test/ui.test.js`. Catches runtime errors and checks the
// drag/drop flows (wheel), the editor's box maths, settings validation and the jobs list.
'use strict';
const fs = require('fs');
const path = require('path');
const vm = require('vm');

const UI = path.join(__dirname, '..');
const fails = [];
const ok = (cond, msg) => {
  console.log(`${cond ? 'ok  ' : 'FAIL'}  ${msg}`);
  if (!cond) fails.push(msg);
};
const tick = (ms = 0) => new Promise((r) => setTimeout(r, ms));

// ---------- fake DOM ----------
class El {
  constructor(tag) {
    this.tagName = tag.toUpperCase();
    this.children = [];
    this.attrs = {};
    this.listeners = {};
    this.style = {};
    this.dataset = {};
    this.hidden = false;
    this.textContent = '';
    this.value = '';
    this.className = '';
    const self = this;
    this.classList = {
      add: (c) => { self.className = `${self.className} ${c}`.trim(); },
      toggle: () => {},
      contains: (c) => self.className.split(' ').includes(c),
    };
  }
  setAttribute(k, v) { this.attrs[k] = String(v); }
  removeAttribute(k) { delete this.attrs[k]; }
  getAttribute(k) { return this.attrs[k]; }
  append(...c) {
    const kids = c.map((x) => (typeof x === 'string' ? Object.assign(new El('#text'), { textContent: x }) : x));
    kids.forEach((k) => { k.parent = this; });
    this.children.push(...kids);
  }
  prepend(...c) { c.forEach((k) => { k.parent = this; }); this.children.unshift(...c); }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter((k) => k !== this); }
  replaceChildren(...c) { this.children = []; this.append(...c); }
  addEventListener(t, f) { (this.listeners[t] ||= []).push(f); }
  fire(t, e = {}) {
    (this.listeners[t] || []).forEach((f) => f(e));
    if (this[`on${t}`]) return this[`on${t}`]({ preventDefault() {}, target: this, ...e });
  }
  all() { return this.children.flatMap((c) => [c, ...c.all()]); }
  matches(sel) {
    if (sel.startsWith('.')) return this.className.split(' ').includes(sel.slice(1));
    const m = sel.match(/^(\w+)(?:\[(\w+)=([^\]]+)\])?$/);
    return m && this.tagName === m[1].toUpperCase() && (!m[2] || this[m[2]] === m[3] || this.attrs[m[2]] === m[3]);
  }
  querySelectorAll(sel) {
    const parts = sel.trim().split(/\s+/);
    let scope = [this];
    for (const p of parts) scope = scope.flatMap((s) => s.all().filter((e) => e.matches(p)));
    return scope;
  }
  querySelector(sel) { return this.querySelectorAll(sel)[0] || null; }
  focus() {}
  setPointerCapture() {}
  getBoundingClientRect() { return { left: 0, top: 0, width: this.width, height: this.height }; }
  getContext() { return new Proxy({}, { get: (t, k) => (k in t ? t[k] : () => {}), set: (t, k, v) => { t[k] = v; return true; } }); }
  toDataURL() { return `data:image/png;base64,iVBORw0KGgo${'A'.repeat(20000)}`; }
}

function page(html, query = '') {
  const listeners = {};
  let dropHandler = null;
  const calls = [];
  const byId = {};
  const src = fs.readFileSync(path.join(UI, html), 'utf8');
  for (const m of src.matchAll(/<(\w+)[^>]*\bid="([^"]+)"/g)) {
    byId[m[2]] = new El(m[1]);
    if (/\bhidden\b/.test(m[0])) byId[m[2]].hidden = true;
  }
  const choice = (kind, id, label, params = [], editor = false) => ({ kind, id, label, label_ar: label, params, editor });
  const resp = {
    describe: {
      category: 'video',
      convert: ['mp4', 'mov', 'mkv', 'webm', 'avi', 'wmv', 'gif', 'mp3', 'm4a', 'wav'].map((t) => choice('convert', t, t.toUpperCase())),
      tools: [
        choice('tool', 'compress', 'Compress', [{ key: 'percent', label: 'Reduce by %', label_ar: '', default: '50' }]),
        choice('tool', 'crop', 'Crop', [], true),
        choice('tool', 'trim', 'Trim'),
        choice('tool', 'speed', 'Speed'),
        { ...choice('tool', 'recipe', '★ Web image'), preset: { steps: 'resize max=1600 | compress percent=60' } },
      ],
      actions: ['reveal', 'copy-path', 'move', 'terminal'].map((a) => choice('action', a, a)),
    },
    doctor: [{ name: 'FFmpeg', path: '/usr/bin/ffmpeg', hint: '' }, { name: 'libvips', path: null, hint: 'apt' }],
    hook_ok: true,
    take_pending: ['/home/u/clip.mp4'],
    get_settings: {
      convert: 'Shift', tools: 'Alt', actions: 'Control', reveal: true,
      recipes: [{ name: 'Web image', steps: 'resize max=1600 | strip' }],
      watch: [{ dir: '/home/u/in', exts: 'heic', steps: 'convert to=jpg', enabled: true }],
    },
    pick_folder: '/home/u/new',
    editor_state: query === 'pages'
      ? { paths: ['/home/u/d.pdf'], tool: 'pages', video: false, width: 0, height: 0, duration: 0, preview: '', pages: ['data:p1', 'data:p2', 'data:p3'], page_refs: [[1, 1], [1, 2], [1, 3]] }
      : query === 'merge-pdf'
      ? { paths: ['/u/a.pdf', '/u/b.pdf'], tool: 'merge-pdf', video: false, width: 0, height: 0, duration: 0, preview: '', pages: ['a1', 'a2', 'b1'], page_refs: [[1, 1], [1, 2], [2, 1]] }
      : query === 'trim' || query === 'snapshot'
      ? { paths: ['/u/clip.mp4'], tool: query, video: true, width: 1280, height: 720, duration: 10, preview: 'data:image/png;base64,AAAA', pages: [], page_refs: [], fps: 25, waveform: 'data:image/png;base64,WAVE', clips: [] }
      : query === 'join'
      ? { paths: ['/u/a.mp4', '/u/b.mov', '/u/c.mp4'], tool: 'join', video: true, width: 0, height: 0, duration: 0, preview: '', pages: [], page_refs: [], fps: 0, waveform: '',
        clips: [{ path: '/u/a.mp4', name: 'a.mp4', duration: 4, thumb: 'data:a' }, { path: '/u/b.mov', name: 'b.mov', duration: 2, thumb: 'data:b' }, { path: '/u/c.mp4', name: 'c.mp4', duration: 1.5, thumb: '' }] }
      : { paths: ['/home/u/p.png', '/home/u/q.png'], tool: query || 'redact', video: false, width: 1600, height: 1200, duration: 0, preview: 'data:image/png;base64,AAAA', pages: [] },
    detect_text: [{ x: 5, y: 6, w: 70, h: 20 }],
    detect_faces: [{ x: 100, y: 50, w: 60, h: 60 }],
    frame_at: 'data:image/png;base64,FRAME',
    float_list: ['/home/u/a.txt', '/home/u/b.png'],
    drag_icon: 'data:image/png;base64,AAAA',
  };
  class FormData {
    constructor(form) {
      this.map = new Map(form.querySelectorAll('input').concat(form.querySelectorAll('select'))
        .filter((i) => i.name && (i.type !== 'checkbox' || i.checked))
        .map((i) => [i.name, i.type === 'checkbox' ? 'on' : i.value]));
    }
    get(k) { return this.map.has(k) ? this.map.get(k) : null; }
    [Symbol.iterator]() { return this.map[Symbol.iterator](); }
  }
  class Image {
    set src(v) { this._src = v; setTimeout(() => { this.naturalWidth = 800; this.naturalHeight = 600; this.onload && this.onload(); }, 0); }
    get src() { return this._src; }
  }
  const document = {
    documentElement: {},
    getElementById: (id) => byId[id] || null,
    createElement: (t) => new El(t),
    createElementNS: (_, t) => new El(t),
    addEventListener: (t, f) => { (listeners[`doc:${t}`] ||= []).push(f); },
    querySelectorAll: () => [],
  };
  const sandbox = {
    console, setTimeout, clearTimeout, Promise, Math, Object, String, Number, JSON, Map, Symbol, Array,
    URLSearchParams, document, Image, FormData,
    navigator: { language: 'en-US' },
    devicePixelRatio: 1,
    location: { search: '' },
    __TAURI__: {
      core: {
        invoke: async (cmd, args) => {
          calls.push([cmd, args]);
          if (cmd === 'set_settings') {
            const s = args.settings;
            if (s.convert === s.tools || s.tools === s.actions || s.convert === s.actions) throw 'each wheel needs its own key';
          }
          return resp[cmd];
        },
      },
      event: { listen: async (name, cb) => { (listeners[name] ||= []).push(cb); } },
      webview: { getCurrentWebview: () => ({ onDragDropEvent: async (h) => { dropHandler = h; } }) },
      drag: { startDrag: async (opts, cb) => { calls.push(['startDrag', opts]); cb({ result: 'Dropped' }); } },
    },
  };
  sandbox.window = sandbox;
  sandbox.addEventListener = (t, f) => { (listeners[`win:${t}`] ||= []).push(f); };
  const ctx = vm.createContext(sandbox);
  for (const m of src.matchAll(/<script src="([^"]+)"/g)) {
    vm.runInContext(fs.readFileSync(path.join(UI, m[1]), 'utf8'), ctx, { filename: m[1] });
  }
  return {
    byId,
    calls,
    drag: (payload) => dropHandler({ payload }),
    emit: (name, payload) => (listeners[name] || []).forEach((cb) => cb({ payload })),
    key: (key) => (listeners['doc:keydown'] || []).forEach((f) => f({ key, target: { tagName: 'BODY' }, preventDefault() {} })),
    doc: (type, e) => (listeners[`doc:${type}`] || []).forEach((f) => f(e)),
  };
}

const segs = (p) => p.byId.wheel.children.filter((c) => c.tagName === 'PATH');

(async () => {
  // ---------- wheel ----------
  {
    const p = page('wheel.html');
    await p.drag({ type: 'enter', paths: ['/v/clip.mp4'], position: { x: 210, y: 30 } });
    ok(segs(p).length === 10, 'wheel: 10 convert segments for a video');
    ok(segs(p).filter((s) => s.attrs.class.includes('hot')).length === 1, 'wheel: segment under the pointer is highlighted');
    p.emit('wheel-mode', 'tools');
    ok(segs(p).length === 5, 'wheel: Alt switches to the tools ring');
    // first tools segment spans the top-right quarter: compress (has options)
    await p.drag({ type: 'drop', paths: ['/v/clip.mp4'], position: { x: 260, y: 40 } });
    await tick();
    ok(!p.byId.params.hidden, 'wheel: dropping on a tool with options opens the form');
    ok(p.calls.some(([c]) => c === 'keep_wheel'), 'wheel: the form keeps the wheel open');
    p.byId.params.querySelector('input').value = '70';
    await p.byId.params.fire('submit');
    const run = p.calls.find(([c]) => c === 'run_job');
    ok(run && run[1].id === 'compress' && run[1].opts.percent === '70', 'wheel: form submits run_job with the typed option');
    // second segment (crop) opens the editor instead
    p.emit('wheel-hidden');
    await p.drag({ type: 'enter', paths: ['/v/clip.mp4'], position: { x: 210, y: 210 } });
    p.emit('wheel-mode', 'tools');
    await p.drag({ type: 'drop', paths: ['/v/clip.mp4'], position: { x: 360, y: 260 } });
    await tick();
    const ed = p.calls.find(([c]) => c === 'open_editor');
    ok(ed && ed[1].tool === 'crop', 'wheel: editor tools open the editor');
    // centre drop cancels; dwell on a mode chip switches mode
    await p.drag({ type: 'enter', paths: ['/v/clip.mp4'], position: { x: 210, y: 210 } });
    await p.drag({ type: 'over', position: { x: 246, y: 180 } });
    await tick(500);
    ok(segs(p).length === 4 && p.byId.wheel.all().some((e) => e.textContent === 'Actions'), 'wheel: dwelling on a chip switches to actions');
    await p.drag({ type: 'over', position: { x: 210, y: 245 } });
    ok(p.byId.wheel.all().some((e) => e.textContent === 'Float'), 'wheel: hub shows Float');
    await p.drag({ type: 'drop', paths: ['/v/clip.mp4'], position: { x: 210, y: 245 } });
    const fl = p.calls.find(([c]) => c === 'float_add');
    ok(fl && fl[1].paths[0] === '/v/clip.mp4', 'wheel: dropping on the hub floats the files on the orb');
  }

  // ---------- wheel: keyboard mode (Ctrl+Shift+Space) ----------
  {
    const p = page('wheel.html');
    await p.emit('wheel-open', ['/v/clip.mp4']);
    await tick();
    ok(segs(p).length === 10 && segs(p)[0].attrs.class.includes('hot'), 'keyboard: wheel opens on the selection, first item active');
    p.key('ArrowRight');
    p.key('Enter');
    await tick();
    let run = p.calls.find(([c]) => c === 'run_job');
    ok(run && run[1].kind === 'convert' && run[1].id === 'mov', `keyboard: arrows + Enter convert (${run && run[1].id})`);
    p.key('Tab');
    ok(segs(p).length === 5, 'keyboard: Tab switches to the tools ring');
    // mouse: hover then click a segment, no drag involved
    p.doc('mousemove', { clientX: 260, clientY: 40 });
    p.doc('click', { clientX: 260, clientY: 40, target: {} });
    await tick();
    ok(!p.byId.params.hidden, 'keyboard: clicking a tool with options opens its form');
  }

  // ---------- editor ----------
  const pointer = (cv, type, fx, fy) => cv.fire(type, { pointerId: 1, clientX: fx * cv.width, clientY: fy * cv.height });
  const dragBox = (cv, a, b) => { pointer(cv, 'pointerdown', ...a); pointer(cv, 'pointermove', ...b); pointer(cv, 'pointerup', ...b); };
  {
    const p = page('editor.html', 'redact');
    await tick(10);
    const cv = p.byId.canvas;
    ok(cv.width === 800, 'editor: preview loaded into the canvas');
    dragBox(cv, [0.1, 0.1], [0.4, 0.3]);
    dragBox(cv, [0.6, 0.6], [0.9, 0.9]);
    await p.byId.apply.fire('click');
    const run = p.calls.find(([c]) => c === 'run_job');
    const boxes = run && run[1].opts.boxes.split(';');
    // preview 800 px wide, source 1600 px: every value doubles
    ok(boxes && boxes.length === 2 && boxes[0] === '160,120,480,240', `editor: redact boxes in source pixels (${boxes})`);
    ok(run && run[1].paths.length === 2 && run[1].opts.style === 'blur', 'editor: applies to the whole selection with blur');
  }
  {
    const p = page('editor.html', 'crop');
    await tick(10);
    const cv = p.byId.canvas;
    const sel = p.byId.controls.querySelector('select');
    sel.value = '16:9';
    sel.fire('change', { target: sel });
    dragBox(cv, [0.1, 0.1], [0.9, 0.9]);
    await p.byId.apply.fire('click');
    const run = p.calls.find(([c]) => c === 'run_job');
    const [, , w, h] = run ? run[1].opts.box.split(',').map(Number) : [];
    ok(run && Math.abs(w / h - 16 / 9) < 0.01, `editor: crop keeps 16:9 (${run && run[1].opts.box})`);
    await p.byId.apply.fire('click');
  }
  {
    const p = page('editor.html', 'crop');
    await tick(10);
    await p.byId.apply.fire('click');
    ok(p.byId.err.textContent.includes('draw a box'), 'editor: applying without a box explains why');
  }
  {
    const p = page('editor.html', 'redact');
    await tick(10);
    const detect = p.byId.controls.querySelectorAll('button').find((b) => b.textContent === 'Detect text');
    await detect.fire('click');
    await tick();
    await p.byId.apply.fire('click');
    const run = p.calls.find(([c]) => c === 'run_job');
    ok(run && run[1].opts.boxes === '10,12,140,40', `editor: detected text boxes are redacted (${run && run[1].opts.boxes})`);
  }
  {
    const p = page('editor.html', 'adjust');
    await tick(10);
    const sat = p.byId.controls.querySelectorAll('input')[2];
    sat.value = '1.6';
    sat.fire('input', { target: sat });
    await p.byId.apply.fire('click');
    const run = p.calls.find(([c]) => c === 'run_job');
    ok(run && run[1].id === 'adjust' && run[1].opts.saturation === '1.6' && run[1].opts.contrast === '1', 'editor: adjust sends slider values');
  }
  {
    const p = page('editor.html', 'pages');
    await tick(10);
    const cards = () => p.byId.pages.children;
    ok(cards().length === 3 && p.byId.canvas.hidden, 'organizer: one card per page, canvas hidden');
    const btns = (i) => cards()[i].querySelectorAll('button');
    btns(2)[0].fire('click'); // move page 3 earlier -> 1,3,2
    btns(0)[1].fire('click'); // rotate first card
    btns(2)[3].fire('click'); // remove last (page 2)
    btns(0)[2].fire('click'); // duplicate first -> 1,1,3
    await p.byId.apply.fire('click');
    const run = p.calls.find(([c]) => c === 'run_job');
    ok(run && run[1].opts.spec === '1,1,3' && run[1].opts.rotate === '1:90,2:90', `organizer: spec and rotations (${run && JSON.stringify(run[1].opts)})`);
  }
  {
    const p = page('editor.html', 'merge-pdf');
    await tick(10);
    const cards = () => p.byId.pages.children;
    ok(cards().length === 3 && cards()[2].querySelector('.page-n').textContent.includes('b.pdf'), 'merge: pages of every PDF, labelled by file');
    cards()[2].querySelectorAll('button')[0].fire('click'); // b1 earlier -> a1, b1, a2
    cards()[0].querySelectorAll('button')[0].fire('click'); // no-op at the start
    await p.byId.apply.fire('click');
    const run = p.calls.find(([c]) => c === 'run_job');
    ok(run && run[1].id === 'arrange' && run[1].opts.items === '1:1,2:1,1:2', `merge: arrange items (${run && run[1].opts.items})`);
  }
  {
    const p = page('editor.html', 'annotate');
    await tick(10);
    const cv = p.byId.canvas;
    dragBox(cv, [0.2, 0.2], [0.5, 0.5]);
    p.byId.controls.querySelectorAll('button')[3].fire('click'); // number
    pointer(cv, 'pointerdown', 0.7, 0.7);
    p.key('z'); // plain z is not undo
    await p.byId.apply.fire('click');
    const save = p.calls.find(([c]) => c === 'save_annotation');
    ok(save && save[1].src === '/home/u/p.png' && save[1].data.startsWith('data:image/png;base64,'), 'editor: annotate sends the layer as PNG');
    ok(save && save[1].width === 1600 && save[1].height === 1200, 'editor: layer is composited at the source size');
    ok(p.calls.some(([c]) => c === 'close_editor'), 'editor: closes after saving');
  }
  {
    const p = page('editor.html', 'redact');
    await tick(10);
    const btn = p.byId.controls.querySelectorAll('button').find((b) => b.textContent === 'Detect faces');
    await btn.fire('click');
    await p.byId.apply.fire('click');
    const run = p.calls.find(([c]) => c === 'run_job');
    // faces come back in preview pixels (800 px of a 1600 px source), like text boxes
    ok(run && run[1].opts.boxes === '200,100,120,120', `editor: detected faces are redacted (${run && run[1].opts.boxes})`);
  }
  {
    const p = page('editor.html', 'trim');
    await tick(10);
    ok(!p.byId.timeline.hidden && p.byId.wave.src === 'data:image/png;base64,WAVE', 'trim: timeline with waveform shown');
    ok(p.byId.saveFrame.hidden && !p.byId.setStart.hidden, 'trim: in/out buttons, no frame button');
    const scrub = p.byId.scrub;
    scrub.value = '2';
    scrub.fire('input', { target: scrub });
    p.key('i');
    p.key('ArrowRight'); // +1 frame (1/25 s)
    p.key('i');
    await tick(150);
    ok(p.calls.some(([c, a]) => c === 'frame_at' && Math.abs(a.t - 2.04) < 1e-6), 'trim: scrubbing fetches the frame (debounced)');
    scrub.value = '7.5';
    scrub.fire('input', { target: scrub });
    p.byId.setEnd.fire('click');
    ok(p.byId.clock.textContent === '0:07.500', `trim: clock shows the position (${p.byId.clock.textContent})`);
    await p.byId.apply.fire('click');
    const run = p.calls.find(([c]) => c === 'run_job');
    ok(run && run[1].id === 'trim' && run[1].opts.start === '2.040' && run[1].opts.end === '7.500', `trim: sends the frame-exact range (${run && JSON.stringify(run[1].opts)})`);
  }
  {
    const p = page('editor.html', 'trim');
    await tick(10);
    p.byId.scrub.value = '3';
    p.byId.scrub.fire('input', { target: p.byId.scrub });
    p.byId.setStart.fire('click');
    await p.byId.apply.fire('click');
    const run = p.calls.find(([c]) => c === 'run_job');
    ok(run && run[1].opts.start === '3.000' && run[1].opts.end === undefined, 'trim: untouched end is left open');
  }
  {
    const p = page('editor.html', 'snapshot');
    await tick(10);
    ok(!p.byId.saveFrame.hidden && p.byId.setStart.hidden, 'snapshot: frame button, no in/out');
    p.byId.scrub.value = '4.2';
    p.byId.scrub.fire('input', { target: p.byId.scrub });
    await p.byId.saveFrame.fire('click');
    const run = p.calls.find(([c]) => c === 'run_job');
    ok(run && run[1].id === 'snapshot' && run[1].opts.at === '4.200', `snapshot: saves the current frame (${run && run[1].opts.at})`);
  }
  {
    const p = page('editor.html', 'join');
    await tick(10);
    const list = p.byId.clips;
    ok(!list.hidden && list.children.length === 3 && p.byId.canvas.hidden, 'join: one card per clip');
    ok(p.byId.info.textContent === '3 · 0:07.500', `join: total length shown (${p.byId.info.textContent})`);
    // move c.mp4 to the front, then drop b.mov
    list.children[2].querySelector('.page-tools').children[0].fire('click');
    list.children[1].querySelector('.page-tools').children[0].fire('click');
    list.children[2].querySelector('.page-tools').children[1].fire('click');
    await p.byId.apply.fire('click');
    const run = p.calls.find(([c]) => c === 'run_job');
    ok(run && run[1].id === 'join' && run[1].paths.join() === '/u/c.mp4,/u/a.mp4', `join: clips in the chosen order (${run && run[1].paths})`);
    ok(p.calls.some(([c]) => c === 'close_editor'), 'join: closes after starting the job');
  }
  {
    const p = page('editor.html', 'join');
    await tick(10);
    const list = p.byId.clips;
    list.children[0].querySelector('.page-tools').children[1].fire('click');
    list.children[0].querySelector('.page-tools').children[1].fire('click');
    await p.byId.apply.fire('click');
    ok(!p.calls.some(([c]) => c === 'run_job') && p.byId.err.textContent.includes('two clips'), 'join: refuses a single clip');
  }
  {
    // a frame requested while scrubbing must not land after the editor closed
    const p = page('editor.html', 'trim');
    await tick(10);
    p.byId.scrub.value = '5';
    p.byId.scrub.fire('input', { target: p.byId.scrub });
    p.key('Escape');
    await tick(200);
    ok(!p.calls.some(([c]) => c === 'frame_at'), 'trim: closing cancels a pending frame fetch');
  }

  // ---------- orb (Float shelf) ----------
  {
    const p = page('shelf.html');
    await tick();
    const orb = p.byId.orb;
    ok(p.byId.badge.textContent === '2' && orb.attrs['data-tauri-drag-region'] === undefined, 'orb: held files shown, window drag off');
    orb.fire('pointerdown', { button: 0, clientX: 10, clientY: 10 });
    await orb.fire('pointermove', { clientX: 30, clientY: 10 });
    await tick();
    const drag = p.calls.find(([c]) => c === 'startDrag');
    ok(drag && drag[1].item.length === 2 && drag[1].icon.startsWith('data:image/png'), 'orb: dragging starts a native file drag');
    ok(p.calls.some(([c]) => c === 'float_clear'), 'orb: a completed drop clears the shelf');
    p.emit('float-changed', 0);
    ok(p.byId.badge.hidden && orb.attrs['data-tauri-drag-region'] === '', 'orb: empty shelf moves the window again');
  }

  // ---------- home ----------
  {
    const p = page('index.html');
    await tick(10);
    ok(p.byId.grid.children.length === 10, 'home: pending file shows its conversions');
    ok(p.byId.engines.children.length === 2, 'home: engines table filled');
    const box = p.byId.settings;
    const tools = box.querySelector('select[name=tools]');
    tools.value = 'Shift';
    await box.onsubmit({ preventDefault() {} });
    ok(p.byId.settingsMsg.textContent.includes('own key'), 'home: duplicate wheel keys are rejected');
    tools.value = 'Meta';
    // add a recipe row, remove the watch rule, add a new watch rule through the folder picker
    const lists = box.querySelectorAll('.list');
    lists[0].children[2].fire('click');
    const rows = lists[0].children[1].children;
    rows[1].querySelector('.r-name').value = 'Thumbs';
    rows[1].querySelector('.r-steps').value = 'resize max=300 | strip';
    lists[1].children[1].children[0].querySelectorAll('button')[1].fire('click');
    lists[1].children[2].fire('click');
    const wrow = lists[1].children[1].children[0];
    await wrow.querySelector('button').fire('click');
    await tick();
    wrow.querySelector('.w-steps').value = 'convert to=webp';
    await box.onsubmit({ preventDefault() {} });
    const set = p.calls.filter(([c]) => c === 'set_settings').pop();
    const st = set && set[1].settings;
    ok(st && st.tools === 'Meta' && st.reveal === true, 'home: settings saved');
    ok(st && st.recipes.length === 2 && st.recipes[1].name === 'Thumbs', 'home: recipes edited');
    ok(st && st.watch.length === 1 && st.watch[0].dir === '/home/u/new' && st.watch[0].steps === 'convert to=webp', 'home: watch rule picked and saved');
    // saved recipe runs with its preset steps
    p.byId.tabs.children[1].fire('click');
    p.byId.grid.children.find((b) => b.textContent === '★ Web image').fire('click');
    const rec = p.calls.find(([c, a]) => c === 'run_job' && a.id === 'recipe');
    ok(rec && rec[1].opts.steps.startsWith('resize max=1600'), 'home: saved recipe runs with its steps');
  }

  // ---------- jobs ----------
  {
    const p = page('jobs.html');
    p.emit('job', { type: 'start', job: 1, index: 0, total: 2, file: 'clip.mp4', title: '→ WEBM' });
    p.emit('job', { type: 'progress', job: 1, index: 0, fraction: 0.6 });
    p.emit('job', { type: 'start', job: 1, index: 1, total: 2, file: 'b.mov', title: '→ WEBM' });
    p.emit('job', { type: 'failed', job: 1, index: 1, error: 'FFmpeg failed' });
    p.emit('job', { type: 'done', job: 1, index: 0, output: '/x/clip.webm' });
    const rows = p.byId.list.children;
    ok(rows.length === 2, 'jobs: one row per file');
    ok(rows[1].querySelector('.bar').children[0].style.width === '60%' && rows[1].className.includes('done'), 'jobs: progress then done');
    ok(rows[0].className.includes('failed'), 'jobs: failure marked');
  }

  console.log(fails.length ? `${fails.length} failed` : 'all passed');
  process.exit(fails.length ? 1 : 0);
})().catch((e) => {
  console.error(e);
  process.exit(2);
});
