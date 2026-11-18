/**
 * The three-ring radial menu shown under the pointer while files are dragged.
 *
 *   outer   modes      Convert / Tools / Actions (dwell or scroll to morph the ring)
 *   middle  choices    formats, tools or actions for the current mode
 *   inner   sub-ring   quality presets or a tool's default options
 *   hub     cancel     dropping here abandons the job (or shows micro-progress / Reveal)
 *
 * Pure CSS transitions/transforms keep the 380px overlay lightweight and 60fps.
 */

import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react';
import { ParamsForm } from '../components/ParamsForm';
import {
  C,
  CHILD_R,
  DWELL_MS,
  HUB_R,
  IDLE_MS,
  ITEMS_R,
  LEAVE_MS,
  MODES,
  MODES_R,
  OPEN_MS,
  SCALE,
  WHEEL_PX,
  arcPath,
  childLabelMax,
  hitTest,
  itemLabelMax,
  midAngle,
  polar,
  shortLabel,
  sourceExtLabel,
  wedgePopTransform,
  type Hit,
  type WheelMode,
} from '../lib/geometry';
import {
  describe,
  hideWheel,
  keepWheel,
  onDragDrop,
  openEditor,
  reveal,
  runJob,
  safeListen,
  wheelState,
} from '../lib/api';
import { useI18n } from '../lib/i18n';
import type { Choice, Desc, JobEvent, Sub } from '../lib/types';

function modeTilt(index: number, count: number): number {
  let deg = (midAngle(index, count) * 180) / Math.PI + 90;
  deg = ((deg % 360) + 360) % 360;
  if (deg > 180) deg -= 360;
  return deg > 90 ? deg - 180 : deg;
}

interface MicroJob {
  id: number;
  fraction: number;
  state: 'run' | 'done' | 'failed';
  output?: string;
  error?: string;
}

interface State {
  paths: string[];
  desc: Desc | null;
  mode: WheelMode;
  hover: number;
  sub: number;
  open: number;
  chip: number;
  centre: boolean;
  form: Choice | null;
}

const INITIAL: State = {
  paths: [],
  desc: null,
  mode: 'convert',
  hover: -1,
  sub: -1,
  open: -1,
  chip: -1,
  centre: false,
  form: null,
};

type Action =
  | { type: 'loaded'; paths: string[]; desc: Desc }
  | { type: 'mode'; mode: WheelMode }
  | { type: 'hover'; index: number; closeOpen: boolean }
  | { type: 'sub'; index: number }
  | { type: 'open'; index: number }
  | { type: 'chip'; index: number }
  | { type: 'centre'; on: boolean }
  | { type: 'form'; choice: Choice | null }
  | { type: 'reset' };

const firstFilled = (d: Desc): WheelMode => MODES.find((m) => d[m].length) ?? 'convert';

function reduce(s: State, a: Action): State {
  switch (a.type) {
    case 'loaded': {
      const mode = a.desc[s.mode].length ? s.mode : firstFilled(a.desc);
      return { ...s, paths: a.paths, desc: a.desc, mode, hover: -1, sub: -1, open: -1 };
    }
    case 'mode':
      return a.mode === s.mode ? s : { ...s, mode: a.mode, hover: -1, sub: -1, open: -1 };
    case 'hover':
      return {
        ...s,
        hover: a.index,
        sub: -1,
        open: a.closeOpen ? -1 : s.open,
      };
    case 'sub':
      return a.index === s.sub ? s : { ...s, sub: a.index };
    case 'open':
      return { ...s, open: a.index };
    case 'chip':
      return a.index === s.chip ? s : { ...s, chip: a.index };
    case 'centre':
      return a.on === s.centre ? s : { ...s, centre: a.on };
    case 'form':
      return { ...s, form: a.choice, open: a.choice ? -1 : s.open };
    case 'reset':
      return INITIAL;
    default:
      return s;
  }
}

const RING_CIRCUMFERENCE = 2 * Math.PI * (HUB_R - 4);

