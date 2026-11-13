/**
 * The editor contract: preview-pixel boxes become source-pixel jobs, crop keeps its aspect,
 * detected boxes are redacted, annotations are composited at full size, the PDF organizer and the
 * join timeline send the order the user chose, and closing cancels a pending frame fetch.
 */

import { afterEach, describe, expect, it } from 'vitest';
import { cleanup } from '@testing-library/react';
import { Editor } from '../src/editor/Editor';
import { editorState, firstCall, flush, installTauri, mount, type Harness } from './harness';

const canvasOf = () => document.getElementById('canvas') as HTMLCanvasElement;
const err = () => document.getElementById('err')!.textContent ?? '';
const controls = () => document.getElementById('controls') as HTMLElement;
const byText = (root: Element, text: string) =>
  [...root.querySelectorAll('button')].find((b) => b.textContent === text) as HTMLButtonElement | undefined;

async function open(tool: string): Promise<Harness> {
  const h = installTauri({ editor_state: editorState(tool) });
  mount(<Editor />);
  await flush();
  return h;
}

describe('editor', () => {
  afterEach(() => {
    cleanup();
    delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
  });

  it('loads the preview into the canvas', async () => {
    await open('redact');
    expect(canvasOf().width).toBe(800);
  });

  it('sends redaction boxes in source pixels for the whole selection', async () => {
    const h = await open('redact');
    const cv = canvasOf();
    await h.dragBox(cv, [0.1, 0.1], [0.4, 0.3]);
    await h.dragBox(cv, [0.6, 0.6], [0.9, 0.9]);
    await h.click(document.getElementById('apply'));

    const run = firstCall(h.calls, 'run_job');
    const boxes = String(run?.args.opts.boxes).split(';');
    // Preview 800 px wide, source 1600 px: every value doubles.
    expect(boxes).toHaveLength(2);
    expect(boxes[0]).toBe('160,120,480,240');
    expect(run?.args.paths).toHaveLength(2);
    expect(run?.args.opts.style).toBe('blur');
  });

  it('keeps a chosen aspect ratio while cropping', async () => {
    const h = await open('crop');
    const cv = canvasOf();
    const select = controls().querySelector('select') as HTMLSelectElement;
    await h.input(select, '16:9', 'change');
    await h.dragBox(cv, [0.1, 0.1], [0.9, 0.9]);
    await h.click(document.getElementById('apply'));

    const run = firstCall(h.calls, 'run_job');
    const [, , w, hh] = String(run?.args.opts.box).split(',').map(Number);
    expect(Math.abs(w / hh - 16 / 9)).toBeLessThan(0.01);
  });

  it('explains why applying without a box does nothing', async () => {
    const h = await open('crop');
    await h.click(document.getElementById('apply'));
    expect(err()).toContain('draw a box');
    expect(firstCall(h.calls, 'run_job')).toBeUndefined();
  });

  it('redacts detected text boxes', async () => {
    const h = await open('redact');
    await h.click(byText(controls(), 'Detect text'));
    await h.click(document.getElementById('apply'));
    expect(firstCall(h.calls, 'run_job')?.args.opts.boxes).toBe('10,12,140,40');
  });

  it('redacts detected faces', async () => {
    const h = await open('redact');
    await h.click(byText(controls(), 'Detect faces'));
    await h.click(document.getElementById('apply'));
    expect(firstCall(h.calls, 'run_job')?.args.opts.boxes).toBe('200,100,120,120');
  });

  it('sends every adjust slider value', async () => {
    const h = await open('adjust');
    const saturation = controls().querySelectorAll('input')[2] as HTMLInputElement;
    await h.input(saturation, '1.6');
    await h.click(document.getElementById('apply'));

    const run = firstCall(h.calls, 'run_job');
    expect(run?.args.id).toBe('adjust');
    expect(run?.args.opts.saturation).toBe('1.6');
    expect(run?.args.opts.contrast).toBe('1');
  });

  it('reorders, rotates, removes and duplicates PDF pages', async () => {
    const h = await open('pages');
    expect((document.getElementById('pages') as HTMLElement).children).toHaveLength(3);
    expect(canvasOf().hidden).toBe(true);

    const cards = () => [...(document.getElementById('pages') as HTMLElement).children];
    const buttons = (i: number) => [...cards()[i].querySelectorAll('button')];

    await h.click(buttons(2)[0]); // move page 3 earlier -> 1,3,2
    await h.click(buttons(0)[1]); // rotate the first card
    await h.click(buttons(2)[3]); // remove the last (page 2)
    await h.click(buttons(0)[2]); // duplicate the first -> 1,1,3
    await h.click(document.getElementById('apply'));

    const run = firstCall(h.calls, 'run_job');
    expect(run?.args.opts.spec).toBe('1,1,3');
    expect(run?.args.opts.rotate).toBe('1:90,2:90');
  });

  it('labels merged pages by file and arranges them across PDFs', async () => {
    const h = await open('merge-pdf');
    const cards = () => [...(document.getElementById('pages') as HTMLElement).children];
    expect(cards()).toHaveLength(3);
    expect(cards()[2].querySelector('.page-n')!.textContent).toContain('b.pdf');

    await h.click(cards()[2].querySelectorAll('button')[0]); // b1 earlier -> a1, b1, a2
    await h.click(cards()[0].querySelectorAll('button')[0]); // no-op at the start
    await h.click(document.getElementById('apply'));

    const run = firstCall(h.calls, 'run_job');
    expect(run?.args.id).toBe('arrange');
    expect(run?.args.opts.items).toBe('1:1,2:1,1:2');
  });

  it('sends annotations as a full-size PNG layer', async () => {
    const h = await open('annotate');
    const cv = canvasOf();
    await h.dragBox(cv, [0.2, 0.2], [0.5, 0.5]);
    await h.click(controls().querySelectorAll('button')[3]); // number stamp
    await h.pointer(cv, 'pointerdown', 0.7, 0.7);
    await h.key('z'); // plain z is not undo
    await h.click(document.getElementById('apply'));

    const save = firstCall(h.calls, 'save_annotation');
    expect(save?.args.src).toBe('/home/u/p.png');
    expect(String(save?.args.data).startsWith('data:image/png;base64,')).toBe(true);
    expect(save?.args.width).toBe(1600);
    expect(save?.args.height).toBe(1200);
    expect(h.calls.some((c) => c.cmd === 'close_editor')).toBe(true);
  });

  it('shows the timeline, fetches scrub frames and sends a frame-exact range', async () => {
    const h = await open('trim');
    expect((document.getElementById('timeline') as HTMLElement).hidden).toBe(false);
    expect((document.getElementById('wave') as HTMLImageElement).src).toBe('data:image/png;base64,WAVE');
    expect((document.getElementById('saveFrame') as HTMLElement).hidden).toBe(true);
    expect((document.getElementById('setStart') as HTMLElement).hidden).toBe(false);

    const scrub = document.getElementById('scrub') as HTMLInputElement;
    await h.input(scrub, '2', 'change');
    await h.key('i');
    await h.key('ArrowRight'); // +1 frame (1/25 s)
    await h.key('i');
    await h.wait(150);
    const frame = firstCall(h.calls, 'frame_at');
    expect(frame).toBeDefined();
    expect(Math.abs(frame!.args.t - 2.04)).toBeLessThan(1e-6);

    await h.input(scrub, '7.5', 'change');
    await h.click(document.getElementById('setEnd'));
    expect(document.getElementById('clock')!.textContent).toBe('0:07.500');

    await h.click(document.getElementById('apply'));
    const run = firstCall(h.calls, 'run_job');
    expect(run?.args.id).toBe('trim');
    expect(run?.args.opts.start).toBe('2.040');
    expect(run?.args.opts.end).toBe('7.500');
  });

  it('leaves an untouched out point open', async () => {
    const h = await open('trim');
    const scrub = document.getElementById('scrub') as HTMLInputElement;
    await h.input(scrub, '3', 'change');
    await h.click(document.getElementById('setStart'));
    await h.click(document.getElementById('apply'));

    const run = firstCall(h.calls, 'run_job');
    expect(run?.args.opts.start).toBe('3.000');
    expect(run?.args.opts.end).toBeUndefined();
  });

  it('saves the frame under the playhead in snapshot mode', async () => {
    const h = await open('snapshot');
    expect((document.getElementById('saveFrame') as HTMLElement).hidden).toBe(false);
    expect((document.getElementById('setStart') as HTMLElement).hidden).toBe(true);

    const scrub = document.getElementById('scrub') as HTMLInputElement;
    await h.input(scrub, '4.2', 'change');
    await h.click(document.getElementById('saveFrame'));

    const run = firstCall(h.calls, 'run_job');
    expect(run?.args.id).toBe('snapshot');
    expect(run?.args.opts.at).toBe('4.200');
  });

  it('joins clips in the order the user arranged', async () => {
    const h = await open('join');
    const list = document.getElementById('clips') as HTMLElement;
    expect(list.hidden).toBe(false);
    expect(list.children).toHaveLength(3);
    expect(canvasOf().hidden).toBe(true);
    expect(document.getElementById('info')!.textContent).toBe('3 · 0:07.500');

    await h.click(list.children[2].querySelector('.page-tools')!.children[0]); // c earlier
    await h.click(list.children[1].querySelector('.page-tools')!.children[0]); // c first
    await h.click(list.children[2].querySelector('.page-tools')!.children[1]); // drop b
    await h.click(document.getElementById('apply'));

    const run = firstCall(h.calls, 'run_job');
    expect(run?.args.id).toBe('join');
    expect(run?.args.paths.join()).toBe('/u/c.mp4,/u/a.mp4');
    expect(h.calls.some((c) => c.cmd === 'close_editor')).toBe(true);
  });

  it('refuses to join a single clip', async () => {
    const h = await open('join');
    const list = document.getElementById('clips') as HTMLElement;
    await h.click(list.children[0].querySelector('.page-tools')!.children[1]);
    await h.click(list.children[0].querySelector('.page-tools')!.children[1]);
    await h.click(document.getElementById('apply'));

    expect(firstCall(h.calls, 'run_job')).toBeUndefined();
    expect(err()).toContain('two clips');
  });

  it('cancels a pending frame fetch when the editor closes', async () => {
    const h = await open('trim');
    const scrub = document.getElementById('scrub') as HTMLInputElement;
    await h.input(scrub, '5', 'change');
    await h.key('Escape');
    await h.wait(200);

    expect(h.calls.some((c) => c.cmd === 'frame_at')).toBe(false);
  });
});
