// The orb: a drop target for sessions without global keys (Wayland) and Tangerine's Float
// shelf. Dragging files over it opens the wheel around it; files dropped on the wheel's hub
// are held here (badge) and can be dragged out of the orb into any other app.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const orb = document.getElementById('orb');
const badge = document.getElementById('badge');
const DRAG_START_PX = 6;
let held = 0;
let press = null;

function setHeld(n) {
  held = n;
  badge.textContent = n ? String(n) : '';
  badge.hidden = !n;
  // With files held, a drag takes the files out; without, it moves the orb.
  if (n) orb.removeAttribute('data-tauri-drag-region');
  else orb.setAttribute('data-tauri-drag-region', '');
}

window.__TAURI__.webview.getCurrentWebview().onDragDropEvent((e) => {
  const t = e.payload.type;
  orb.classList.toggle('over', t === 'enter' || t === 'over');
  if (t === 'enter') invoke('wheel_at_shelf');
  if (t === 'drop') invoke('float_add', { paths: e.payload.paths });
});

orb.addEventListener('pointerdown', (e) => {
  if (held && e.button === 0) press = { x: e.clientX, y: e.clientY };
});
orb.addEventListener('pointerup', () => { press = null; });
orb.addEventListener('pointermove', async (e) => {
  if (!press || Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_START_PX) return;
  press = null;
  const files = await invoke('float_list');
  if (!files.length) return;
  const icon = await invoke('drag_icon');
  await window.__TAURI__.drag.startDrag({ item: files, icon }, (ev) => {
    if (ev.result === 'Dropped') invoke('float_clear');
  });
});
orb.addEventListener('dblclick', async () => {
  const files = held ? await invoke('float_list') : [];
  if (files.length) invoke('open_in_home', { paths: files });
  else invoke('show_home');
});
orb.addEventListener('contextmenu', (e) => {
  e.preventDefault();
  invoke('float_clear');
});

listen('float-changed', (e) => setHeld(e.payload));
invoke('float_list').then((f) => setHeld(f.length));
