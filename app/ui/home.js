// Main window: click-based version of the wheel for when dragging is awkward, engine status,
// and file-manager integration.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);
const make = (tag, props) => Object.assign(document.createElement(tag), props);

let paths = [];
let desc = null;
let mode = 'convert';
let view = 'convert';
// Tool the wheel asked the home window to open (the rename action lands here, where the
// pattern has a live preview instead of a pattern the user never saw).
let wantedTool = null;
const VIEW_TITLES = { convert: 'navConvert', tools: 'tools', shelf: 'navShelf', watch: 'watchFolders', settings: 'settings' };

async function load(p) {
  if (!p.length) return;
  paths = p;
  desc = await invoke('describe', { paths });
  if (!desc[mode].length) mode = ['convert', 'tools', 'actions'].find((m) => desc[m].length) || 'convert';
  if (view !== 'convert' && view !== 'tools') showView('convert');
  const names = paths.map((x) => x.split(/[\\/]/).pop());
  $('files').textContent = `${paths.length} ${window.ZT('selected')}: ${names.slice(0, 6).join(', ')}${names.length > 6 ? '…' : ''}`;
  $('sel').hidden = false;
  render();
  if (wantedTool) pickTool(wantedTool);
}

function render() {
  $('form').hidden = true;
  $('tabs').replaceChildren(...['convert', 'tools', 'actions'].map((m) => make('button', {
    textContent: window.ZT(m),
    className: m === mode ? 'on' : '',
    role: 'tab',
    disabled: !desc[m].length,
    onclick: () => { mode = m; render(); },
  })));
  const list = desc[mode];
  $('grid').replaceChildren(...(list.length
    ? list.map((it) => Object.assign(make('button', { textContent: window.ZL(it), onclick: () => choose(it) }), { dataset: { id: it.id } }))
    : [make('p', { className: 'hint', textContent: window.ZT('nothing') })]));
}

function choose(it) {
  if (it.editor) return invoke('open_editor', { paths, tool: it.id });
  const go = (opts) => invoke('run_job', { paths, kind: it.kind, id: it.id, opts });
  if (!it.params.length) return go(it.preset || {});
  window.ZForm($('form'), it, async (opts) => { await go(opts); $('form').hidden = true; }, () => { $('form').hidden = true; });
}

async function engines() {
  const rows = await invoke('doctor');
  $('engines').replaceChildren(...rows.map((r) => {
    const tr = make('tr');
    tr.append(
      make('td', { textContent: r.path ? '✓' : '✗', className: r.path ? 'ok' : 'bad' }),
      make('td', { textContent: r.name }),
    );
    const td = make('td');
    td.append(make('code', { textContent: r.path || r.hint }));
    tr.append(td);
    return tr;
  }));
}

window.__TAURI__.webview.getCurrentWebview().onDragDropEvent((e) => {
  const t = e.payload.type;
  $('drop').classList.toggle('over', t === 'enter' || t === 'over');
  if (t === 'drop') load(e.payload.paths);
});

// ---------- window controls (undecorated shell) ----------
const appWin = window.__TAURI__.window.getCurrentWindow();
$('winClose').onclick = () => appWin.close();
$('winMin').onclick = () => appWin.minimize();
$('winMax').onclick = () => appWin.toggleMaximize();
for (const strip of document.querySelectorAll('[data-resize]')) {
  strip.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    appWin.startResizeDragging(strip.dataset.resize);
  });
}

// ---------- studio navigation ----------
const VIEW_IDS = { convert: 'navConvert', tools: 'navTools', shelf: 'navShelf', watch: 'navWatch', settings: 'navSettings' };

function showView(name) {
  view = name;
  // The tools ring has no separate page: it is the same picker with the Tools tab active.
  const target = name === 'tools' ? 'convert' : name;
  for (const viewName of ['convert', 'shelf', 'watch', 'settings']) {
    $(`view-${viewName}`).hidden = viewName !== target;
  }
  for (const [viewName, id] of Object.entries(VIEW_IDS)) {
    $(id).classList.toggle('on', viewName === name);
  }
  $('viewTitle').textContent = window.ZT(VIEW_TITLES[name] || 'navConvert');
  if (name === 'tools' && desc) {
    mode = desc.tools.length ? 'tools' : mode;
    render();
  }
  if (name === 'shelf') renderShelf();
  if (name === 'watch') renderWatch();
}

for (const [name, id] of Object.entries(VIEW_IDS)) {
  $(id).onclick = () => showView(name);
}

function pickTool(id) {
  wantedTool = id;
  showView('tools');
  const btn = [...$('grid').children].find((b) => b.dataset && b.dataset.id === id);
  if (btn) {
    wantedTool = null;
    btn.click();
  }
}

listen('open-tool', (e) => pickTool(e.payload));

