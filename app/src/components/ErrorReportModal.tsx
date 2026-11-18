/**
 * Diagnostics & Error Report modal shared between the Settings Engines center and Jobs window.
 * Technical paths, exit codes, and stderr traces stay inside `dir="ltr"` containers so mixed
 * Arabic/Latin logs remain legible.
 */

import { useEffect, useState } from 'react';
import { copyText, saveDiagnosticReport } from '../lib/api';
import { useI18n } from '../lib/i18n';
import type { DiagnosticsReport } from '../lib/types';

export interface FailedJobEntry {
  name: string;
  error: string;
}

interface Props {
  open: boolean;
  onClose: () => void;
  report: DiagnosticsReport | null;
  loading: boolean;
  failedJobs?: FailedJobEntry[];
}

export function ErrorReportModal({ open, onClose, report, loading, failedJobs = [] }: Props) {
  const { t } = useI18n();
  const [notice, setNotice] = useState('');

  useEffect(() => {
    if (!open) return;
    setNotice('');
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [open, onClose]);

  if (!open) return null;

  const text = report?.report_text ?? '';
  const failingEngines = (report?.engines ?? []).filter((e) => e.health === 'broken');

  return (
    <div
      className="modal-backdrop"
      id="errorReportModal"
      role="dialog"
      aria-modal="true"
      aria-labelledby="errorReportTitle"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="modal-card wide max-w-[660px]" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h3 id="errorReportTitle">{t('errorReportTitle')}</h3>
          <button type="button" className="icon-btn" onClick={onClose} aria-label={t('close')}>
            ✕
          </button>
        </div>

        <div className="modal-body flex flex-col gap-3">
          {loading ? (
            <p className="hint">{t('testingEngines')}</p>
          ) : (
            <>
              {report && (
                <div className="flex flex-wrap gap-3 text-xs text-muted" dir="ltr">
                  <span>
                    OS: <strong>{report.os}</strong> ({report.arch})
                  </span>
                  <span>
                    v<strong>{report.app_version}</strong>
                  </span>
                  <span>
                    Profile: <strong>{report.profile}</strong>
                  </span>
                </div>
              )}

              {!failingEngines.length && !failedJobs.length ? (
                <p className="ok text-xs font-medium">{t('noErrorsFound')}</p>
              ) : null}

              {failingEngines.length > 0 && (
                <div className="flex flex-col gap-1.5">
                  {failingEngines.map((eng) => (
                    <div key={eng.id} className="rounded-md border border-bad/40 bg-bg p-2 text-xs">
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <strong className="bad">{eng.name}</strong>
                        <span className="font-mono text-muted" dir="ltr">
                          exit={eng.exit_code ?? '—'}
                        </span>
                      </div>
                      {eng.path && (
                        <div className="mt-1 font-mono text-[11px] text-muted break-all" dir="ltr">
                          {eng.path}
                        </div>
                      )}
                      {eng.stderr && (
                        <pre className="mt-1 max-h-24 overflow-auto whitespace-pre-wrap rounded bg-surface p-1.5 font-mono text-[11px]" dir="ltr">
                          {eng.stderr}
                        </pre>
                      )}
                    </div>
                  ))}
                </div>
              )}

              {failedJobs.length > 0 && (
                <div className="flex flex-col gap-1">
                  <strong className="text-xs">{t('failedJobsTitle')}</strong>
                  {failedJobs.map((j, idx) => (
                    <div key={`${j.name}-${idx}`} className="rounded-md border border-line bg-bg p-2 text-xs">
                      <div className="font-semibold" dir="auto">
                        {j.name}
                      </div>
                      <pre className="mt-1 max-h-20 overflow-auto whitespace-pre-wrap font-mono text-[11px] text-bad" dir="ltr">
                        {j.error}
                      </pre>
                    </div>
                  ))}
                </div>
              )}

              <pre
                id="diagnosticReportLog"
                dir="ltr"
                className="max-h-52 overflow-auto rounded-lg border border-line bg-bg p-2.5 font-mono text-[11px] leading-relaxed"
              >
                {text}
              </pre>

              {notice && (
                <p className="hint text-xs" role="status">
                  {notice}
                </p>
              )}
            </>
          )}
        </div>

        <div className="modal-foot">
          <button
            type="button"
            id="copyErrorReportBtn"
            disabled={loading || !text}
            onClick={async () => {
              await copyText(text);
              setNotice(t('copied'));
            }}
          >
            {t('copyReport')}
          </button>
          <button
            type="button"
            id="saveErrorReportBtn"
            disabled={loading || !text}
            onClick={async () => {
              try {
                const saved = await saveDiagnosticReport(text);
                if (saved) setNotice(`${t('reportSaved')} ${saved}`);
              } catch (err) {
                setNotice(String(err));
              }
            }}
          >
            {t('saveReport')}
          </button>
          <button type="button" className="primary" onClick={onClose}>
            {t('close')}
          </button>
        </div>
      </div>
    </div>
  );
}
