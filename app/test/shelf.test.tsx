/**
 * The orb contract: the badge counts what the shelf holds, dragging out of the orb starts a
 * native file drag, a completed drop empties the shelf, and an empty orb moves the window again
 * instead of starting a drag with nothing in it.
 */

import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, waitFor } from '@testing-library/react';
import { Shelf } from '../src/shelf/Shelf';
import { firstCall, flush, installTauri, mount } from './harness';

const orb = () => document.getElementById('orb')!;
const badge = () => document.getElementById('badge');

describe('orb', () => {
  afterEach(() => {
    cleanup();
    delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
  });

  it('shows how many files it holds and gives up the window drag', async () => {
    installTauri();
    mount(<Shelf />);
    await flush();

    expect(badge()!.textContent).toBe('2');
    expect(orb().getAttribute('data-tauri-drag-region')).toBeNull();
  });

  it('drags the held files out and clears them once the drop lands', async () => {
    const h = installTauri();
    mount(<Shelf />);
    await flush();

    await h.pointerOn(orb(), 'pointerdown', 10, 10);
    await h.pointerOn(orb(), 'pointermove', 30, 10);

    const drag = firstCall(h.calls, 'startDrag');
    expect(drag?.args.item).toHaveLength(2);
    expect(String(drag?.args.icon).startsWith('data:image/png')).toBe(true);
    await waitFor(() => expect(h.calls.some((c) => c.cmd === 'float_clear')).toBe(true));
  });

  it('moves the window again once the shelf is empty', async () => {
    const h = installTauri();
    mount(<Shelf />);
    await flush();

    await h.emit('float-changed', 0);
    expect(badge()).toBeNull();
    expect(orb().getAttribute('data-tauri-drag-region')).toBe('');

    await h.pointerOn(orb(), 'pointerdown', 10, 10);
    await h.pointerOn(orb(), 'pointermove', 30, 10);
    expect(firstCall(h.calls, 'startDrag')).toBeUndefined();
  });

  it('adds files dropped on the orb to the shelf', async () => {
    const h = installTauri();
    mount(<Shelf />);
    await flush();

    await h.drag({ type: 'drop', paths: ['/u/new.png'], position: { x: 10, y: 10 } });
    expect(firstCall(h.calls, 'float_add')?.args.paths).toEqual(['/u/new.png']);
  });

  it('hands the dragged paths to the wheel that opens around the orb', async () => {
    const h = installTauri();
    mount(<Shelf />);
    await flush();

    await h.drag({ type: 'enter', paths: ['/u/a.pdf', '/u/b.pdf'], position: { x: 10, y: 10 } });
    const open = firstCall(h.calls, 'wheel_at_shelf');
    expect(open?.args.paths).toEqual(['/u/a.pdf', '/u/b.pdf']);
  });
});