export function Wheel() {
  const { t, zl } = useI18n();
  const [state, rawDispatch] = useReducer(reduce, INITIAL);
  const latest = useRef(state);
  const commit = useCallback((action: Action) => {
    const next = reduce(latest.current, action);
    latest.current = next;
    rawDispatch(action);
    return next;
  }, []);

  const chipTimer = useRef<number | undefined>(undefined);
  const openTimer = useRef<number | undefined>(undefined);
  const hideTimer = useRef<number | undefined>(undefined);
  const idleTimer = useRef<number | undefined>(undefined);
  const errTimer = useRef<number | undefined>(undefined);
  const toastTimer = useRef<number | undefined>(undefined);
  const clickMode = useRef(false);
  const lastNativeOver = useRef(0);
  const nativeDragPaths = useRef<string[] | null>(null);
  const loadGen = useRef(0);
  const inFlightLoad = useRef<{ key: string; promise: Promise<{ desc: Desc; mode: WheelMode }> } | null>(null);
  const [runError, setRunError] = useState('');
  const [microJob, setMicroJob] = useState<MicroJob | null>(null);
  const microJobRef = useRef<MicroJob | null>(null);
  microJobRef.current = microJob;

  const items = state.desc ? state.desc[state.mode] : [];
  const kids = state.open >= 0 && items[state.open] ? items[state.open].children ?? [] : [];

  const stopTimers = useCallback(() => {
    window.clearTimeout(chipTimer.current);
    window.clearTimeout(openTimer.current);
    window.clearTimeout(hideTimer.current);
    window.clearTimeout(idleTimer.current);
    window.clearTimeout(errTimer.current);
    window.clearTimeout(toastTimer.current);
  }, []);

  const fail = useCallback((e: unknown) => {
    setRunError(e instanceof Error ? e.message : String(e));
    window.clearTimeout(errTimer.current);
    errTimer.current = window.setTimeout(() => setRunError(''), IDLE_MS);
  }, []);

  /** Hide the wheel when the pointer rests on it and nothing is picked for a while. */
  const bumpIdle = useCallback(() => {
    window.clearTimeout(idleTimer.current);
    idleTimer.current = window.setTimeout(() => {
      if (!latest.current.form && !microJobRef.current) void hideWheel();
    }, IDLE_MS);
  }, []);

  const setMode = useCallback(
    (mode: WheelMode) => {
      const s = latest.current;
      if (mode === s.mode) return;
      if (s.desc && s.desc[mode].length === 0) return;
      commit({ type: 'mode', mode });
    },
    [commit],
  );

  const load = useCallback(
    async (paths: string[]) => {
      const s = latest.current;
      const key = paths.join('\n');
      if (s.desc && key === s.paths.join('\n')) {
        return { desc: s.desc, mode: s.mode };
      }
      if (inFlightLoad.current && inFlightLoad.current.key === key) {
        return inFlightLoad.current.promise;
      }
      const gen = ++loadGen.current;
      const promise = (async () => {
        const desc = await describe(paths);
        if (loadGen.current !== gen) {
          return { desc, mode: latest.current.mode };
        }
        const next = commit({ type: 'loaded', paths, desc });
        return { desc, mode: next.mode };
      })().finally(() => {
        if (inFlightLoad.current?.key === key) {
          inFlightLoad.current = null;
        }
      });
      inFlightLoad.current = { key, promise };
      return promise;
    },
    [commit],
  );

  const launchJob = useCallback(
    async (paths: string[], kind: string, id: string, opts: Record<string, string>) => {
      setRunError('');
      const jobId = await runJob(paths, kind, id, opts);
      if (typeof jobId === 'number' && jobId > 0) {
        window.clearTimeout(idleTimer.current);
        window.clearTimeout(toastTimer.current);
        setMicroJob({ id: jobId, fraction: 0, state: 'run' });
      }
    },
    [],
  );

  const choose = useCallback(
    (it: Choice) => {
      if (it.compatible === false) return;
      const s = latest.current;
      if (it.editor) {
        void openEditor(s.paths, it.id);
        return;
      }
      if (!it.params.length) {
        void launchJob(s.paths, it.kind, it.id, it.preset ?? {}).catch(fail);
        return;
      }
      void keepWheel();
      commit({ type: 'form', choice: it });
    },
    [commit, fail, launchJob],
  );

  const chooseSub = useCallback(
    (it: Choice, child: Sub) => {
      const s = latest.current;
      if (child.custom) {
        choose(it);
        return;
      }
      const opts = { ...(it.preset ?? {}), [child.key]: child.value };
      void launchJob(s.paths, it.kind, it.id, opts).catch(fail);
    },
    [choose, fail, launchJob],
  );

  const hitOf = useCallback((pos: { x: number; y: number }): Hit => {
    const s = latest.current;
    const list = s.desc ? s.desc[s.mode] : [];
    const children = s.open >= 0 && list[s.open] ? list[s.open].children ?? [] : [];
    return hitTest(pos, {
      dpr: window.devicePixelRatio || 1,
      scale: SCALE,
      itemCount: list.length,
      childCount: children.length,
    });
  }, []);

  const over = useCallback(
    (pos: { x: number; y: number }) => {
      if (microJobRef.current) return;
      const s = latest.current;
      const h = hitOf(pos);

      bumpIdle();
      const centre = h.centre === true;
      if (centre !== s.centre) commit({ type: 'centre', on: centre });

      const chip = h.chip ?? -1;
      if (chip !== s.chip) {
        window.clearTimeout(chipTimer.current);
        commit({ type: 'chip', index: chip });
        if (chip >= 0 && MODES[chip] !== s.mode && (!s.desc || s.desc[MODES[chip]].length)) {
          chipTimer.current = window.setTimeout(() => setMode(MODES[chip]), DWELL_MS);
        }
      }

      const item = h.item ?? -1;
      if (h.item != null || h.centre || h.chip != null) {
        if (item !== s.hover) {
          const children = s.desc && s.desc[s.mode][item] ? s.desc[s.mode][item].children ?? [] : [];
          const closeOpen = s.open >= 0 && s.open !== item;
          commit({ type: 'hover', index: item, closeOpen });
          window.clearTimeout(openTimer.current);
          if (children.length) {
            openTimer.current = window.setTimeout(() => {
              if (latest.current.hover === item) commit({ type: 'open', index: item });
            }, OPEN_MS);
          }
        }
      }

      const sub = h.sub ?? -1;
      if (sub !== s.sub) commit({ type: 'sub', index: sub });
    },
    [bumpIdle, commit, hitOf, setMode],
  );

  const drop = useCallback(
    async (paths: string[], pos: { x: number; y: number }) => {
      window.clearTimeout(chipTimer.current);
      window.clearTimeout(openTimer.current);
      const effectivePaths = paths.length ? paths : latest.current.paths;
      if (!effectivePaths.length) return hideWheel();
      await load(effectivePaths);
      const h = hitOf(pos);
      const s = latest.current;
      const list = s.desc ? s.desc[s.mode] : [];
      const children = s.open >= 0 && list[s.open] ? list[s.open].children ?? [] : [];
      if (h.centre) return hideWheel();
      if (h.sub != null) {
        if (!children.length || s.open < 0) return;
        return chooseSub(list[s.open], children[h.sub]);
      }
      if (h.item != null && list[h.item]) return choose(list[h.item]);
      if (h.chip != null) {
        setMode(MODES[h.chip]);
        clickMode.current = true;
        void keepWheel();
        return;
      }
      return hideWheel();
    },
    [choose, chooseSub, hitOf, load, setMode],
  );

  // Listen to job progress for the instant micro-progress ring & Reveal toast inside the Wheel.
  useEffect(() => {
    return safeListen<JobEvent>('job', (ev) => {
      const cur = microJobRef.current;
      if (!cur || ev.job !== cur.id) return;
      if (ev.type === 'progress') {
        setMicroJob({ ...cur, fraction: ev.fraction });
      } else if (ev.type === 'done') {
        setMicroJob({ id: cur.id, fraction: 1, state: 'done', output: ev.output });
        window.clearTimeout(toastTimer.current);
        toastTimer.current = window.setTimeout(() => {
          setMicroJob(null);
          void hideWheel();
        }, 2600);
      } else if (ev.type === 'failed') {
        setMicroJob(null);
        fail(ev.error);
        window.clearTimeout(toastTimer.current);
        toastTimer.current = window.setTimeout(() => void hideWheel(), 2400);
      }
    });
  }, [fail]);

  // Native drag-and-drop: enter loads the file types, over tracks the pointer, drop commits.
  useEffect(
    () =>
      onDragDrop(async (p) => {
        if (latest.current.form || microJobRef.current) return;
        if (p.type === 'enter') {
          window.clearTimeout(hideTimer.current);
          lastNativeOver.current = Date.now();
          bumpIdle();
          if (p.paths.length) {
            nativeDragPaths.current = p.paths;
            try {
              await load(p.paths);
            } catch (e) {
              fail(e);
              hideTimer.current = window.setTimeout(() => void hideWheel(), IDLE_MS);
              return;
            }
          }
          over(p.position);
        } else if (p.type === 'over') {
          lastNativeOver.current = Date.now();
          over(p.position);
        } else if (p.type === 'drop') {
          if (p.paths.length) nativeDragPaths.current = p.paths;
          await drop(p.paths.length ? p.paths : (nativeDragPaths.current ?? []), p.position);
        } else {
          commit({ type: 'hover', index: -1, closeOpen: false });
          commit({ type: 'sub', index: -1 });
          hideTimer.current = window.setTimeout(() => {
            if (!microJobRef.current && !latest.current.form) void hideWheel();
          }, LEAVE_MS);
        }
      }),
    [bumpIdle, commit, drop, fail, load, over],
  );

  // Click/keyboard mode (Ctrl+Shift+Space on a selection): no drag, so mouse moves, clicks,
  // the mouse wheel and the keys drive the wheel.
  useEffect(() => {
    const offs: Array<() => void> = [];
    offs.push(
      safeListen<string[]>('wheel-open', async (paths) => {
        stopTimers();
        loadGen.current += 1;
        inFlightLoad.current = null;
        clickMode.current = true;
        nativeDragPaths.current = null;
        setMicroJob(null);
        commit({ type: 'reset' });
        try {
          const { desc } = await load(paths);
          const list = desc[latest.current.mode];
          if (list.length) commit({ type: 'hover', index: 0, closeOpen: true });
        } catch (e) {
          fail(e);
          hideTimer.current = window.setTimeout(() => void hideWheel(), IDLE_MS);
        }
        bumpIdle();
      }),
    );

    offs.push(safeListen<void>('wheel-alive', () => bumpIdle()));

    offs.push(
      safeListen<{ x: number; y: number }>('wheel-hover', (pos) => {
        if (latest.current.form || microJobRef.current) return;
        // Prefer native DragDropEvent::Over when active; otherwise drive hover at 60 Hz from
        // the OS pointer sampler so section/tool switching stays instant even on WebView2.
        if (Date.now() - lastNativeOver.current > 80) {
          over(pos);
        }
      }),
    );

    offs.push(
      safeListen<{ position: { x: number; y: number }; paths: string[] }>('wheel-commit', async (payload) => {
        if (latest.current.form || microJobRef.current) return;
        await drop(payload.paths, payload.position);
      }),
    );

    offs.push(
      safeListen<string[]>('wheel-files', async (paths) => {
        // Never let a delayed Explorer probe replace paths already supplied by native OS DragDrop.
        if (nativeDragPaths.current !== null || latest.current.form || microJobRef.current || !paths.length) return;
        window.clearTimeout(hideTimer.current);
        bumpIdle();
        try {
          await load(paths);
        } catch (e) {
          fail(e);
          hideTimer.current = window.setTimeout(() => void hideWheel(), IDLE_MS);
        }
      }),
    );

    offs.push(
      safeListen<string>('wheel-mode', (mode) => {
        if (!latest.current.form && !microJobRef.current) setMode(mode as WheelMode);
      }),
    );

    offs.push(
      safeListen<void>('wheel-hidden', () => {
        stopTimers();
        loadGen.current += 1;
        inFlightLoad.current = null;
        clickMode.current = false;
        nativeDragPaths.current = null;
        setMicroJob(null);
        commit({ type: 'reset' });
      }),
    );

    wheelState()
      .then(async (st) => {
        if (!st?.paths?.length || latest.current.paths.length) return;
        if (st.click) clickMode.current = true;
        if (st.mode) setMode(st.mode);
        try {
          const { desc } = await load(st.paths);
          if (st.click) {
            const list = desc[latest.current.mode];
            if (list.length) commit({ type: 'hover', index: 0, closeOpen: true });
          }
          bumpIdle();
        } catch (e) {
          fail(e);
        }
      })
      .catch(() => {});

    const physical = (e: MouseEvent) => ({
      x: e.clientX * (window.devicePixelRatio || 1),
      y: e.clientY * (window.devicePixelRatio || 1),
    });
    const onMove = (e: MouseEvent) => {
      if (!latest.current.form && !microJobRef.current) over(physical(e));
    };
    const onClick = (e: MouseEvent) => {
      const target = e.target as Element | null;
      if (
        (!clickMode.current && !latest.current.paths.length) ||
        latest.current.form ||
        microJobRef.current ||
        target?.closest?.('.params') ||
        target?.closest?.('.wheel-toast-btn')
      ) {
        return;
      }
      void drop(latest.current.paths, physical(e));
    };
    const onBlur = () => {
      if (clickMode.current && !latest.current.form && !microJobRef.current) void hideWheel();
    };
    const onWheel = (e: WheelEvent) => {
      const s = latest.current;
      if (s.form || !s.desc || microJobRef.current) return;
      const filled = MODES.filter((m) => s.desc![m].length);
      if (filled.length < 2) return;
      e.preventDefault();
      const at = filled.indexOf(s.mode);
      const next = e.deltaY > 0 ? (at + 1) % filled.length : (at - 1 + filled.length) % filled.length;
      setMode(filled[next]);
    };
    const onKey = (e: KeyboardEvent) => {
      const s = latest.current;
      if (e.key === 'Escape') {
        if (microJobRef.current) {
          setMicroJob(null);
          void hideWheel();
          return;
        }
        if (s.open >= 0) {
          commit({ type: 'open', index: -1 });
          commit({ type: 'sub', index: -1 });
          return;
        }
        void hideWheel();
        return;
      }
      if (s.form || !s.desc || microJobRef.current) return;
      const list = s.desc[s.mode];
      const n = list.length;
      const k = Math.max(s.open >= 0 ? list[s.open]?.children?.length ?? 0 : 0, 1);
      if (e.key === 'ArrowDown' || e.key === 'ArrowRight') {
        if (s.open >= 0) commit({ type: 'sub', index: (s.sub + 1 + k) % k });
        else if (n) commit({ type: 'hover', index: (s.hover + 1 + n) % n, closeOpen: true });
      } else if (e.key === 'ArrowUp' || e.key === 'ArrowLeft') {
        if (s.open >= 0) commit({ type: 'sub', index: (s.sub - 1 + k) % k });
        else if (n) commit({ type: 'hover', index: (s.hover - 1 + n) % n, closeOpen: true });
      } else if (e.key === 'Tab') {
        e.preventDefault();
        const next = [...MODES.slice(MODES.indexOf(s.mode) + 1), ...MODES].find((m) => s.desc![m].length);
        if (next) setMode(next);
        commit({ type: 'hover', index: 0, closeOpen: true });
      } else if ((e.key === 'Enter' || e.key === ' ') && s.hover >= 0) {
        const it = list[s.hover];
        if (!it || it.compatible === false) return;
        const children = it.children ?? [];
        if (s.open >= 0 && children[s.sub]) return chooseSub(it, children[s.sub]);
        if (children.length) {
          commit({ type: 'open', index: s.hover });
          commit({ type: 'sub', index: 0 });
          return;
        }
        return choose(it);
      }
    };

    document.addEventListener('mousemove', onMove);
    document.addEventListener('click', onClick);
    document.addEventListener('wheel', onWheel, { passive: false });
    document.addEventListener('keydown', onKey);
    window.addEventListener('blur', onBlur);
    return () => {
      offs.forEach((off) => off());
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('click', onClick);
      document.removeEventListener('wheel', onWheel);
      document.removeEventListener('keydown', onKey);
      window.removeEventListener('blur', onBlur);
    };
  }, [bumpIdle, choose, chooseSub, commit, drop, fail, load, over, setMode, stopTimers]);

  useEffect(() => stopTimers, [stopTimers]);

  const srcExt = useMemo(() => sourceExtLabel(state.paths), [state.paths]);
  const hoveredItem = state.hover >= 0 ? items[state.hover] : undefined;

  const livePair = useMemo(() => {
    if (state.centre || microJob) return '';
    if (hoveredItem && hoveredItem.compatible !== false && hoveredItem.kind === 'convert' && srcExt) {
      return `${srcExt} → ${hoveredItem.id.toUpperCase()}`;
    }
    return '';
  }, [state.centre, microJob, hoveredItem, srcExt]);

  const caption = useMemo(() => {
    if (microJob) {
      return microJob.state === 'done'
        ? `✓ ${t('doneLabel')}`
        : `${Math.round(microJob.fraction * 100)}%`;
    }
    if (state.centre) return t('cancel');
    if (state.sub >= 0 && kids[state.sub]) return zl(kids[state.sub]);
    if (hoveredItem) {
      return hoveredItem.compatible === false ? t('incompatibleFormat') : zl(hoveredItem);
    }
    if (!state.desc) return t('wheelEmpty');
    if (state.desc.mixed) return t('mixed');
    if (state.paths.length > 1) return `${state.paths.length} ${t('files')}`;
    return shortLabel(state.paths[0]?.split(/[\\/]/).pop() ?? t(state.mode), 20);
  }, [microJob, state.centre, state.sub, hoveredItem, state.desc, state.mode, state.paths, kids, t, zl]);

  const kicker = microJob ? (microJob.state === 'run' ? t('converting') : '') : state.centre || !state.desc ? '' : t(state.mode);

  const closeForm = () => {
    commit({ type: 'form', choice: null });
    bumpIdle();
  };

  return (
    <div
      className="relative select-none"
      style={{ width: WHEEL_PX, height: WHEEL_PX, background: 'rgba(0, 0, 0, 0.01)' }}
      onDragEnter={(e) => {
        e.preventDefault();
        if (e.dataTransfer) {
          e.dataTransfer.dropEffect = e.dataTransfer.effectAllowed === 'move' ? 'move' : 'copy';
        }
      }}
      onDragOver={(e) => {
        // Accept Move when OS holds Shift alongside Copy/Link so WebView2 never aborts the drop.
        e.preventDefault();
        if (e.dataTransfer) {
          e.dataTransfer.dropEffect = e.dataTransfer.effectAllowed === 'move' ? 'move' : 'copy';
        }
      }}
      onDrop={(e) => {
        e.preventDefault();
      }}
    >
      <svg
        id="wheel"
        className="wheel-svg"
        viewBox="0 0 480 480"
        role="menu"
        aria-label="Zeidex wheel: modes, choices and options"
        onDragStart={(e) => e.preventDefault()}
      >
        <defs>
          <radialGradient id="wheel-disc-fill" cx="38%" cy="26%" r="82%">
            <stop offset="0%" stopColor="var(--card)" />
            <stop offset="100%" stopColor="var(--card)" stopOpacity="0.88" />
          </radialGradient>
        </defs>

        {/* Solid translucent hit-test surface so WebView2 never treats transparent gaps as click-through. */}
        <circle cx={C} cy={C} r={238} fill="rgba(0, 0, 0, 0.01)" className="wheel-hitbox" />

        <g className="wheel-root-group">
          <circle cx={C} cy={C} r={234} className="wheel-disc" />
          <circle cx={C} cy={C} r={230.5} className="wheel-rim" />
          <circle cx={C} cy={C} r={ITEMS_R[1]} className="ring-line" />
          <circle cx={C} cy={C} r={CHILD_R[1]} className="ring-line" />

          {/* Outer ring: the three modes. */}
          <g className="ring-modes">
            {MODES.map((m, i) => (
              <path
                key={m}
                data-seg={`mode-${m}`}
                d={arcPath(i, MODES.length, MODES_R[0], MODES_R[1])}
                className={`seg mode${m === state.mode ? ' current' : ''}${i === state.chip ? ' hot' : ''}`}
                fillRule="evenodd"
              />
            ))}
            {MODES.map((m, i) => {
              const [x, y] = polar((MODES_R[0] + MODES_R[1]) / 2 - 3, midAngle(i, MODES.length));
              const tilt = modeTilt(i, MODES.length);
              return (
                <text
                  key={`label-${m}`}
                  x={x}
                  y={y}
                  transform={tilt ? `rotate(${tilt} ${x} ${y})` : undefined}
                  className={`lbl mode${m === state.mode ? ' on' : ''}${i === state.chip && m !== state.mode ? ' hot' : ''}`}
                  textAnchor="middle"
                  dominantBaseline="middle"
                >
                  {t(m)}
                </text>
              );
            })}
          </g>

          {/* Middle ring: choices with magnetic wedge snapping via CSS transforms. */}
          <g key={state.mode} className={`ring-items${state.open >= 0 ? ' dim' : ''}`}>
            {items.map((it, i) => {
              const active = i === state.hover && it.compatible !== false;
              const pop = wedgePopTransform(i, items.length, active);
              return (
                <g key={`${it.kind}-${it.id}`} className="wheel-wedge" style={pop ? { transform: pop } : undefined}>
                  <path
                    data-seg={`item-${it.id}`}
                    data-compatible={it.compatible === false ? 'false' : 'true'}
                    d={arcPath(i, items.length, ITEMS_R[0], ITEMS_R[1])}
                    className={`seg${active ? ' hot' : ''}${it.compatible === false ? ' dim-incompatible' : ''}${it.pinned && it.compatible !== false ? ' pinned' : ''}`}
                    fillRule="evenodd"
                  />
                  {(() => {
                    const [x, y] = polar((ITEMS_R[0] + ITEMS_R[1]) / 2, midAngle(i, items.length));
                    return (
                      <text
                        x={x}
                        y={y}
                        className={`lbl${active ? ' hot' : ''}${it.compatible === false ? ' dim-incompatible' : ''}`}
                        textAnchor="middle"
                        dominantBaseline="middle"
                      >
                        {shortLabel(zl(it), itemLabelMax(items.length))}
                      </text>
                    );
                  })()}
                  {(it.children ?? []).length ? (
                    <circle
                      cx={polar(ITEMS_R[0] + 10, midAngle(i, items.length))[0]}
                      cy={polar(ITEMS_R[0] + 10, midAngle(i, items.length))[1]}
                      r={3}
                      className="dot"
                    />
                  ) : null}
                </g>
              );
            })}
          </g>

          {/* Inner ring: nested sub-ring or dashed hint. */}
          {kids.length ? (
            <g key={`kids-${state.open}`} className="ring-children">
              {kids.map((k, i) => {
                const active = i === state.sub;
                const pop = wedgePopTransform(i, kids.length, active);
                const [x, y] = polar((CHILD_R[0] + CHILD_R[1]) / 2, midAngle(i, kids.length));
                return (
                  <g
                    key={`${k.key}-${k.value}-${k.label}`}
                    className="wheel-wedge"
                    style={pop ? { transform: pop } : undefined}
                  >
                    <path
                      data-seg={`sub-${k.key}-${k.value}`}
                      d={arcPath(i, kids.length, CHILD_R[0], CHILD_R[1])}
                      className={`seg sub${active ? ' hot' : ''}`}
                      fillRule="evenodd"
                    />
                    <text
                      x={x}
                      y={y}
                      className={`lbl sub${active ? ' hot' : ''}`}
                      textAnchor="middle"
                      dominantBaseline="middle"
                    >
                      {shortLabel(zl(k), childLabelMax(kids.length))}
                    </text>
                  </g>
                );
              })}
            </g>
          ) : (
            <circle key="hint" cx={C} cy={C} r={(CHILD_R[0] + CHILD_R[1]) / 2} className="hintring" />
          )}

          {/* Hub: cancel, live EXT -> TARGET pill, or micro-progress ring. */}
          <circle
            cx={C}
            cy={C}
            r={HUB_R}
            className={microJob?.state === 'done' ? 'hub done' : state.centre ? 'hub cancel' : 'hub'}
          />
          {microJob && (
            <circle
              cx={C}
              cy={C}
              r={HUB_R - 4}
              fill="none"
              stroke={microJob.state === 'done' ? 'var(--ok)' : 'var(--accent)'}
              strokeWidth={4}
              strokeLinecap="round"
              strokeDasharray={RING_CIRCUMFERENCE}
              strokeDashoffset={RING_CIRCUMFERENCE * (1 - microJob.fraction)}
              transform={`rotate(-90 ${C} ${C})`}
              style={{ transition: 'stroke-dashoffset 140ms linear' }}
            />
          )}
          {livePair ? (
            <g>
              <rect x={C - 44} y={C - 22} width={88} height={20} rx={10} className="hub-pill" />
              <text
                x={C}
                y={C - 11}
                className="hub-pill-txt"
                textAnchor="middle"
                dominantBaseline="middle"
                direction="ltr"
                style={{ unicodeBidi: 'isolate' }}
              >
                {shortLabel(livePair, 14)}
              </text>
            </g>
          ) : kicker ? (
            <text x={C} y={C - 9} className="hub-kicker" textAnchor="middle">
              {shortLabel(kicker, 14)}
            </text>
          ) : null}
          <text
            x={C}
            y={livePair || kicker ? C + 13 : C + 4}
            className={state.centre ? 'cap cancel' : 'cap'}
            textAnchor="middle"
          >
            {shortLabel(caption, 16)}
          </text>
        </g>
      </svg>

      {microJob?.state === 'done' && microJob.output && (
        <div className="pointer-events-auto absolute inset-x-0 bottom-3 flex justify-center">
          <button
            type="button"
            id="wheelRevealBtn"
            className="wheel-toast-btn primary text-xs shadow-md"
            onClick={async () => {
              await reveal(microJob.output!);
              setMicroJob(null);
              void hideWheel();
            }}
          >
            {t('revealInFolder')}
          </button>
        </div>
      )}

      {runError && (
        <p className="wheel-err" role="alert" aria-live="assertive">
          {runError}
        </p>
      )}

      {state.form && (
        <div className="absolute inset-0 grid place-items-center">
          <ParamsForm
            id="params"
            choice={state.form}
            onCancel={() => {
              closeForm();
              void hideWheel();
            }}
            onSubmit={async (opts) => {
              const s = latest.current;
              if (!s.form) return;
              await launchJob(s.paths, s.form.kind, s.form.id, opts);
              closeForm();
            }}
          />
        </div>
      )}
    </div>
  );
}
