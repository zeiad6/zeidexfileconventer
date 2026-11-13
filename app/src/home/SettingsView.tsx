/**
 * Settings panel: wheel keys, result-reveal and hotkey toggles, saved recipes and watch rules.
 *
 * Inputs are deliberately uncontrolled and read back from the DOM on submit. Rows are created and
 * removed while the form is open, and reading at submit time means a row the user typed into is
 * never lost to a re-render — the same contract the previous form had.
 */

import { useEffect, useRef, useState, type FormEvent } from 'react';
import { getSettings, pickFolder, setSettings } from '../lib/api';
import { useI18n } from '../lib/i18n';
import type { Recipe, Settings, WatchRule } from '../lib/types';

const KEYS = ['Shift', 'Alt', 'Control', 'Meta'];

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
  const [recipes, setRecipes] = useState<RecipeRow[]>([]);
  const [watch, setWatch] = useState<WatchRow[]>([]);
  const [msg, setMsg] = useState('');
  const form = useRef<HTMLFormElement>(null);
  const watchHeading = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    let alive = true;
    getSettings()
      .then((s) => {
        if (!alive) return;
        setSettingsState(s);
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
    const next: Settings = {
      convert: String(data.get('convert') ?? ''),
      tools: String(data.get('tools') ?? ''),
      actions: String(data.get('actions') ?? ''),
      reveal: data.get('reveal') === 'on',
      hotkey: data.get('hotkey') === 'on',
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
