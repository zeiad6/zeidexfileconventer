/**
 * The options form a choice opens when it has parameters (compression percent, a rename
 * pattern, a password…). Shared by the wheel and the studio window so both behave the same.
 * Inputs are uncontrolled: the backend supplies defaults and the form reads what the user typed,
 * which keeps the wheel's DOM stable while the pointer is over it.
 */

import { useRef, useState, type FormEvent } from 'react';
import { pickFiles } from '../lib/api';
import { useI18n } from '../lib/i18n';
import type { Choice } from '../lib/types';

interface Props {
  choice: Choice;
  onSubmit: (opts: Record<string, string>) => Promise<void>;
  onCancel: () => void;
  /** `params` floats over the wheel; `inline` sits in the studio's selection card. */
  variant?: 'params' | 'inline';
  /** Kept stable because the old UI's id is part of the automation contract. */
  id?: string;
}

export function ParamsForm({ choice, onSubmit, onCancel, variant = 'params', id }: Props) {
  const { t, zl } = useI18n();
  const form = useRef<HTMLFormElement>(null);
  const [error, setError] = useState('');

  const submit = async (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    if (!form.current) return;
    setError('');
    try {
      await onSubmit(Object.fromEntries(new FormData(form.current)) as Record<string, string>);
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <form
      id={id}
      ref={form}
      className={variant === 'params' ? 'params' : 'inline-form'}
      onSubmit={submit}
    >
      <h3>{zl(choice)}</h3>
      {choice.params.map((p) => (
        <label key={p.key}>
          {zl(p)}
          <input name={p.key} defaultValue={p.default} autoComplete="off" />
          {p.key === 'image' && (
            <button
              type="button"
              className="mt-1"
              onClick={async () => {
                const picked = await pickFiles();
                const input = form.current?.querySelector<HTMLInputElement>(`input[name="${p.key}"]`);
                if (picked.length && input) input.value = picked[0];
              }}
            >
              …
            </button>
          )}
        </label>
      ))}
      <p className="err" role="alert">
        {error}
      </p>
      <div className="mt-2 flex justify-end gap-2">
        <button type="button" onClick={onCancel}>
          {t('cancel')}
        </button>
        <button type="submit" className="primary">
          {t('run')}
        </button>
      </div>
    </form>
  );
}
