// Drop target for sessions without global keys (Wayland), and a one-glance entry point.
// Dragging files over it opens the wheel around it; drag on to a segment and release.
const { invoke } = window.__TAURI__.core;
const orb = document.getElementById('orb');
window.__TAURI__.webview.getCurrentWebview().onDragDropEvent((e) => {
  const t = e.payload.type;
  orb.classList.toggle('over', t === 'enter' || t === 'over');
  if (t === 'enter') invoke('wheel_at_shelf');
  if (t === 'drop') invoke('open_in_home', { paths: e.payload.paths });
});
orb.addEventListener('dblclick', () => invoke('show_home'));
