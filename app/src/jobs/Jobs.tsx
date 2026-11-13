/**
 * Progress window: one row per file of each job, newest first. Rows are keyed by job and file
 * index, so a progress event updates the row a start event created instead of appending one.
 */

import { useEffect, useReducer } from 'react';
import { reveal, safeListen } from '../lib/api';
import { useI18n } from '../lib/i18n';
import type { JobEvent } from '../lib/types';

interface Row {
  key: string;
  name: string;
  meta: string;
  fraction: number;
  state: 'run' | 'done' | 'failed';
  output?: string;
  error?: string;
  text?: string;
}

type Action = { type: 'event'; event: JobEvent } | { type: 'clear' };

function upsert(rows: Row[], key: string, patch: (row: Row) => Row, seed: Row): Row[] {
  const at = rows.findIndex((r) => r.key === key);
  if (at < 0) return [...rows, patch(seed)];
  return rows.map((r, i) => (i === at ? patch(r) : r));
}

function reduce(rows: Row[], action: Action): Row[] {
  if (action.type === 'clear') return [];
  const e = action.event;
  const base: Row = { key: '', name: '', meta: '', fraction: 0, state: 'run' };
  if (e.type === 'text') {
    const key = `${e.job}:${e.title}`;
    return upsert(rows, key, (r) => ({ ...r, state: 'done', text: e.text }), {
      ...base,
      key,
      name: e.title,
    });
  }
  const key = `${e.job}:${e.index ?? 0}`;
  if (e.type === 'start') {
    return upsert(
      rows,
      key,
      (r) => ({ ...r, name: e.file, meta: `${e.title} · ${e.index + 1}/${e.total}` }),
      { ...base, key, name: e.file, meta: `${e.title} · ${e.index + 1}/${e.total}` },
    );
  }
  if (e.type === 'progress') {
    return upsert(rows, key, (r) => ({ ...r, fraction: e.fraction }), { ...base, key, fraction: e.fraction });
  }
  if (e.type === 'done') {
    return upsert(rows, key, (r) => ({ ...r, state: 'done', fraction: 1, output: e.output, meta: `→ ${e.output.split(/[\\/]/).pop()}` }), {
      ...base,
      key,
      state: 'done',
      fraction: 1,
      output: e.output,
      meta: `→ ${e.output.split(/[\\/]/).pop()}`,
    });
  }
  return upsert(rows, key, (r) => ({ ...r, state: 'failed', fraction: 1, error: e.error }), {
    ...base,
    key,
    state: 'failed',
    fraction: 1,
    error: e.error,
  });
}

export function Jobs() {
  const { t } = useI18n();
  const [rows, dispatch] = useReducer(reduce, []);

  useEffect(() => safeListen<JobEvent>('job', (event) => dispatch({ type: 'event', event })), []);

  return (
    <main className="p-4">
      <h1 className="m-0 mb-2 flex items-center justify-between text-[15px] font-semibold">
        <span>{t('jobs')}</span>
        <button type="button" onClick={() => dispatch({ type: 'clear' })}>
          {t('clear')}
        </button>
      </h1>
      <div id="list" aria-live="polite">
        {[...rows].reverse().map((row) => (
          <div
            key={row.key}
            className={`job border-t border-line py-2 ${row.state === 'done' ? 'done' : ''}${row.state === 'failed' ? 'failed' : ''}`}
          >
            {row.name && <div className="name font-semibold break-words">{row.name}</div>}
            {row.meta && <div className="meta text-xs text-muted">{row.meta}</div>}
            <div className="bar my-1.5 h-1.5 overflow-hidden rounded-sm bg-line">
              <i
                className="block h-full transition-[width] duration-200"
                style={{
                  width: `${Math.round(row.fraction * 100)}%`,
                  background:
                    row.state === 'failed' ? 'var(--bad)' : row.state === 'done' ? 'var(--ok)' : 'var(--accent)',
                }}
              />
            </div>
            {row.state === 'done' && row.output && (
              <button type="button" onClick={() => void reveal(row.output!)}>
                {t('show')}
              </button>
            )}
            {row.error && <div className="text-xs break-words text-bad">{row.error}</div>}
            {row.text && (
              <pre className="max-h-[220px] overflow-auto rounded-md bg-bg p-2 text-xs whitespace-pre-wrap">
                {row.text}
              </pre>
            )}
          </div>
        ))}
      </div>
    </main>
  );
}
