/**
 * Settings panel: wheel keys, result-reveal, hotkey and the two floating surfaces (tray icon,
 * drop orb), saved recipes and watch rules.
 *
 * Inputs are deliberately uncontrolled and read back from the DOM on submit. Rows are created and
 * removed while the form is open, and reading at submit time means a row the user typed into is
 * never lost to a re-render — the same contract the previous form had.
 */

import { useEffect, useRef, useState, type FormEvent } from 'react';
import { copyText, exportRecipes, getSettings, importRecipes, pickFolder, setSettings } from '../lib/api';
import { useI18n } from '../lib/i18n';
import type { Recipe, Settings, WatchRule } from '../lib/types';

const KEYS = ['Shift', 'Alt', 'Control', 'Meta'];
const FAVORITE_CATEGORIES = [
  ['all', 'catAll'],
  ['image', 'catImage'],
  ['video', 'catVideo'],
  ['audio', 'catAudio'],
  ['pdf', 'catPdf'],
  ['document', 'catDocument'],
  ['ebook', 'catEbook'],
  ['archive', 'catArchive'],
  ['subtitle', 'catSubtitle'],
] as const;
const FORMAT_POOL = [
  'jpg', 'png', 'webp', 'avif', 'heic', 'pdf', 'mp4', 'mov', 'mkv', 'webm', 'gif', 'mp3', 'm4a', 'wav', 'flac',
  'docx', 'md', 'txt', 'epub', 'zip', '7z',
];

interface RecipeRow extends Recipe {
  id: number;
}

interface WatchRow extends WatchRule {
  id: number;
}

let seq = 0;
const nextId = () => (seq += 1);

const valueOf = (row: Element, selector: string) =>
  (row.querySelector(selector) as HTMLInputElement | null)?.value.trim() ?? '';

