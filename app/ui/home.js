// Main window: click-based version of the wheel for when dragging is awkward, engine status,
// and file-manager integration.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);
const make = (tag, props) => Object.assign(document.createElement(tag), props);

let paths = [];
let desc = null;
let mode = 'convert';

async function load(p) {
  if (!p.length) return;
  paths = p;
  desc = await invoke('describe', { paths });
  if (!desc[mode].length) mode = ['convert', 'tools', 'actions'].find((m) => desc[m].length) || 'convert';
  const names = paths.map((x) => x.split(/[\\/]/).pop());
  $('files').textContent = `${paths.length} ${window.ZT('selected')}: ${names.slice(0, 6).join(', ')}${names.length > 6 ? '…' : ''}`;
  $('sel').hidden = false;
  render();
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
    ? list.map((it) => make('button', { textContent: window.ZL(it), onclick: () => choose(it) }))
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
