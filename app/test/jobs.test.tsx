/**
 * The progress window contract: one row per file, progress fills the bar, completion and failure
 * are marked on the row they belong to, and the newest job stays on top.
 */

import { afterEach, describe, expect, it } from 'vitest';
import { cleanup } from '@testing-library/react';
import { Jobs } from '../src/jobs/Jobs';
import { installTauri, mount } from './harness';

const rows = () => [...document.getElementById('list')!.children] as HTMLElement[];

describe('jobs', () => {
  afterEach(() => {
    cleanup();
    delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
  });

  it('tracks each file of a job, newest first', async () => {
    const h = installTauri();
    mount(<Jobs />);

    await h.emit('job', { type: 'start', job: 1, index: 0, total: 2, file: 'clip.mp4', title: '→ WEBM' });
    await h.emit('job', { type: 'progress', job: 1, index: 0, fraction: 0.6 });
    await h.emit('job', { type: 'start', job: 1, index: 1, total: 2, file: 'b.mov', title: '→ WEBM' });
    await h.emit('job', { type: 'failed', job: 1, index: 1, error: 'FFmpeg failed' });
    await h.emit('job', { type: 'done', job: 1, index: 0, output: '/x/clip.webm' });

    const list = rows();
    expect(list).toHaveLength(2);
    expect(list[1].className).toContain('done');
    // A finished row reads 100% (the previous build froze the bar at the last progress event).
    expect((list[1].querySelector('.bar')!.children[0] as HTMLElement).style.width).toBe('100%');
    expect(list[0].className).toContain('failed');
    expect(list[0].textContent).toContain('FFmpeg failed');
  });

  it('appends log text to the row it belongs to', async () => {
    const h = installTauri();
    mount(<Jobs />);

    await h.emit('job', { type: 'start', job: 2, index: 0, total: 1, file: 'a.png', title: '→ WEBP' });
    await h.emit('job', { type: 'text', job: 2, title: '→ WEBP', text: 'line one\nline two' });

    const row = rows()[0];
    expect(row.querySelector('pre')!.textContent).toContain('line one');
  });

  it('can be cleared', async () => {
    const h = installTauri();
    mount(<Jobs />);
    await h.emit('job', { type: 'start', job: 3, index: 0, total: 1, file: 'c.mov', title: '→ MP4' });
    expect(rows()).toHaveLength(1);

    await h.click(document.querySelector('button'));
    expect(rows()).toHaveLength(0);
  });
});