async function renderShelf() {
  const files = await invoke('float_list');
  $('shelfList').replaceChildren(...(files.length
    ? files.map((p) => {
      const li = make('li');
      li.append(
        make('span', { className: 'name', textContent: p.split(/[\\/]/).pop(), title: p }),
        make('button', { type: 'button', textContent: window.ZT('open'), onclick: () => invoke('open_in_home', { paths: [p] }) }),
      );
      return li;
    })
    : [make('li', { className: 'hint', textContent: window.ZT('shelfEmpty') })]));
}

async function renderWatch() {
  const s = await invoke('get_settings');
  const rules = s.watch || [];
  $('watchList').replaceChildren(...(rules.length
    ? rules.map((w) => {
      const row = make('div', { className: 'watch-item' });
      row.append(
        make('span', { className: 'dir', textContent: w.dir || '—', title: w.dir }),
        make('code', { textContent: `${w.exts || '*'} → ${w.steps || '—'}` }),
        make('span', { className: w.enabled ? 'ok' : 'bad', textContent: window.ZT(w.enabled ? 'on' : 'off') }),
      );
      return row;
    })
    : [make('p', { className: 'hint', textContent: window.ZT('watchEmpty') })]));
}

$('shelfAdd').onclick = async () => {
  const picked = await invoke('pick_files');
  if (picked.length) await invoke('float_add', { paths: picked });
  renderShelf();
};
$('shelfClear').onclick = async () => { await invoke('float_clear'); renderShelf(); };
$('shelfOpen').onclick = async () => {
  const files = await invoke('float_list');
  if (files.length) invoke('open_in_home', { paths: files });
};
$('watchEdit').onclick = async () => {
  showView('settings');
  await settings();
  const heading = [...document.querySelectorAll('#settings h3')]
    .find((h) => h.textContent === window.ZT('watchFolders'));
  if (heading) {
    heading.scrollIntoView({ block: 'center' });
    heading.classList.add('flash');
    setTimeout(() => heading.classList.remove('flash'), 900);
  }
};

// Browser drag-and-drop support: images and links dragged directly from a browser (Chrome, Edge, Firefox)
const dropArea = $('drop');
dropArea.addEventListener('dragover', (e) => {
  e.preventDefault();
  dropArea.classList.add('over');
});
dropArea.addEventListener('dragleave', () => {
  dropArea.classList.remove('over');
});
dropArea.addEventListener('drop', async (e) => {
  e.preventDefault();
  dropArea.classList.remove('over');
  const html = e.dataTransfer.getData('text/html');
  const uri = e.dataTransfer.getData('text/uri-list');
  let url = null;
  if (html) {
    const m = html.match(/<img[^>]+src=["']([^"']+)["']/i);
    if (m) url = m[1];
  }
  if (!url && uri) {
    url = uri.split('\n').map((s) => s.trim()).find((s) => s && !s.startsWith('#'));
  }
  if (url && (url.startsWith('http://') || url.startsWith('https://'))) {
    try {
      const localPath = await invoke('download_dragged_url', { url });
      if (localPath) load([localPath]);
    } catch (err) {
      console.warn('browser drag download failed', err);
    }
  }
});
$('choose').onclick = async () => load(await invoke('pick_files'));
$('integrate').onclick = async () => {
  try {
    const written = await invoke('integrate_shell');
    $('integrated').textContent = `${window.ZT('added')} ${written.join(' · ')}`;
  } catch (e) {
    $('integrated').textContent = String(e);
  }
};

