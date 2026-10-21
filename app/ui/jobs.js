// Progress window: one row per file of each job, newest first.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const list = document.getElementById('list');
const make = (tag, props) => Object.assign(document.createElement(tag), props);
const rows = new Map();

function row(key, name, meta) {
  let r = rows.get(key);
  if (!r) {
    r = make('div', { className: 'job' });
    r.append(make('div', { className: 'name', textContent: name }), make('div', { className: 'meta', textContent: meta }));
    const bar = make('div', { className: 'bar' });
    bar.append(make('i'));
    r.append(bar);
    list.prepend(r);
    rows.set(key, r);
  }
  return r;
}

listen('job', ({ payload: e }) => {
  const key = `${e.job}:${e.index ?? 0}`;
  if (e.type === 'start') {
    row(key, e.file, `${e.title} · ${e.index + 1}/${e.total}`);
  } else if (e.type === 'progress') {
    row(key, '', '').querySelector('.bar i').style.width = `${Math.round(e.fraction * 100)}%`;
  } else if (e.type === 'done') {
    const r = row(key, '', '');
    r.classList.add('done');
    const name = e.output.split(/[\\/]/).pop();
    r.querySelector('.meta').textContent = `→ ${name}`;
    r.append(make('button', { textContent: window.ZT('show'), onclick: () => invoke('reveal', { path: e.output }) }));
  } else if (e.type === 'failed') {
    const r = row(key, '', '');
    r.classList.add('failed');
    r.append(make('div', { className: 'error', textContent: e.error }));
  } else if (e.type === 'text') {
    const r = row(`${e.job}:${e.title}`, e.title, '');
    r.classList.add('done');
    r.append(make('pre', { textContent: e.text }));
  }
});

document.getElementById('clear').onclick = () => {
  list.replaceChildren();
  rows.clear();
};
