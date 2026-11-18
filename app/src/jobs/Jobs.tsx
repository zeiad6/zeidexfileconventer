/**
 * Progress window: one row per file of each job, newest first. Rows are keyed by job and file
 * index, so a progress event updates the row a start event created instead of appending one.
 */

import { useEffect, useReducer, useState } from 'react';
import { ErrorReportModal } from '../components/ErrorReportModal';
import {
  cancelAllJobs,
  cancelJob,
  diagnosticReport,
  localdropCancel,
  openOutputFolder,
  pauseJob,
  resumeJob,
  reveal,
  safeListen,
} from '../lib/api';
import { useI18n } from '../lib/i18n';
import type { DiagnosticsReport, JobEvent } from '../lib/types';

interface Row {
  key: string;
  job: number;
  name: string;
  meta: string;
  fraction: number;
  state: 'run' | 'paused' | 'done' | 'failed';
  output?: string;
  error?: string;
  text?: string;
  /** LocalDrop pairing code (SVG markup) while it waits for the phone. */
  qr?: string;
  address?: string;
  pair?: boolean;
  sent?: { device: string; files: number; declined: boolean };
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
  if (e.type === 'paused') {
    return rows.map((r) => (r.job === e.job && r.state === 'run' ? { ...r, state: 'paused' } : r));
  }
  if (e.type === 'resumed') {
    return rows.map((r) => (r.job === e.job && r.state === 'paused' ? { ...r, state: 'run' } : r));
  }
  if (e.type === 'cancelled') {
    return rows.map((r) =>
      r.job === e.job && (r.state === 'run' || r.state === 'paused')
        ? { ...r, state: 'failed', fraction: 1, error: 'Cancelled' }
        : r,
    );
  }
  const base: Row = { key: '', job: e.job, name: '', meta: '', fraction: 0, state: 'run' };
  if (e.type === 'text') {
    const key = `${e.job}:${e.title}`;
    return upsert(rows, key, (r) => ({ ...r, state: 'done', text: e.text }), {
      ...base,
      key,
      name: e.title,
    });
  }
  if (e.type === 'pair') {
    const key = `${e.job}:0`;
    const seed: Row = { ...base, key, name: e.file, meta: e.title, pair: true, qr: e.svg, address: e.address };
    return upsert(rows, key, (r) => ({ ...r, ...seed }), seed);
  }
  if (e.type === 'sent') {
    const key = `${e.job}:0`;
    const done = (r: Row): Row => ({ ...r, state: e.declined ? 'failed' : 'done', fraction: 1, qr: undefined, sent: e });
    return upsert(rows, key, done, { ...base, key, pair: true });
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
    // Once the phone connects, the code has done its job.
    return upsert(rows, key, (r) => ({ ...r, fraction: e.fraction, qr: undefined }), { ...base, key, fraction: e.fraction });
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
  return upsert(rows, key, (r) => ({ ...r, state: 'failed', fraction: 1, error: e.error, qr: undefined }), {
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
  const [reportOpen, setReportOpen] = useState(false);
  const [reportLoading, setReportLoading] = useState(false);
  const [report, setReport] = useState<DiagnosticsReport | null>(null);

  useEffect(() => safeListen<JobEvent>('job', (event) => dispatch({ type: 'event', event })), []);

  const failedJobs = rows
    .filter((r) => r.state === 'failed' && r.error)
    .map((r) => ({ name: r.name || `#${r.job}`, error: r.error! }));

  return (
    <main className="p-4">
      <h1 className="m-0 mb-2 flex flex-wrap items-center justify-between gap-2 text-[15px] font-semibold">
        <span>{t('jobs')}</span>
        <div className="flex flex-wrap gap-1.5">
          <button
            type="button"
            onClick={() => {
              void cancelAllJobs();
              dispatch({ type: 'clear' });
            }}
          >
            {t('clear')}
          </button>
          <button
            type="button"
            id="viewJobsReportBtn"
            className={failedJobs.length ? 'border-bad/60 bad' : undefined}
            onClick={async () => {
              setReportOpen(true);
              try {
                setReportLoading(true);
                const rep = await diagnosticReport(failedJobs);
                setReport(rep);
              } catch {
                /* fallback handled inside modal */
              } finally {
                setReportLoading(false);
              }
            }}
          >
            {t('viewErrorReport')}
          </button>
        </div>
      </h1>
      <div id="list" aria-live="polite">
        {[...rows].reverse().map((row) => (
          <div
            key={row.key}
            className={`job border-t border-line py-2 ${row.state === 'done' ? 'done' : ''}${row.state === 'failed' ? 'failed' : ''}`}
          >
            {/* File names are mostly Latin: dir="auto" keeps "3 · 48.2 MB" from reversing under RTL. */}
            {row.name && (
              <div className="name font-semibold break-words" dir="auto">
                {row.name}
              </div>
            )}
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
            {!row.pair && (row.state === 'run' || row.state === 'paused') && (
              <div className="mt-1 flex flex-wrap gap-1.5">
                {row.state === 'run' ? (
                  <button type="button" onClick={() => void pauseJob(row.job)}>
                    {t('pause')}
                  </button>
                ) : (
                  <button type="button" onClick={() => void resumeJob(row.job)}>
                    {t('resume')}
                  </button>
                )}
                <button type="button" onClick={() => void cancelJob(row.job)}>
                  {t('cancel')}
                </button>
                {row.state === 'paused' && <span className="self-center text-xs text-muted">{t('paused')}</span>}
              </div>
            )}
            {row.state === 'done' && row.output && (
              <div className="mt-1 flex flex-wrap gap-1.5">
                <button type="button" onClick={() => void reveal(row.output!)}>
                  {t('show')}
                </button>
                <button type="button" onClick={() => void openOutputFolder(row.output!)}>
                  {t('openOutputFolder')}
                </button>
              </div>
            )}
            {row.qr && (
              <figure className="pair m-0 my-2 flex flex-col items-center gap-1.5">
                {/* As an <img>, the SVG cannot run script even if its markup were hostile. */}
                <img
                  src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(row.qr)}`}
                  alt={t('pairCode')}
                  width={200}
                  height={200}
                  className="rounded-md bg-white p-1"
                />
                <figcaption className="text-center text-xs text-muted">
                  {t('scanToReceive')} <bdi dir="ltr">{row.address}</bdi>
                </figcaption>
              </figure>
            )}
            {row.pair && row.state === 'run' && (
              <button type="button" onClick={() => void localdropCancel()}>
                {t('cancel')}
              </button>
            )}
            {row.sent && (
              <div className="text-xs">
                {t(row.sent.declined ? 'declinedBy' : 'sentTo')
                  .replace('{n}', String(row.sent.files))
                  .replace('{device}', row.sent.device)}
              </div>
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
      <ErrorReportModal
        open={reportOpen}
        onClose={() => setReportOpen(false)}
        report={report}
        loading={reportLoading}
        failedJobs={failedJobs}
      />
    </main>
  );
}
