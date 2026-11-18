/**
 * The drag-wheel contract, ported from the plain-JS suite the previous build shipped: segment
 * counts per ring, the dwell that opens the nested ring, live mode switching, the keyboard path,
 * the options form and the hub that cancels.
 */

import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { cleanup, waitFor } from '@testing-library/react';
import { Wheel } from '../src/wheel/Wheel';
import { C, SCALE } from '../src/lib/geometry';
import { firstCall, installTauri, mount, type Harness } from './harness';

const CLIP = ['/v/clip.mp4'];

/** Middle of segment `i` of `n` on the ring at radius `r`, mapped into the 380 px window. */
const at = (i: number, n: number, r: number) => {
  const angle = -Math.PI / 2 + ((i + 0.5) * 2 * Math.PI) / n;
  return { x: C * SCALE + r * SCALE * Math.cos(angle), y: C * SCALE + r * SCALE * Math.sin(angle) };
};

const segs = () => [...document.querySelectorAll('#wheel path')];
const hot = () => [...document.querySelectorAll('#wheel path.hot')];
const params = () => document.getElementById('params') as HTMLFormElement | null;

describe('wheel', () => {
  let h: Harness;

  beforeEach(() => {
    h = installTauri();
  });

  afterEach(() => {
    cleanup();
    delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
  });

  it('draws three rings and follows the pointer', async () => {
    mount(<Wheel />);
    await h.drag({ type: 'enter', paths: CLIP, position: at(0, 10, 150) });

    expect(segs()).toHaveLength(13); // 3 modes + 10 formats
    expect(hot()).toHaveLength(1);
    // The current mode is filled, so its light label sits on the accent, not on the pale ring.
    expect(document.querySelector('[data-seg="mode-convert"]')!.classList.contains('current')).toBe(true);
    expect(document.querySelector('[data-seg="mode-tools"]')!.classList.contains('current')).toBe(false);
  });

  it('loads the rings from the paths the orb or the drag hook discovered', async () => {
    mount(<Wheel />);
    await h.emit('wheel-files', CLIP);

    expect(segs()).toHaveLength(13);
    expect(document.querySelector('[data-seg="item-mp4"]')).not.toBeNull();
  });

  it('hides itself when nobody touches the wheel it opened', async () => {
    mount(<Wheel />);
    await h.emit('wheel-files', CLIP);
    expect(h.calls.some((c) => c.cmd === 'hide_wheel')).toBe(false);

    // IDLE_MS is 3200: the wheel must not sit on top of every other window forever.
    await h.wait(3400);
    expect(h.calls.some((c) => c.cmd === 'hide_wheel')).toBe(true);
  });

  it('opens the nested ring after a dwell and applies the value it was dropped on', async () => {
    mount(<Wheel />);
    await h.drag({ type: 'enter', paths: CLIP, position: at(0, 10, 150) });
    await h.wait(400);

    expect(segs()).toHaveLength(16); // + 3 quality presets

    await h.drag({ type: 'drop', paths: CLIP, position: at(0, 3, 86) });
    const run = firstCall(h.calls, 'run_job');
    expect(run?.args.id).toBe('mp4');
    expect(run?.args.opts.quality).toBe('90');
  });

  it('switches rings with the mode key and opens the options form for a tool', async () => {
    mount(<Wheel />);
    await h.drag({ type: 'enter', paths: CLIP, position: at(0, 10, 150) });
    await h.emit('wheel-mode', 'tools');

    expect(segs()).toHaveLength(9); // 3 modes + 6 tools

    await h.drag({ type: 'drop', paths: CLIP, position: at(0, 5, 150) });
    expect(params()).not.toBeNull();
    expect(h.calls.some((c) => c.cmd === 'keep_wheel')).toBe(true);

    const input = params()!.querySelector('input') as HTMLInputElement;
    input.value = '70';
    await h.click(params()!.querySelector('button[type="submit"]'));

    const run = h.calls.filter((c) => c.cmd === 'run_job').at(-1);
    expect(run?.args.id).toBe('compress');
    expect(run?.args.opts.percent).toBe('70');
  });

  it('opens the editor for tools that need one', async () => {
    mount(<Wheel />);
    await h.drag({ type: 'enter', paths: CLIP, position: at(0, 10, 150) });
    await h.emit('wheel-mode', 'tools');
    await h.drag({ type: 'drop', paths: CLIP, position: at(1, 5, 150) });

    const opened = firstCall(h.calls, 'open_editor');
    expect(opened?.args.tool).toBe('crop');
  });

  it('morphs to the actions ring on a dwell, and the hub cancels', async () => {
    mount(<Wheel />);
    await h.drag({ type: 'enter', paths: CLIP, position: at(0, 10, 150) });
    await h.drag({ type: 'over', position: at(2, 3, 207) });
    await h.wait(500);

    expect(segs()).toHaveLength(7); // 3 modes + 4 actions
    expect(document.getElementById('wheel')!.textContent).toContain('Actions');

    await h.drag({ type: 'drop', paths: CLIP, position: { x: C * SCALE, y: C * SCALE } });
    expect(h.calls.some((c) => c.cmd === 'hide_wheel')).toBe(true);
  });

  it('runs entirely from the keyboard: arrows, Enter and Tab', async () => {
    mount(<Wheel />);
    await h.emit('wheel-open', CLIP);

    expect(segs()).toHaveLength(13);
    expect(hot()).toHaveLength(1);

    await h.key('ArrowRight');
    await h.key('Enter');
    expect(segs()).toHaveLength(16);

    await h.key('Enter');
    const run = firstCall(h.calls, 'run_job');
    expect(run?.args.kind).toBe('convert');
    expect(run?.args.id).toBe('mov');
    expect(run?.args.opts.quality).toBe('90');

    await h.key('Tab');
    expect(segs()).toHaveLength(9);
  });

  it('clicks a segment with the mouse when the wheel was opened by the hotkey', async () => {
    mount(<Wheel />);
    await h.emit('wheel-open', CLIP);
    await h.key('Tab');

    const point = at(0, 6, 150);
    await h.mouse('mousemove', point.x, point.y);
    await h.mouse('click', point.x, point.y);

    expect(params()).not.toBeNull();
  });

  it('says what to drop instead of showing an empty ring before it knows the files', async () => {
    mount(<Wheel />);
    // The Windows drag poller opens the wheel with only a mode: the paths follow on the next
    // event (the selection probe, or the OLE DragEnter once the pointer moves). Until then the
    // hub must read as "waiting", not as a wheel with no tools, which is what users reported.
    await h.emit('wheel-mode', 'convert');
    const hub = () => document.getElementById('wheel')!.textContent ?? '';

    expect(hub()).toContain('Drop files');
    expect(segs()).toHaveLength(3); // the mode ring only: no phantom item segments

    await h.emit('wheel-files', CLIP);
    expect(hub()).not.toContain('Drop files');
    expect(segs()).toHaveLength(13);
    expect(document.querySelector('[data-seg="item-mp4"]')).not.toBeNull();
  });

  it('the mouse wheel cycles the modes it was not asked to open', async () => {
    mount(<Wheel />);
    await h.emit('wheel-open', CLIP);
    await h.emit('wheel-mode', 'convert');

    await h.wheel(120);

    expect(document.getElementById('wheel')!.textContent).toContain('Tools');
  });

  it('cancelling the form hides the wheel instead of leaving it stranded', async () => {
    mount(<Wheel />);
    await h.emit('wheel-open', CLIP);
    await h.key('Tab');
    // Click compress: it has both a nested ring and textual options, and a click opens the form
    // (Enter would open the nested ring first, which is the dwell behaviour).
    const point = at(0, 6, 150);
    await h.mouse('mousemove', point.x, point.y);
    await h.mouse('click', point.x, point.y);

    const cancel = params()!.querySelectorAll('button')[0] as HTMLButtonElement;
    expect(cancel.textContent).toBe('Cancel');
    await h.click(cancel);

    // The form leaves through a 140 ms AnimatePresence exit, so it is still in the DOM for a
    // moment after the click; waiting is the contract, not a race the test should win.
    await waitFor(() => expect(params()).toBeNull());
    expect(h.calls.some((c) => c.cmd === 'hide_wheel')).toBe(true);
  });

  it('dims incompatible pinned formats via smart context filtering', async () => {
    const d = (await import('./harness')).videoDesc();
    d.convert.push({
      kind: 'convert',
      id: 'docx',
      label: 'DOCX',
      label_ar: 'DOCX',
      params: [],
      editor: false,
      children: [],
      compatible: false,
      pinned: true,
    });
    h = installTauri({ describe: d });
    mount(<Wheel />);
    await h.emit('wheel-open', CLIP);

    const docxSeg = document.querySelector('[data-seg="item-docx"]')!;
    expect(docxSeg.getAttribute('data-compatible')).toBe('false');
    expect(docxSeg.classList.contains('dim-incompatible')).toBe(true);
  });

  it('shows live EXT -> TARGET pill, micro-progress ring, and Reveal in Folder toast after drop', async () => {
    h = installTauri({ run_job: 77 });
    mount(<Wheel />);
    await h.drag({ type: 'enter', paths: ['/v/clip.mkv'], position: at(0, 10, 150) });

    // Live center pill displays source extension -> hovered target format.
    const wheel = document.getElementById('wheel')!;
    expect(wheel.textContent).toContain('MKV → MP4');
    // Solid hit-test backdrop exists so transparent gaps are never click-through dead zones.
    expect(wheel.querySelector('.wheel-hitbox')).not.toBeNull();

    await h.drag({ type: 'drop', paths: ['/v/clip.mkv'], position: at(0, 10, 150) });
    expect(firstCall(h.calls, 'run_job')?.args.id).toBe('mp4');

    await h.emit('job', { type: 'progress', job: 77, index: 0, fraction: 0.5 });
    expect(wheel.textContent).toContain('50%');

    await h.emit('job', { type: 'done', job: 77, index: 0, output: '/v/clip.mp4' });
    const revealBtn = document.getElementById('wheelRevealBtn') as HTMLButtonElement;
    expect(revealBtn).not.toBeNull();
    await h.click(revealBtn);
    expect(firstCall(h.calls, 'reveal')?.args.path).toBe('/v/clip.mp4');
  });

  it('renders Arabic RTL labels and LTR live conversion badges inside the wheel cleanly', async () => {
    localStorage.setItem('zeidex_lang', 'ar');
    try {
      mount(<Wheel />);
      await h.drag({ type: 'enter', paths: ['/v/clip.mkv'], position: at(0, 10, 150) });

      expect(document.documentElement.dir).toBe('rtl');
      const wheel = document.getElementById('wheel')!;
      expect(wheel.textContent).toContain('تحويل');
      expect(wheel.textContent).toContain('MKV → MP4');
      expect(document.querySelector('[direction="ltr"]')?.textContent).toContain('MKV → MP4');
    } finally {
      localStorage.removeItem('zeidex_lang');
    }
  });

  it('switches sections on mouse movement / wheel-hover and commits drops via wheel-commit onto any tool', async () => {
    h = installTauri({ run_job: 88 });
    mount(<Wheel />);
    await h.emit('wheel-files', CLIP);

    // Move mouse over the "Tools" chip (index 1 of 3 modes) without requiring wheel-open clickMode.
    const toolsChip = at(1, 3, 210);
    await h.mouse('mousemove', toolsChip.x, toolsChip.y);
    await h.wait(220);
    expect(document.getElementById('wheel')!.textContent).toContain('Tools');

    // Hover a tool with presets via native wheel-hover stream (index 0 of 6: Compress), open sub-ring, then commit via wheel-commit.
    const compressWedge = at(0, 6, 150);
    await h.emit('wheel-hover', compressWedge);
    await h.wait(220);
    expect(segs().length).toBeGreaterThan(8);

    await h.emit('wheel-commit', { paths: [], position: at(0, 2, 86) });
    const run = firstCall(h.calls, 'run_job');
    expect(run?.args.kind).toBe('tool');
    expect(run?.args.id).toBe('compress');
    expect(run?.args.opts.percent).toBe('50');
    expect(run?.args.paths).toEqual(CLIP);
  });

  it('locks native DragDrop paths so late provisional wheel-files never add un-dragged files', async () => {
    h = installTauri({ run_job: 99 });
    mount(<Wheel />);

    const draggedDoc = ['/docs/only-this-report.pdf'];
    await h.drag({ type: 'enter', paths: draggedDoc, position: at(0, 10, 150) });
    // Simulate a late Explorer/clipboard probe trying to inject extra unrelated files.
    await h.emit('wheel-files', ['/stale/a.txt', '/stale/b.docx', '/stale/c.pdf']);

    await h.drag({ type: 'drop', paths: [], position: at(0, 10, 150) });
    const run = firstCall(h.calls, 'run_job');
    expect(run?.args.paths).toEqual(draggedDoc);
  });

  it('keeps the requested mode when wheel-mode arrives before wheel-files loads', async () => {
    mount(<Wheel />);
    await h.emit('wheel-mode', 'tools');
    await h.emit('wheel-files', CLIP);

    expect(document.querySelector('[data-seg="mode-tools"]')!.classList.contains('current')).toBe(true);
    expect(document.querySelector('[data-seg="item-compress"]')).not.toBeNull();
    expect(segs()).toHaveLength(9); // 3 modes + 6 tools
  });

  it('does not reset dwell timers under a 60 Hz wheel-hover stream', async () => {
    mount(<Wheel />);
    await h.emit('wheel-files', CLIP);

    // Stream 60 Hz wheel-hover events over the Tools mode chip, then verify it switches after DWELL_MS.
    const toolsChip = at(1, 3, 210);
    for (let i = 0; i < 4; i++) {
      await h.emit('wheel-hover', toolsChip);
    }
    await h.wait(180);
    expect(document.querySelector('[data-seg="mode-tools"]')!.classList.contains('current')).toBe(true);

    // Stream 60 Hz wheel-hover events over Compress, then verify its nested sub-ring opens after OPEN_MS.
    const compressWedge = at(0, 6, 150);
    for (let i = 0; i < 4; i++) {
      await h.emit('wheel-hover', compressWedge);
    }
    await h.wait(210);
    expect(document.querySelector('[data-seg="sub-percent-50"]')).not.toBeNull();
  });

  it('recovers dragged paths and mode from wheel_state if mounted after opening', async () => {
    h = installTauri({ wheel_state: { mode: 'tools', paths: CLIP, click: false } });
    mount(<Wheel />);
    await h.wait(30);

    expect(document.querySelector('[data-seg="mode-tools"]')!.classList.contains('current')).toBe(true);
    expect(document.querySelector('[data-seg="item-compress"]')).not.toBeNull();
  });
});