export function SettingsView({ flashWatch = false }: { flashWatch?: boolean }) {
  const { t } = useI18n();
  const [ready, setReady] = useState(false);
  const [settings, setSettingsState] = useState<Settings | null>(null);
  const [favorites, setFavorites] = useState<string[]>([]);
  const [catFavorites, setCatFavorites] = useState<Record<string, string[]>>({});
  const [favCategory, setFavCategory] = useState<string>('all');
  const [favPick, setFavPick] = useState<string>('mp4');
  const [recipes, setRecipes] = useState<RecipeRow[]>([]);
  const [watch, setWatch] = useState<WatchRow[]>([]);
  const [showImport, setShowImport] = useState(false);
  const [importText, setImportText] = useState('');
  const [msg, setMsg] = useState('');
  const form = useRef<HTMLFormElement>(null);
  const watchHeading = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    let alive = true;
    getSettings()
      .then((s) => {
        if (!alive) return;
        setSettingsState(s);
        setFavorites((s.favorites ?? []).slice(0, 12));
        setCatFavorites(s.category_favorites ?? {});
        setRecipes((s.recipes ?? []).map((r) => ({ id: nextId(), ...r })));
        setWatch((s.watch ?? []).map((w) => ({ id: nextId(), ...w, enabled: w.enabled !== false })));
        setReady(true);
      })
      .catch((err) => alive && setMsg(String(err)));
    return () => {
      alive = false;
    };
  }, []);

  // Entering from the watch view highlights the rules section instead of leaving the user to hunt.
  useEffect(() => {
    if (!flashWatch || !ready) return;
    watchHeading.current?.scrollIntoView?.({ block: 'center' });
  }, [flashWatch, ready]);

  const save = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const el = form.current;
    if (!el) return;
    const data = new FormData(el);
    const rawParallel = Number(data.get('max_parallel') ?? 0);
    const next: Settings = {
      convert: String(data.get('convert') ?? ''),
      tools: String(data.get('tools') ?? ''),
      actions: String(data.get('actions') ?? ''),
      reveal: data.get('reveal') === 'on',
      hotkey: data.get('hotkey') === 'on',
      tray: data.get('tray') === 'on',
      orb: data.get('orb') === 'on',
      background: data.get('background') === 'on',
      hwaccel: (String(data.get('hwaccel') ?? 'auto') as Settings['hwaccel']),
      favorites,
      category_favorites: catFavorites,
      max_parallel: rawParallel > 0 ? rawParallel : null,
      recipes: [...el.querySelectorAll('[data-list="recipes"] .rows .row-item')]
        .map((row) => ({ name: valueOf(row, '.r-name'), steps: valueOf(row, '.r-steps') }))
        .filter((r) => r.name || r.steps),
      watch: [...el.querySelectorAll('[data-list="watch"] .rows .row-item')]
        .map((row) => ({
          dir: valueOf(row, '.w-dir'),
          exts: valueOf(row, '.w-exts'),
          steps: valueOf(row, '.w-steps'),
          enabled: (row.querySelector('.w-on') as HTMLInputElement | null)?.checked ?? false,
        }))
        .filter((w) => w.dir || w.steps),
    };
    try {
      await setSettings(next);
      setMsg(t('saved'));
    } catch (err) {
      setMsg(String(err));
    }
  };

  if (!ready || !settings) {
    return (
      <>
        <form id="settings" className="settings" ref={form} />
        <p className="hint" id="settingsMsg" aria-live="polite">
          {msg}
        </p>
      </>
    );
  }

  return (
    <>
      <form id="settings" className="settings" ref={form} onSubmit={save}>
        {(
          [
            ['convert', 'keyConvert'],
            ['tools', 'keyTools'],
            ['actions', 'keyActions'],
          ] as const
        ).map(([field, label]) => (
          <label key={field}>
            {t(label)}
            <select name={field} defaultValue={settings[field]}>
              {KEYS.map((key) => (
                <option key={key} value={key}>
                  {key}
                </option>
              ))}
            </select>
          </label>
        ))}

        <label className="check">
          <input type="checkbox" name="reveal" defaultChecked={settings.reveal} />
          {t('reveal')}
        </label>
        <label className="check">
          <input type="checkbox" name="hotkey" defaultChecked={settings.hotkey !== false} />
          {t('hotkey')}
        </label>
        <label className="check">
          <input type="checkbox" name="tray" defaultChecked={settings.tray === true} />
          {t('trayIcon')}
        </label>
        <label className="check">
          <input type="checkbox" name="orb" defaultChecked={settings.orb === true} />
          {t('orbIcon')}
        </label>
        <label className="check">
          <input type="checkbox" name="background" defaultChecked={settings.background !== false} />
          {t('runInBackground')}
        </label>

        <label>
          {t('hwaccel')}
          <select name="hwaccel" defaultValue={settings.hwaccel ?? 'auto'}>
            <option value="auto">{t('hwaccelAuto')}</option>
            <option value="on">{t('hwaccelOn')}</option>
            <option value="off">{t('hwaccelOff')}</option>
          </select>
        </label>

        <label>
          {t('maxParallel')}
          <select name="max_parallel" defaultValue={String(settings.max_parallel ?? 0)}>
            <option value="0">{t('parallelAuto')}</option>
            {[1, 2, 3, 4, 6, 8].map((n) => (
              <option key={n} value={String(n)}>
                {n}
              </option>
            ))}
          </select>
        </label>

        <div className="favorites-box" id="wheelFavorites">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <h3>{t('wheelFavorites')}</h3>
            <select
              id="favCategorySelect"
              aria-label={t('wheelFavorites')}
              value={favCategory}
              onChange={(e) => setFavCategory(e.target.value)}
            >
              {FAVORITE_CATEGORIES.map(([id, key]) => (
                <option key={id} value={id}>
                  {t(key)}
                </option>
              ))}
            </select>
          </div>
          {(() => {
            const activeFavs = favCategory === 'all' ? favorites : (catFavorites[favCategory] ?? []);
            const setActiveFavs = (updater: (prev: string[]) => string[]) => {
              if (favCategory === 'all') {
                setFavorites(updater);
              } else {
                setCatFavorites((prev) => ({
                  ...prev,
                  [favCategory]: updater(prev[favCategory] ?? []),
                }));
              }
            };
            return (
              <>
                <div className="my-1.5 flex flex-wrap gap-1.5">
                  {activeFavs.map((fmt, idx) => (
                    <span
                      key={fmt}
                      className="inline-flex items-center gap-1 rounded-md border border-line bg-bg px-2 py-0.5 text-xs"
                    >
                      <code>{fmt.toUpperCase()}</code>
                      <button
                        type="button"
                        aria-label={`${t('moveUp')} ${fmt}`}
                        disabled={idx === 0}
                        onClick={() =>
                          setActiveFavs((list) => {
                            const next = [...list];
                            [next[idx - 1], next[idx]] = [next[idx], next[idx - 1]];
                            return next;
                          })
                        }
                      >
                        ↑
                      </button>
                      <button
                        type="button"
                        aria-label={`${t('moveDown')} ${fmt}`}
                        disabled={idx === activeFavs.length - 1}
                        onClick={() =>
                          setActiveFavs((list) => {
                            const next = [...list];
                            [next[idx + 1], next[idx]] = [next[idx], next[idx + 1]];
                            return next;
                          })
                        }
                      >
                        ↓
                      </button>
                      <button
                        type="button"
                        aria-label={`${t('remove')} ${fmt}`}
                        onClick={() => setActiveFavs((list) => list.filter((x) => x !== fmt))}
                      >
                        ✕
                      </button>
                    </span>
                  ))}
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  <select
                    aria-label={t('addFavorite')}
                    value={favPick}
                    onChange={(e) => setFavPick(e.target.value)}
                  >
                    {FORMAT_POOL.map((fmt) => (
                      <option key={fmt} value={fmt}>
                        {fmt.toUpperCase()}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    id="addFavoriteBtn"
                    disabled={activeFavs.length >= 12 || activeFavs.includes(favPick)}
                    onClick={() =>
                      setActiveFavs((list) =>
                        list.length >= 12 || list.includes(favPick) ? list : [...list, favPick],
                      )
                    }
                  >
                    {t('addFavorite')}
                  </button>
                </div>
              </>
            );
          })()}
        </div>

        <div className="list" data-list="recipes">
          <h3>{t('recipes')}</h3>
          <div className="rows">
            {recipes.map((row) => (
              <RecipeRow
                key={row.id}
                row={row}
                onRemove={() => setRecipes((list) => list.filter((r) => r.id !== row.id))}
              />
            ))}
          </div>
          <button type="button" onClick={() => setRecipes((list) => [...list, { id: nextId(), name: '', steps: '' }])}>
            {t('add')}
          </button>
          <div className="mt-2 flex flex-wrap gap-2">
            <button
              type="button"
              id="exportRecipesBtn"
              onClick={async () => {
                try {
                  const el = form.current;
                  const current = el
                    ? [...el.querySelectorAll('[data-list="recipes"] .rows .row-item')]
                        .map((row) => ({ name: valueOf(row, '.r-name'), steps: valueOf(row, '.r-steps') }))
                        .filter((r) => r.name || r.steps)
                    : recipes.map(({ name, steps }) => ({ name, steps }));
                  const json = await exportRecipes(current);
                  await copyText(json);
                  setMsg(t('recipesExported'));
                } catch (err) {
                  setMsg(String(err));
                }
              }}
            >
              {t('exportRecipes')}
            </button>
            <button type="button" id="importRecipesToggleBtn" onClick={() => setShowImport((v) => !v)}>
              {t('importRecipes')}
            </button>
          </div>
          {showImport && (
            <div className="mt-2 flex flex-col gap-2">
              <textarea
                id="importRecipesText"
                rows={4}
                className="w-full rounded-md border border-line bg-bg p-2 font-mono text-xs"
                placeholder={t('pasteRecipeJson')}
                value={importText}
                onChange={(e) => setImportText(e.target.value)}
              />
              <div className="flex gap-2">
                <button
                  type="button"
                  id="importRecipesApplyBtn"
                  onClick={async () => {
                    try {
                      const imported = await importRecipes(importText);
                      setRecipes((list) => [...list, ...imported.map((r) => ({ id: nextId(), ...r }))]);
                      setImportText('');
                      setShowImport(false);
                      setMsg(t('recipesImported').replace('{n}', String(imported.length)));
                    } catch (err) {
                      setMsg(String(err));
                    }
                  }}
                >
                  {t('apply')}
                </button>
              </div>
            </div>
          )}
        </div>

        <div className="list" data-list="watch">
          <h3 ref={watchHeading} className={flashWatch ? 'flash' : undefined}>
            {t('watchFolders')}
          </h3>
          <div className="rows">
            {watch.map((row) => (
              <WatchRuleRow
                key={row.id}
                row={row}
                onRemove={() => setWatch((list) => list.filter((w) => w.id !== row.id))}
              />
            ))}
          </div>
          <button
            type="button"
            onClick={() =>
              setWatch((list) => [...list, { id: nextId(), dir: '', exts: '', steps: '', enabled: true }])
            }
          >
            {t('add')}
          </button>
        </div>

        <button type="submit" className="primary">
          {t('saveSettings')}
        </button>
      </form>
      <p className="hint" id="settingsMsg" aria-live="polite">
        {msg}
      </p>
    </>
  );
}

function RecipeRow({ row, onRemove }: { row: RecipeRow; onRemove: () => void }) {
  const { t } = useI18n();
  return (
    <div className="row-item">
      <input className="r-name" placeholder={t('recipeName')} defaultValue={row.name} />
      <input className="r-steps wide" placeholder={t('recipeSteps')} defaultValue={row.steps} />
      <button type="button" onClick={onRemove} aria-label={t('remove')}>
        ✕
      </button>
    </div>
  );
}

function WatchRuleRow({ row, onRemove }: { row: WatchRow; onRemove: () => void }) {
  const { t } = useI18n();
  const dir = useRef<HTMLInputElement>(null);
  return (
    <div className="row-item">
      <button
        type="button"
        onClick={async () => {
          const picked = await pickFolder();
          if (picked && dir.current) dir.current.value = picked;
        }}
      >
        {t('folder')}
      </button>
      <input className="w-dir wide" ref={dir} defaultValue={row.dir} readOnly placeholder={t('folder')} />
      <input className="w-exts" defaultValue={row.exts} placeholder={t('watchExts')} />
      <input className="w-steps wide" defaultValue={row.steps} placeholder={t('watchSteps')} />
      <label className="check inline">
        <input type="checkbox" className="w-on" defaultChecked={row.enabled} />
        {t('on')}
      </label>
      <button type="button" onClick={onRemove} aria-label={t('remove')}>
        ✕
      </button>
    </div>
  );
}
