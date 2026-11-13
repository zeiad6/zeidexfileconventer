/**
 * The drag-wheel contract, ported from the plain-JS suite the previous build shipped: segment
 * counts per ring, the dwell that opens the nested ring, live mode switching, the keyboard path,
 * the options form and the hub that cancels.
 */

import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { cleanup, waitFor } from '@testing-library/react';
import { Wheel } from '../src/wheel/Wheel';
import { C } from '../src/lib/geometry';
import { firstCall, installTauri, mount, type Harness } from './harness';

const CLIP = ['/v/clip.mp4'];

/** Middle of segment `i` of `n` on the ring at radius `r` (the wheel is 480 px, centred at 240). */
const at = (i: number, n: number, r: number) => {
  const angle = -Math.PI / 2 + ((i + 0.5) * 2 * Math.PI) / n;
  return { x: C + r * Math.cos(angle), y: C + r * Math.sin(angle) };
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

    await h.drag({ type: 'drop', paths: CLIP, position: { x: C, y: C } });
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
});
