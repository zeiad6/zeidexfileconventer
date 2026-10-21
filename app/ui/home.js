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
  if (!it.params.length) return go({});
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
  box.append(rev);
  box.onchange = async () => {
    const f = new FormData(box);
    const next = { convert: f.get('convert'), tools: f.get('tools'), actions: f.get('actions'), reveal: f.get('reveal') === 'on' };
    try {
      await invoke('set_settings', { settings: next });
      $('settingsMsg').textContent = window.ZT('saved');
    } catch (e) {
      $('settingsMsg').textContent = String(e);
    }
  };
}

listen('open-files', async () => load(await invoke('take_pending')));
listen('hook-error', () => { $('hook').textContent = window.ZT('hookOff'); });

(async () => {
  $('hook').textContent = window.ZT((await invoke('hook_ok')) ? 'hookOn' : 'hookOff');
  engines();
  settings();
  load(await invoke('take_pending'));
})();
