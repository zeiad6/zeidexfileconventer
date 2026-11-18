/**
 * Studio shell: the click-based version of the wheel for when dragging is awkward, engine status,
 * the floating shelf, watch folders, settings and file-manager integration.
 *
 * Every id the previous plain-JS build exposed (`#grid`, `#tabs`, `#form`, `#sel`, `#view-*`,
 * `#nav*`, `#win*`, the resize strips) is kept: those ids are the automation contract the UI tests
 * and external tooling rely on.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { ErrorReportModal } from '../components/ErrorReportModal';
import { ParamsForm } from '../components/ParamsForm';
import {
  appWindow,
  buildProfile,
  describe as describePaths,
  diagnosticReport,
  doctor,
  downloadAllMissing,
  downloadEngine,
  installEngine,
  downloadDraggedUrl,
  floatAdd,
  floatClear,
  floatList,
  getSettings,
  hookOk,
  integrateShell,
  modelsStatus,
  onDragDrop,
  openEditor,
  openInHome,
  pickFiles,
  runJob,
  safeListen,
  takePending,
  testAllEngines,
} from '../lib/api';
import { useI18n } from '../lib/i18n';
import { MODES } from '../lib/geometry';
import type {
  Choice,
  Desc,
  DiagnosticsReport,
  Engine,
  EngineDownloadProgress,
  Mode,
  ModelStatus,
  WatchRule,
} from '../lib/types';
import { AboutModal, DonateModal } from './Modals';
import { SettingsView } from './SettingsView';

type View = 'convert' | 'tools' | 'shelf' | 'watch' | 'settings';

const VIEW_IDS: Record<View, string> = {
  convert: 'navConvert',
  tools: 'navTools',
  shelf: 'navShelf',
  watch: 'navWatch',
  settings: 'navSettings',
};

const VIEW_TITLES: Record<View, string> = {
  convert: 'navConvert',
  tools: 'tools',
  shelf: 'navShelf',
  watch: 'watchFolders',
  settings: 'settings',
};

const TABS: Mode[] = ['convert', 'tools', 'actions'];

const RESIZE_STRIPS = ['North', 'South', 'East', 'West', 'NorthEast', 'NorthWest', 'SouthEast', 'SouthWest'];

const base = (p: string) => p.split(/[\\/]/).pop() ?? p;

function formatBytes(bytes?: number | null): string {
  if (!bytes || bytes <= 0) return '';
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  if (bytes >= 1e3) return `${Math.round(bytes / 1e3)} KB`;
  return `${bytes} B`;
}

const IS_MAC = /Mac/i.test(navigator.platform || navigator.userAgent);

export function Home() {
  const { t, zl, toggle, ar } = useI18n();
  const [paths, setPaths] = useState<string[]>([]);
  const [desc, setDesc] = useState<Desc | null>(null);
  const [mode, setMode] = useState<Mode>('convert');
  const [view, setView] = useState<View>('convert');
  const [form, setForm] = useState<Choice | null>(null);
  const [dropOver, setDropOver] = useState(false);
  const [hookKey, setHookKey] = useState<'hookOn' | 'hookOff' | 'selectionHint' | null>(null);
  const [engines, setEngines] = useState<Engine[]>([]);
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [profile, setProfile] = useState<'lite' | 'full' | null>(null);
  const [downloading, setDownloading] = useState<Record<string, EngineDownloadProgress>>({});
  const [downloadingAll, setDownloadingAll] = useState(false);
  const [testingEngines, setTestingEngines] = useState(false);
  const [reportOpen, setReportOpen] = useState(false);
  const [reportLoading, setReportLoading] = useState(false);
  const [diagReport, setDiagReport] = useState<DiagnosticsReport | null>(null);
  const [integrated, setIntegrated] = useState('');
  const [shelfFiles, setShelfFiles] = useState<string[]>([]);
  const [watchRules, setWatchRules] = useState<WatchRule[]>([]);
  const [flashWatch, setFlashWatch] = useState(false);
  const [about, setAbout] = useState(false);
  const [donate, setDonate] = useState(false);
  const [runError, setRunError] = useState('');

  const pathsRef = useRef(paths);
  pathsRef.current = paths;
  const descRef = useRef(desc);
  descRef.current = desc;
  const modeRef = useRef(mode);
  modeRef.current = mode;
  const viewRef = useRef(view);
  viewRef.current = view;
  // Tool a wheel action asked this window to open (rename lands here, where the pattern has a
  // live preview instead of a pattern the user never saw).
  const wantedTool = useRef<string | null>(null);

  const choose = useCallback((it: Choice) => {
    if (it.editor) {
      void openEditor(pathsRef.current, it.id);
      return;
    }
    if (!it.params.length) {
      setRunError('');
      void runJob(pathsRef.current, it.kind, it.id, it.preset ?? {}).catch((e) => setRunError(String(e)));
      return;
    }
    setRunError('');
    setForm(it);
  }, []);

  const pickTool = useCallback(
    (id: string, described?: Desc | null) => {
      const d = described ?? descRef.current;
      setView('tools');
      if (d?.tools.length) setMode('tools');
      const it = d?.tools.find((c) => c.id === id);
      if (it) {
        wantedTool.current = null;
        choose(it);
      } else if (!d) {
        wantedTool.current = id;
      }
    },
    [choose],
  );

  const load = useCallback(
    async (next: string[]) => {
      if (!next.length) return;
      setRunError('');
      const d = await describePaths(next);
      setPaths(next);
      setDesc(d);
      const fallback = MODES.find((m) => d[m].length) ?? 'convert';
      setMode(d[modeRef.current]?.length ? modeRef.current : fallback);
      if (viewRef.current !== 'convert' && viewRef.current !== 'tools') setView('convert');
      const wanted = wantedTool.current;
      if (wanted) pickTool(wanted, d);
    },
    [pickTool],
  );

  const showView = useCallback((next: View) => {
    setView(next);
    if (next === 'tools' && descRef.current?.tools.length) setMode('tools');
    setForm(null);
  }, []);

  // Native drag-and-drop: highlight the drop zone, then describe what was dropped.
  useEffect(
    () =>
      onDragDrop((p) => {
        setDropOver(p.type === 'enter' || p.type === 'over');
        if (p.type === 'drop') void load(p.paths);
      }),
    [load],
  );

  // Backend events that make this window follow the rest of the app.
  useEffect(() => {
    const offs = [
      safeListen<string>('open-tool', (id) => pickTool(id)),
      safeListen<void>('open-files', () => {
        void takePending().then(load);
      }),
      safeListen<void>('hook-error', () => setHookKey('hookOff')),
      // Ctrl+Shift+Space is also used by other apps: never pop up, only explain.
      safeListen<void>('selection-empty', () => setHookKey('selectionHint')),
      safeListen<EngineDownloadProgress>('engine-download', (ev) => {
        setDownloading((prev) => {
          const next = { ...prev };
          if (ev.stage === 'done' || ev.stage === 'failed') {
            delete next[ev.id];
          } else {
            next[ev.id] = ev;
          }
          return next;
        });
        if (ev.stage === 'done') {
          void doctor().then(setEngines).catch(() => {});
          void modelsStatus().then((m) => Array.isArray(m) && setModels(m)).catch(() => {});
          void buildProfile().then((p) => p && setProfile(p)).catch(() => {});
        }
      }),
    ];
    return () => offs.forEach((off) => off());
  }, [load, pickTool]);

  const refreshEngines = useCallback(() => {
    void doctor().then(setEngines).catch(() => {});
    void modelsStatus().then((m) => Array.isArray(m) && setModels(m)).catch(() => {});
    void buildProfile().then((p) => p && setProfile(p)).catch(() => {});
  }, []);

  // The shelf, watch summary, and engine diagnostics scan when their view opens.
  useEffect(() => {
    if (view === 'shelf') {
      floatList()
        .then(setShelfFiles)
        .catch(() => {});
    } else if (view === 'watch') {
      getSettings()
        .then((s) => setWatchRules(s.watch ?? []))
        .catch(() => {});
    } else if (view === 'settings') {
      refreshEngines();
    }
  }, [view, refreshEngines]);

  useEffect(() => {
    let alive = true;
    hookOk()
      .then((ok) => alive && setHookKey(ok ? 'hookOn' : 'hookOff'))
      .catch(() => {});
    doctor()
      .then((rows) => alive && setEngines(rows))
      .catch(() => {});
    modelsStatus()
      .then((m) => alive && Array.isArray(m) && setModels(m))
      .catch(() => {});
    buildProfile()
      .then((p) => alive && p && setProfile(p))
      .catch(() => {});
    takePending()
      .then((pending) => {
        if (alive) void load(pending);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [load]);

  const filesLine = paths.length
    ? `${paths.length} ${t('selected')}: ${paths
        .map(base)
        .slice(0, 6)
        .join(', ')}${paths.length > 6 ? '…' : ''}`
    : '';

  const list = desc ? desc[mode] : [];

  const newWatchRule = () => {
    setView('settings');
    setFlashWatch(true);
    window.setTimeout(() => setFlashWatch(false), 900);
  };

  /** Window controls are no-ops outside Tauri (a browser preview of the UI). */
  const control = (action: 'close' | 'minimize' | 'toggleMaximize') => {
    try {
      const win = appWindow();
      void win[action]();
    } catch {
      /* browser preview */
    }
  };
  const resize = (dir: string) => {
    try {
      void appWindow().startResizeDragging(dir);
    } catch {
      /* browser preview */
    }
  };

  return (
    <div className="studio">
      <header className="topbar" data-tauri-drag-region="">
        <div className="lights">
          <button type="button" className="light close" id="winClose" aria-label={t('close')} title={t('close')} onClick={() => control('close')} />
          <button type="button" className="light min" id="winMin" aria-label={t('minimize')} title={t('minimize')} onClick={() => control('minimize')} />
          <button type="button" className="light max" id="winMax" aria-label={t('maximize')} title={t('maximize')} onClick={() => control('toggleMaximize')} />
        </div>
        <div className="topbar-lead" data-tauri-drag-region="">
          <span className="topbar-title" id="viewTitle">
            {t(VIEW_TITLES[view])}
          </span>
          <span className="hint" id="hook">
            {hookKey ? t(hookKey === 'hookOff' && IS_MAC ? 'hookOffMac' : hookKey) : ''}
          </span>
        </div>
        <div className="header-actions">
          <button type="button" className="btn-subtle btn-donate" id="donateBtn" onClick={() => setDonate(true)}>
            <span className="icon">❤️</span> <span>{t('donate')}</span>
          </button>
          <button type="button" className="btn-subtle" id="aboutBtn" onClick={() => setAbout(true)}>
            <span className="icon">ℹ️</span> <span>{t('about')}</span>
          </button>
        </div>
      </header>
      <div className="studio-body">
      <aside className="rail" aria-label={t('sectionsNav')}>
        <div className="rail-head">
          <img src="/icon.png" alt="" className="app-icon" />
          <div className="rail-title">
            <h1>Zeidex</h1>
            <span className="hint">{t('sub')}</span>
          </div>
        </div>
        <nav
          className="rail-nav"
          id="railNav"
          aria-label={t('sectionsNav')}
          onKeyDown={(e) => {
            if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
            const btns = [...e.currentTarget.querySelectorAll<HTMLButtonElement>('button')];
            const idx = btns.indexOf(document.activeElement as HTMLButtonElement);
            if (idx < 0) return;
            e.preventDefault();
            const next = e.key === 'ArrowDown' ? (idx + 1) % btns.length : (idx - 1 + btns.length) % btns.length;
            btns[next]?.focus();
          }}
        >
          {(Object.keys(VIEW_IDS) as View[]).map((name) => (
            <button
              key={name}
              type="button"
              className={`rail-btn${view === name ? ' on' : ''}`}
              id={VIEW_IDS[name]}
              data-view={name}
              aria-current={view === name ? 'page' : undefined}
              onClick={() => showView(name)}
            >
              {t(VIEW_TITLES[name])}
            </button>
          ))}
        </nav>
        <div className="rail-foot">
          <button type="button" className="btn-subtle" id="langToggle" title="Switch language" onClick={toggle}>
            {t('langSwitch')}
          </button>
        </div>
      </aside>

      <main className="stage">

        <div className="views">
          <section className="view" id="view-convert" hidden={view !== 'convert' && view !== 'tools'}>
            <section
              className={`drop${dropOver ? ' over' : ''}`}
              id="drop"
              onDragOver={(e) => {
                e.preventDefault();
                setDropOver(true);
              }}
              onDragLeave={() => setDropOver(false)}
              onDrop={(e) => {
                e.preventDefault();
                setDropOver(false);
                const data = e.dataTransfer;
                const html = data?.getData('text/html') ?? '';
                const uri = data?.getData('text/uri-list') ?? '';
                let url: string | undefined;
                const img = html.match(/<img[^>]+src=["']([^"']+)["']/i);
                if (img) url = img[1];
                if (!url && uri) url = uri.split('\n').map((s) => s.trim()).find((s) => s && !s.startsWith('#'));
                if (url && (url.startsWith('http://') || url.startsWith('https://'))) {
                  // A file dragged out of a browser only exists as a URL: download it, then treat
                  // the local path like any other dropped file.
                  downloadDraggedUrl(url)
                    .then((local) => {
                      if (local) return load([local]);
                      return undefined;
                    })
                    .catch((err) => console.warn('browser drag download failed', err));
                }
              }}
            >
              <p>{t('drop')}</p>
              <button
                type="button"
                className="primary"
                id="choose"
                onClick={() => {
                  void pickFiles().then(load);
                }}
              >
                {t('choose')}
              </button>
            </section>

            <section className="sel" id="sel" hidden={!paths.length}>
              <div className="files" id="files">
                {filesLine}
              </div>
              <div
                className="tabs"
                id="tabs"
                role="tablist"
                aria-label={t('convert')}
                onKeyDown={(e) => {
                  const rtl = document.documentElement.dir === 'rtl';
                  const forward = rtl ? 'ArrowLeft' : 'ArrowRight';
                  const back = rtl ? 'ArrowRight' : 'ArrowLeft';
                  if (e.key !== forward && e.key !== back) return;
                  const tabs = [...e.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]:not(:disabled)')];
                  const idx = tabs.indexOf(document.activeElement as HTMLButtonElement);
                  if (idx < 0 || !tabs.length) return;
                  e.preventDefault();
                  const next = e.key === forward ? (idx + 1) % tabs.length : (idx - 1 + tabs.length) % tabs.length;
                  tabs[next]?.focus();
                  tabs[next]?.click();
                }}
              >
                {TABS.map((m) => (
                  <button
                    key={m}
                    type="button"
                    role="tab"
                    aria-selected={mode === m}
                    tabIndex={mode === m ? 0 : -1}
                    className={m === mode ? 'on' : ''}
                    disabled={!desc?.[m].length}
                    onClick={() => {
                      setForm(null);
                      setMode(m);
                    }}
                  >
                    {t(m)}
                  </button>
                ))}
              </div>
              <div className="grid" id="grid">
                {list.length ? (
                  list.map((it) => (
                    <button key={`${it.kind}-${it.id}`} type="button" data-id={it.id} onClick={() => choose(it)}>
                      {zl(it)}
                    </button>
                  ))
                ) : (
                  <p className="hint">{desc ? t('nothing') : ''}</p>
                )}
              </div>
              <p className="err run-error" id="runError" role="alert">
                {runError}
              </p>
              {form ? (
                <ParamsForm
                  id="form"
                  variant="inline"
                  choice={form}
                  onCancel={() => setForm(null)}
                  onSubmit={async (opts) => {
                    await runJob(pathsRef.current, form.kind, form.id, opts);
                    setForm(null);
                  }}
                />
              ) : (
                /* The previous build shipped `#form` in the HTML and toggled `hidden`; scripts
                   in the wild still look it up, so it stays mounted even with nothing open. */
                <form id="form" className="inline-form" hidden />
              )}
            </section>
          </section>

          <section className="view" id="view-shelf" hidden={view !== 'shelf'}>
            <h2>{t('navShelf')}</h2>
            <p className="hint">{t('shelfHint')}</p>
            <div className="row">
              <button
                type="button"
                id="shelfAdd"
                onClick={async () => {
                  const picked = await pickFiles();
                  if (picked.length) await floatAdd(picked);
                  setShelfFiles(await floatList());
                }}
              >
                {t('choose')}
              </button>
              <button
                type="button"
                id="shelfOpen"
                onClick={async () => {
                  const files = await floatList();
                  if (files.length) await openInHome(files);
                }}
              >
                {t('openAll')}
              </button>
              <button
                type="button"
                id="shelfClear"
                onClick={async () => {
                  await floatClear();
                  setShelfFiles(await floatList());
                }}
              >
                {t('clear')}
              </button>
            </div>
            <ul className="shelf-list" id="shelfList">
              {shelfFiles.length ? (
                shelfFiles.map((path) => (
                  <li key={path}>
                    <span className="name" title={path}>
                      {base(path)}
                    </span>
                    <button type="button" onClick={() => void openInHome([path])}>
                      {t('open')}
                    </button>
                  </li>
                ))
              ) : (
                <li className="hint">{t('shelfEmpty')}</li>
              )}
            </ul>
          </section>

          <section className="view" id="view-watch" hidden={view !== 'watch'}>
            <h2>{t('watchFolders')}</h2>
            <p className="hint">{t('watchHint')}</p>
            <div className="watch-list" id="watchList">
              {watchRules.length ? (
                watchRules.map((rule) => (
                  <div className="watch-item" key={`${rule.dir}:${rule.steps}`}>
                    <span className="dir" title={rule.dir}>
                      {rule.dir || '—'}
                    </span>
                    <code>{`${rule.exts || '*'} → ${rule.steps || '—'}`}</code>
                    <span className={rule.enabled ? 'ok' : 'bad'}>{t(rule.enabled ? 'on' : 'off')}</span>
                  </div>
                ))
              ) : (
                <p className="hint">{t('watchEmpty')}</p>
              )}
            </div>
            <div className="row">
              <button type="button" id="watchEdit" onClick={newWatchRule}>
                {t('editWatch')}
              </button>
            </div>
          </section>

          <section className="view" id="view-settings" hidden={view !== 'settings'}>
            <section className="panel">
              <h2>{t('settings')}</h2>
              <SettingsView flashWatch={flashWatch} />
            </section>

            <section className="panel" id="enginePanel">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="flex flex-wrap items-center gap-2">
                  <h2>{t('engines')}</h2>
                  {profile && (
                    <span className="hint" id="buildProfileBadge">
                      {t(profile === 'lite' ? 'profileLite' : 'profileFull')}
                    </span>
                  )}
                </div>
                <div className="flex flex-wrap gap-1.5">
                  <button type="button" id="refreshEnginesBtn" onClick={refreshEngines}>
                    {t('refreshEngines')}
                  </button>
                  <button
                    type="button"
                    id="testEnginesBtn"
                    disabled={testingEngines}
                    onClick={async () => {
                      try {
                        setTestingEngines(true);
                        const rep = await testAllEngines();
                        setDiagReport(rep);
                        setEngines(rep.engines);
                        setModels(rep.models);
                        setProfile(rep.profile);
                      } catch (err) {
                        setIntegrated(String(err));
                      } finally {
                        setTestingEngines(false);
                      }
                    }}
                  >
                    {testingEngines ? t('testingEngines') : t('testAllEngines')}
                  </button>
                  {engines.some((e) => !e.path && e.downloadable) && (
                    <button
                      type="button"
                      id="downloadAllEnginesBtn"
                      disabled={downloadingAll}
                      onClick={async () => {
                        try {
                          setDownloadingAll(true);
                          const installed = await downloadAllMissing();
                          if (installed.length) {
                            setIntegrated(`${t('engineDownloadedOk')} ${installed.join(', ')}`);
                          }
                          refreshEngines();
                        } catch (err) {
                          setIntegrated(String(err));
                        } finally {
                          setDownloadingAll(false);
                        }
                      }}
                    >
                      {downloadingAll ? t('downloading') : t('downloadAllMissing')}
                    </button>
                  )}
                  <button
                    type="button"
                    id="viewEngineReportBtn"
                    className={engines.some((e) => e.health === 'broken') ? 'border-bad/60 bad' : undefined}
                    onClick={async () => {
                      setReportOpen(true);
                      if (!diagReport) {
                        try {
                          setReportLoading(true);
                          const rep = await diagnosticReport();
                          setDiagReport(rep);
                          setEngines(rep.engines);
                          setModels(rep.models);
                        } catch (err) {
                          setIntegrated(String(err));
                        } finally {
                          setReportLoading(false);
                        }
                      }
                    }}
                  >
                    {t('viewErrorReport')}
                  </button>
                </div>
              </div>
              <table className="engines">
                <tbody id="engines">
                  {engines.map((engine) => {
                    const dl = downloading[engine.id];
                    const mb = engine.size_bytes ? ` (${Math.max(1, Math.round(engine.size_bytes / 1e6))} MB)` : '';
                    const diskLabel = formatBytes(engine.disk_bytes);
                    const statusKey =
                      engine.health === 'healthy'
                        ? 'statusHealthy'
                        : engine.health === 'broken'
                          ? 'statusBroken'
                          : engine.path
                            ? 'statusInstalled'
                            : 'statusMissing';
                    const statusClass =
                      engine.health === 'broken' ? 'bad' : engine.path && engine.health !== 'missing' ? 'ok' : 'bad';
                    const statusIcon =
                      engine.health === 'healthy'
                        ? '🟢'
                        : engine.health === 'broken'
                          ? '🔴'
                          : engine.path
                            ? '✓'
                            : '🟡';
                    const dlLabel = dl
                      ? dl.stage === 'extracting'
                        ? t('extracting')
                        : dl.percent != null
                          ? `${dl.percent}% · ${formatBytes(dl.speed_bps)}/s`
                          : t('downloading')
                      : `${t('downloadEngine')}${mb}`;
                    const triggerDownload = async () => {
                      try {
                        setDownloading((prev) => ({ ...prev, [engine.id]: { id: engine.id, stage: 'downloading' } }));
                        const installedPath = await downloadEngine(engine.id);
                        setIntegrated(`${t('engineDownloadedOk')} ${installedPath}`);
                        refreshEngines();
                      } catch (err) {
                        setIntegrated(String(err));
                      } finally {
                        setDownloading((prev) => {
                          const next = { ...prev };
                          delete next[engine.id];
                          return next;
                        });
                      }
                    };
                    return (
                      <tr key={engine.id} data-health={engine.health ?? (engine.path ? 'installed' : 'missing')}>
                        <td className={statusClass} title={t(statusKey)}>
                          {statusIcon}
                        </td>
                        <td>
                          <div>{engine.name}</div>
                          <div className="hint text-[11px]">
                            <span className={statusClass}>{t(statusKey)}</span>
                            {engine.tier && ` · ${t(engine.tier === 'core' ? 'engineCore' : 'engineOptional')}`}
                            {diskLabel && ` · ${diskLabel}`}
                          </div>
                          {engine.version && (
                            <div className="hint text-[11px]" dir="ltr">
                              {engine.version}
                            </div>
                          )}
                        </td>
                        <td>
                          {engine.path ? (
                            <>
                              <code dir="ltr">{engine.path}</code>
                              {engine.source && engine.source !== 'missing' && (
                                <span className="hint">
                                  {' '}
                                  ({t(
                                    engine.source === 'bundled'
                                      ? 'engineBundled'
                                      : engine.source === 'local'
                                        ? 'engineLocal'
                                        : 'engineSystem',
                                  )})
                                </span>
                              )}
                              {engine.health_detail && (
                                <div className={engine.health === 'broken' ? 'bad text-[11px]' : 'hint text-[11px]'}>
                                  {engine.health_detail}
                                </div>
                              )}
                            </>
                          ) : (
                            <code dir="ltr">{engine.hint}</code>
                          )}
                        </td>
                        <td>
                          <div className="flex flex-wrap gap-1.5">
                            {!engine.path && engine.downloadable && (
                              <button
                                type="button"
                                className="install"
                                data-download-engine={engine.id}
                                disabled={Boolean(dl)}
                                onClick={triggerDownload}
                              >
                                {dlLabel}
                              </button>
                            )}
                            {engine.path && engine.downloadable && (engine.health === 'broken' || engine.source === 'local') && (
                              <button
                                type="button"
                                className="install"
                                data-repair-engine={engine.id}
                                disabled={Boolean(dl)}
                                onClick={triggerDownload}
                              >
                                {dl ? dlLabel : t('reinstallEngine')}
                              </button>
                            )}
                            {!engine.path && (
                              <button
                                type="button"
                                className="install"
                                data-engine={engine.id}
                                onClick={async () => {
                                  try {
                                    await installEngine(engine.id);
                                  } catch (err) {
                                    setIntegrated(String(err));
                                  }
                                }}
                              >
                                {t('install')}
                              </button>
                            )}
                          </div>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
              {models.length > 0 && (
                <div className="mt-3 rounded-lg border border-line bg-bg p-2.5" id="modelsSummary">
                  <strong className="mb-1.5 block text-xs">{t('modelsLabel')}</strong>
                  <div className="flex flex-wrap gap-2 text-xs">
                    {models.map((m) => (
                      <span
                        key={m.id}
                        className={`inline-flex items-center gap-1 rounded border border-line px-2 py-0.5 ${m.present ? 'ok' : 'bad'}`}
                        title={m.path ?? ''}
                      >
                        <span>{m.present ? '✓' : '✗'}</span>
                        <span>{ar ? m.label_ar : m.label}</span>
                        {m.disk_bytes ? <span className="hint">({formatBytes(m.disk_bytes)})</span> : null}
                      </span>
                    ))}
                  </div>
                </div>
              )}
              <div className="row justify-start">
                <button
                  type="button"
                  id="integrate"
                  onClick={async () => {
                    try {
                      const written = await integrateShell();
                      setIntegrated(`${t('added')} ${written.join(' · ')}`);
                    } catch (err) {
                      setIntegrated(String(err));
                    }
                  }}
                >
                  {t('integrate')}
                </button>
              </div>
              <p className="hint" id="integrated">
                {integrated}
              </p>
            </section>
          </section>
        </div>
      </main>
      </div>

      <AboutModal
        open={about}
        onClose={() => setAbout(false)}
        onDonate={() => {
          setAbout(false);
          setDonate(true);
        }}
      />
      <DonateModal open={donate} onClose={() => setDonate(false)} />
      <ErrorReportModal
        open={reportOpen}
        onClose={() => setReportOpen(false)}
        report={diagReport}
        loading={reportLoading}
      />

      {RESIZE_STRIPS.map((dir) => (
        <div
          key={dir}
          className={`rs ${dir.slice(0, 1).toLowerCase()}${dir.length > 5 ? dir.slice(5, 6).toLowerCase() : ''}`}
          data-resize={dir}
          onPointerDown={(e) => {
            e.preventDefault();
            resize(dir);
          }}
        />
      ))}
    </div>
  );
}