async function settings() {
  const s = await invoke('get_settings');
  const keys = ['Shift', 'Alt', 'Control', 'Meta'];
  const box = $('settings');
  box.replaceChildren();
  for (const [field, label] of [['convert', 'keyConvert'], ['tools', 'keyTools'], ['actions', 'keyActions']]) {
    const sel = make('select', { name: field });
    for (const key of keys) sel.append(make('option', { value: key, textContent: key, selected: s[field] === key }));
    const l = make('label', { textContent: window.ZT(label) });
    l.append(sel);
    box.append(l);
  }
  const rev = make('label', { className: 'check' });
  rev.append(make('input', { type: 'checkbox', name: 'reveal', checked: s.reveal }), window.ZT('reveal'));
  const hot = make('label', { className: 'check' });
  hot.append(make('input', { type: 'checkbox', name: 'hotkey', checked: s.hotkey !== false }), window.ZT('hotkey'));
  box.append(rev, hot);

  // Recipes and watch rules are lists of rows; each row keeps its own inputs.
  const recipeRows = make('div', { className: 'rows' });
  const recipeRow = (r = { name: '', steps: '' }) => {
    const row = make('div', { className: 'row-item' });
    row.append(
      make('input', { className: 'r-name', placeholder: window.ZT('recipeName'), value: r.name }),
      make('input', { className: 'r-steps wide', placeholder: window.ZT('recipeSteps'), value: r.steps }),
      make('button', { type: 'button', textContent: '✕', onclick: () => row.remove() }),
    );
    recipeRows.append(row);
  };
  (s.recipes || []).forEach(recipeRow);
  const watchRows = make('div', { className: 'rows' });
  const watchRow = (w = { dir: '', exts: '', steps: '', enabled: true }) => {
    const row = make('div', { className: 'row-item' });
    const dir = make('input', { className: 'w-dir wide', value: w.dir, readOnly: true, placeholder: window.ZT('folder') });
    row.append(
      make('button', { type: 'button', textContent: window.ZT('folder'), onclick: async () => { const d = await invoke('pick_folder'); if (d) dir.value = d; } }),
      dir,
      make('input', { className: 'w-exts', placeholder: window.ZT('watchExts'), value: w.exts }),
      make('input', { className: 'w-steps wide', placeholder: window.ZT('watchSteps'), value: w.steps }),
      make('label', { className: 'check inline', textContent: window.ZT('on') }),
      make('button', { type: 'button', textContent: '✕', onclick: () => row.remove() }),
    );
    row.querySelector('label').prepend(make('input', { type: 'checkbox', className: 'w-on', checked: w.enabled }));
    watchRows.append(row);
  };
  (s.watch || []).forEach(watchRow);
  const section = (title, rows, add) => {
    const wrap = make('div', { className: 'list' });
    wrap.append(make('h3', { textContent: window.ZT(title) }), rows, make('button', { type: 'button', textContent: window.ZT('add'), onclick: () => add() }));
    return wrap;
  };
  box.append(section('recipes', recipeRows, recipeRow), section('watchFolders', watchRows, watchRow));
  const save = make('button', { type: 'submit', className: 'primary', textContent: window.ZT('saveSettings') });
  box.append(save);

  box.onsubmit = async (e) => {
    if (e && e.preventDefault) e.preventDefault();
    const val = (row, cls) => row.querySelector(cls).value.trim();
    const next = {
      convert: box.querySelector('select[name=convert]').value,
      tools: box.querySelector('select[name=tools]').value,
      actions: box.querySelector('select[name=actions]').value,
      reveal: box.querySelector('input[name=reveal]').checked,
      hotkey: box.querySelector('input[name=hotkey]').checked,
      recipes: [...recipeRows.children].map((r) => ({ name: val(r, '.r-name'), steps: val(r, '.r-steps') })).filter((r) => r.name || r.steps),
      watch: [...watchRows.children].map((r) => ({ dir: val(r, '.w-dir'), exts: val(r, '.w-exts'), steps: val(r, '.w-steps'), enabled: r.querySelector('.w-on').checked }))
        .filter((w) => w.dir || w.steps),
    };
    try {
      await invoke('set_settings', { settings: next });
      $('settingsMsg').textContent = window.ZT('saved');
    } catch (err) {
      $('settingsMsg').textContent = String(err);
    }
  };
}

listen('open-files', async () => load(await invoke('take_pending')));
listen('hook-error', () => { $('hook').textContent = window.ZT('hookOff'); });
// Ctrl+Shift+Space is also used by other apps (e.g. editors): never pop up, only explain.
listen('selection-empty', () => { $('hook').textContent = window.ZT('selectionHint'); });

function setupModals() {
  const DEV_URL = 'https://zeidexlink.zeidexsoft.com/zeidex';
  const SITE_URL = 'https://zeidexsoft.com';

  const aboutModal = $('aboutModal');
  const donateModal = $('donateModal');

  $('langToggle').onclick = () => window.toggleLang();
  $('aboutBtn').onclick = () => aboutModal.showModal();
  $('donateBtn').onclick = () => donateModal.showModal();

  $('closeAboutBtn').onclick = () => aboutModal.close();
  $('closeAboutBtn2').onclick = () => aboutModal.close();
  $('closeDonateBtn').onclick = () => donateModal.close();
  $('closeDonateBtn2').onclick = () => donateModal.close();

  $('aboutDonateBtn').onclick = () => {
    aboutModal.close();
    donateModal.showModal();
  };

  $('devProfileBtn').onclick = () => window.openExternal(DEV_URL);
  $('devWebsiteBtn').onclick = () => window.openExternal(SITE_URL);
  $('starsLinkBtn').onclick = () => window.openExternal(DEV_URL);
  $('onlinePayBtn').onclick = () => window.openExternal(DEV_URL);
  $('websitePayBtn').onclick = () => window.openExternal(SITE_URL);

  document.querySelectorAll('[data-copy]').forEach((btn) => {
    btn.onclick = () => window.copyText(btn.dataset.copy, btn);
  });

  window.addEventListener('zeidex-lang-change', () => {
    if (paths.length && desc) render();
    $('viewTitle').textContent = window.ZT(VIEW_TITLES[view] || 'navConvert');
    if (view === 'shelf') renderShelf();
    if (view === 'watch') renderWatch();
    settings();
    invoke('hook_ok').then((ok) => {
      $('hook').textContent = window.ZT(ok ? 'hookOn' : 'hookOff');
    });
  });
}

(async () => {
  setupModals();
  $('hook').textContent = window.ZT((await invoke('hook_ok')) ? 'hookOn' : 'hookOff');
  engines();
  settings();
  load(await invoke('take_pending'));
})();
