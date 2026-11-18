/**
 * The studio shell contract: the pending selection becomes a grid of conversions, settings round
 * trip through validation, recipes run with their saved steps, the rail switches views, the wheel's
 * rename action lands on the tool form, and the traffic lights / edge strips drive the real window.
 */

import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, waitFor } from '@testing-library/react';
import { version } from '../package.json';
import { Home } from '../src/home/Home';
import { firstCall, flush, installTauri, lastCall, mount, type Harness } from './harness';

const byId = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T | null;
const need = <T extends HTMLElement = HTMLElement>(id: string) => byId<T>(id)!;

async function open(): Promise<Harness> {
  const h = installTauri();
  mount(<Home />);
  await flush();
  return h;
}

describe('home', () => {
  afterEach(() => {
    cleanup();
    delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
  });

  it('shows the conversions for the pending selection and the engine table', async () => {
    await open();
    expect(need('grid').children).toHaveLength(10);
    expect(need('engines').children).toHaveLength(2);
    expect(need('view-convert').hidden).toBe(false);
  });

  it('offers an install button only for the engines that are missing', async () => {
    const h = await open();
    const row = (id: string) => need('engines').querySelector(`[data-engine="${id}"]`);
    expect(row('vips')).not.toBeNull();

    await (row('vips') as HTMLButtonElement).click();
    expect(firstCall(h.calls, 'install_engine')?.args.id).toBe('vips');
    // FFmpeg was found, so it has no dead-end button.
    expect(row('ffmpeg')).toBeNull();
  });

  it('supports on-demand SHA-256 engine download in Lite mode', async () => {
    const h = installTauri({
      build_profile: 'lite',
      doctor: [
        { id: 'ffmpeg', name: 'FFmpeg', path: '/usr/bin/ffmpeg', hint: '', tier: 'core', source: 'bundled' },
        {
          id: 'whisper',
          name: 'whisper.cpp',
          path: null,
          hint: 'build whisper.cpp',
          tier: 'optional',
          downloadable: true,
          size_bytes: 10364195,
          source: 'missing',
        },
      ],
      download_engine: '/home/u/.local/share/zeidex/engines/whisper/whisper-cli',
    });
    mount(<Home />);
    await flush();

    expect(need('buildProfileBadge').textContent).toContain('Lite');
    const dlBtn = need('engines').querySelector('[data-download-engine="whisper"]') as HTMLButtonElement;
    expect(dlBtn).not.toBeNull();
    await h.click(dlBtn);
    expect(firstCall(h.calls, 'download_engine')?.args.id).toBe('whisper');
    expect(need('integrated').textContent).toContain('SHA-256');
  });

  it('keeps every automation id the previous build exposed', async () => {
    await open();
    const ids = [
      'grid', 'tabs', 'form', 'sel', 'choose', 'files', 'drop',
      'view-convert', 'view-shelf', 'view-watch', 'view-settings',
      'navConvert', 'navTools', 'navShelf', 'navWatch', 'navSettings', 'railNav',
      'winClose', 'winMin', 'winMax', 'viewTitle', 'hook',
      'engines', 'enginePanel', 'integrate', 'integrated',
      'settings', 'settingsMsg', 'shelfList', 'shelfAdd', 'shelfOpen', 'shelfClear',
      'watchList', 'watchEdit', 'langToggle', 'donateBtn', 'aboutBtn',
    ];
    const missing = ids.filter((id) => byId(id) === null);
    expect(missing).toEqual([]);
  });

  it('round-trips settings, recipes and watch rules', async () => {
    const h = await open();
    await waitFor(() => expect(document.querySelector('select[name="tools"]')).not.toBeNull());

    const box = need<HTMLFormElement>('settings');
    const tools = box.querySelector('select[name="tools"]') as HTMLSelectElement;

    // Duplicate wheel keys are rejected by the backend and surfaced in the message line.
    tools.value = 'Shift';
    await h.click(box.querySelector('button[type="submit"]'));
    expect(need('settingsMsg').textContent).toContain('own key');

    tools.value = 'Meta';
    const lists = box.querySelectorAll('.list');
    expect(lists).toHaveLength(2);

    // Add a recipe and type into it.
    await h.click(lists[0].children[2]);
    const rows = lists[0].children[1].children;
    expect(rows).toHaveLength(2);
    (rows[1].querySelector('.r-name') as HTMLInputElement).value = 'Thumbs';
    (rows[1].querySelector('.r-steps') as HTMLInputElement).value = 'resize max=300 | strip';

    // Remove the saved watch rule, add a new one through the folder picker.
    await h.click(lists[1].children[1].children[0].querySelectorAll('button')[1]);
    expect(lists[1].children[1].children).toHaveLength(0);
    await h.click(lists[1].children[2]);
    const wrow = lists[1].children[1].children[0];
    await h.click(wrow.querySelector('button'));
    expect((wrow.querySelector('.w-dir') as HTMLInputElement).value).toBe('/home/u/new');
    (wrow.querySelector('.w-steps') as HTMLInputElement).value = 'convert to=webp';

    await h.click(box.querySelector('button[type="submit"]'));

    const saved = lastCall(h.calls, 'set_settings')?.args.settings;
    expect(saved.tools).toBe('Meta');
    expect(saved.reveal).toBe(true);
    // Both floating surfaces are opt-in: the installer must not leave a tray icon or an orb on
    // the desktop, so the unchecked boxes have to travel as `false`.
    expect(saved.tray).toBe(false);
    expect(saved.orb).toBe(false);
    expect(saved.background).toBe(true);
    expect(saved.recipes).toHaveLength(2);
    expect(saved.recipes[1]).toMatchObject({ name: 'Thumbs', steps: 'resize max=300 | strip' });
    expect(saved.watch).toHaveLength(1);
    expect(saved.watch[0]).toMatchObject({ dir: '/home/u/new', steps: 'convert to=webp' });
    expect(need('settingsMsg').textContent).toContain('Saved');
  });

  it('runs a saved recipe with its steps', async () => {
    const h = await open();
    await h.click(need('tabs').children[1]);
    const button = [...need('grid').children].find((b) => b.textContent === '★ Web image') as HTMLElement;
    await h.click(button);

    const run = firstCall(h.calls, 'run_job');
    expect(run?.args.id).toBe('recipe');
    expect(String(run?.args.opts.steps).startsWith('resize max=1600')).toBe(true);
  });

  it('opens and closes the about and donate dialogs, and the developer link', async () => {
    const h = await open();

    await h.click(need('aboutBtn'));
    expect((need<HTMLDialogElement>('aboutModal')).open).toBe(true);
    await h.click(need('devProfileBtn'));
    expect(firstCall(h.calls, 'open_url')?.args.url).toBe('https://zeidexlink.zeidexsoft.com/zeidex');
    // The version shown is the package's own, not a string that goes stale.
    expect(need('aboutModal').textContent).toContain(`Version ${version}`);
    await h.click(need('closeAboutBtn'));
    expect(byId('aboutModal')).toBeNull();

    await h.click(need('aboutBtn'));
    await h.key('Escape');
    expect(byId('aboutModal')).toBeNull();

    await h.click(need('donateBtn'));
    expect((need<HTMLDialogElement>('donateModal')).open).toBe(true);
    await h.click(need('closeDonateBtn'));
    expect(byId('donateModal')).toBeNull();
  });

  it('switches language and direction instantly', async () => {
    const h = await open();
    const before = document.documentElement.lang;
    await h.click(need('langToggle'));
    expect(document.documentElement.lang).not.toBe(before);
    expect(document.documentElement.dir).toBe('rtl');
    await h.click(need('langToggle'));
    expect(document.documentElement.lang).toBe(before);
    expect(document.documentElement.dir).toBe('ltr');
  });

  it('switches views from the rail', async () => {
    const h = await open();

    await h.click(need('navShelf'));
    expect(need('view-shelf').hidden).toBe(false);
    expect(need('view-convert').hidden).toBe(true);
    await waitFor(() => expect(need('shelfList').children).toHaveLength(2));

    await h.click(need('navWatch'));
    await waitFor(() => expect(need('watchList').children).toHaveLength(1));

    await h.click(need('navTools'));
    expect(need('view-convert').hidden).toBe(false);
    expect(need('tabs').children[1].className).toContain('on');

    await h.click(need('navSettings'));
    expect(need('view-settings').hidden).toBe(false);
  });

  it('routes the wheel’s rename action to the tool form', async () => {
    const h = await open();
    await h.emit('open-tool', 'rename');

    expect(need('tabs').children[1].className).toContain('on');
    expect(byId('form')).not.toBeNull();
  });

  it('drives the real window from the traffic lights and the edge strips', async () => {
    const h = await open();

    await h.click(need('winMin'));
    await h.click(need('winMax'));
    const east = document.querySelector('[data-resize="East"]')!;
    await h.press(east);
    await h.click(need('winClose'));

    expect(h.calls.some((c) => c.cmd === 'winMinimize')).toBe(true);
    expect(h.calls.some((c) => c.cmd === 'winMaximize')).toBe(true);
    expect(h.calls.some((c) => c.cmd === 'winResize' && c.args === 'East')).toBe(true);
    expect(h.calls.some((c) => c.cmd === 'winClose')).toBe(true);
  });

  it('customizes wheel favorites and exports/imports recipes JSON', async () => {
    const h = installTauri({
      export_recipes: '{"version":1,"recipes":[{"name":"Web","steps":"strip"}]}',
      import_recipes: [{ name: 'Imported', steps: 'compress percent=50 | strip' }],
    });
    mount(<Home />);
    await flush();

    await h.click(need('addFavoriteBtn'));
    await h.click(need('exportRecipesBtn'));
    expect(h.calls.some((c) => c.cmd === 'export_recipes')).toBe(true);

    await h.click(need('importRecipesToggleBtn'));
    const area = need<HTMLTextAreaElement>('importRecipesText');
    area.value = '{"version":1,"recipes":[{"name":"Imported","steps":"compress percent=50 | strip"}]}';
    await h.input(area, area.value);
    await h.click(need('importRecipesApplyBtn'));
    expect(h.calls.some((c) => c.cmd === 'import_recipes')).toBe(true);
  });

  it('runs deep engine health checks, lists models, downloads missing, and opens Error Report modal', async () => {
    const h = installTauri({
      build_profile: 'lite',
      doctor: [
        {
          id: 'ffmpeg',
          name: 'FFmpeg',
          path: '/usr/bin/ffmpeg',
          hint: '',
          version: 'ffmpeg 8.0',
          disk_bytes: 52000000,
          source: 'system',
          health: 'installed',
        },
        {
          id: 'whisper',
          name: 'whisper.cpp',
          path: null,
          hint: 'build whisper.cpp',
          downloadable: true,
          size_bytes: 10364195,
          source: 'missing',
          health: 'missing',
        },
      ],
      test_all_engines: {
        os: 'linux',
        arch: 'x86_64',
        app_version: '0.3.5',
        profile: 'lite',
        engines: [
          {
            id: 'ffmpeg',
            name: 'FFmpeg',
            path: '/usr/bin/ffmpeg',
            hint: '',
            version: 'ffmpeg 8.0',
            disk_bytes: 52000000,
            source: 'system',
            health: 'healthy',
            exit_code: 0,
          },
          {
            id: 'whisper',
            name: 'whisper.cpp',
            path: '/engines/whisper/whisper-cli',
            hint: '',
            downloadable: true,
            source: 'local',
            health: 'broken',
            exit_code: 127,
            stderr: 'error while loading shared libraries',
          },
        ],
        models: [
          { id: 'tesseract-eng', label: 'Tesseract English (eng.traineddata)', label_ar: 'بيانات eng.traineddata', present: true, path: '/usr/share/tessdata/eng.traineddata', disk_bytes: 4000000 },
        ],
        report_text: 'Zeidex Convert Diagnostic & Error Report\n[BROKEN ] whisper.cpp',
      },
    });
    mount(<Home />);
    await flush();

    // Auto-detected version, disk size, and model badge are rendered automatically.
    expect(need('engines').textContent).toContain('ffmpeg 8.0');
    expect(need('engines').textContent).toContain('52.0 MB');
    expect(need('modelsSummary').textContent).toContain('eng.traineddata');

    // Download All Missing works when at least one downloadable missing engine exists.
    await h.click(need('downloadAllEnginesBtn'));
    expect(h.calls.some((c) => c.cmd === 'download_all_missing')).toBe(true);

    // Run Test All Engines smoke test -> marks ffmpeg Healthy and broken whisper Reinstall/Repair.
    await h.click(need('testEnginesBtn'));
    expect(h.calls.some((c) => c.cmd === 'test_all_engines')).toBe(true);
    expect(need('engines').textContent).toContain('Healthy / Working');
    expect(need('engines').textContent).toContain('Broken / Error');
    expect(need('engines').querySelector('[data-repair-engine="whisper"]')).not.toBeNull();

    // View Error Report modal displays highlighted button + dir="ltr" log + save & copy actions.
    expect(need('viewEngineReportBtn').className).toContain('bad');
    await h.click(need('viewEngineReportBtn'));
    expect(need('diagnosticReportLog').getAttribute('dir')).toBe('ltr');
    expect(need('diagnosticReportLog').textContent).toContain('[BROKEN ] whisper.cpp');
    await h.click(need('saveErrorReportBtn'));
    expect(h.calls.some((c) => c.cmd === 'save_diagnostic_report')).toBe(true);
  });
});
