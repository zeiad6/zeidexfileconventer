/**
 * The three-ring radial menu shown under the pointer while files are dragged.
 *
 *   outer   modes      Convert / Tools / Actions (dwell or scroll to morph the ring)
 *   middle  choices    formats, tools or actions for the current mode
 *   inner   sub-ring   quality presets or a tool's default options
 *   hub     cancel     dropping here abandons the job
 *
 * Native drag events drive the wheel, so a drag needs no click: hover a segment and release the
 * button. Keyboard/click mode (Ctrl+Shift+Space) adds arrows, Tab, Enter, Escape and the mouse
 * wheel. Framer Motion animates the ring swap, which is what makes it read as one wheel morphing
 * instead of a redraw.
 */

import { AnimatePresence, motion } from 'framer-motion';
import { useCallback, useEffect, useMemo, useReducer, useRef } from 'react';
import { ParamsForm } from '../components/ParamsForm';
import {
  CHILD_R,
  DWELL_MS,
  HUB_R,
  IDLE_MS,
  ITEMS_R,
  LEAVE_MS,
  MODES,
  MODE_ICON,
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
  type Hit,
  type WheelMode,
} from '../lib/geometry';
import {
  describe,
  hideWheel,
  keepWheel,
  onDragDrop,
  openEditor,
  runJob,
  safeListen,
} from '../lib/api';
import { useI18n } from '../lib/i18n';
import type { Choice, Desc, Sub } from '../lib/types';

const MORPH = { duration: 0.24, ease: [0.2, 0.8, 0.2, 1] as const };
const ORIGIN = { transformOrigin: '240px 240px', transformBox: 'view-box' } as const;

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
      const mode = s.desc && s.desc[s.mode].length ? s.mode : firstFilled(a.desc);
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

export function Wheel() {
  const { t, zl } = useI18n();
  const [state, dispatch] = useReducer(reduce, INITIAL);
  const latest = useRef(state);
  latest.current = state;

  const chipTimer = useRef<number | undefined>(undefined);
  const openTimer = useRef<number | undefined>(undefined);
  const hideTimer = useRef<number | undefined>(undefined);
  const idleTimer = useRef<number | undefined>(undefined);
  const clickMode = useRef(false);

  const items = state.desc ? state.desc[state.mode] : [];
  const kids = state.open >= 0 && items[state.open] ? items[state.open].children ?? [] : [];

  const stopTimers = () => {
    window.clearTimeout(chipTimer.current);
    window.clearTimeout(openTimer.current);
    window.clearTimeout(hideTimer.current);
    window.clearTimeout(idleTimer.current);
  };

  /** Hide the wheel when the pointer rests on it and nothing is picked for a while. */
  const bumpIdle = useCallback(() => {
    window.clearTimeout(idleTimer.current);
    idleTimer.current = window.setTimeout(() => {
      if (!latest.current.form) void hideWheel();
    }, IDLE_MS);
  }, []);

  const setMode = useCallback((mode: WheelMode) => {
    const s = latest.current;
    if (mode === s.mode) return;
    dispatch({ type: 'mode', mode });
  }, []);

  /** Promise of the description plus the mode the caller should reason about. */
  const load = useCallback(async (paths: string[]) => {
    const s = latest.current;
    if (s.desc && paths.join('\n') === s.paths.join('\n')) {
      return { desc: s.desc, mode: s.mode };
    }
    const desc = await describe(paths);
    const mode = desc[s.mode].length ? s.mode : firstFilled(desc);
    // Apply the transition to the ref before React commits, so the drag handler that awaits
    // this call sees the new ring counts when it hit-tests the very same pointer position.
    latest.current = reduce(latest.current, { type: 'loaded', paths, desc });
    dispatch({ type: 'loaded', paths, desc });
    return { desc, mode };
  }, []);

  const choose = useCallback(
    (it: Choice) => {
      const s = latest.current;
      if (it.editor) {
        void openEditor(s.paths, it.id);
        return;
      }
      if (!it.params.length) {
        void runJob(s.paths, it.kind, it.id, it.preset ?? {});
        return;
      }
      void keepWheel();
      dispatch({ type: 'form', choice: it });
    },
    [],
  );

  const chooseSub = useCallback((it: Choice, child: Sub) => {
    const s = latest.current;
    if (child.custom) {
      choose(it);
      return;
    }
    const opts = { ...(it.preset ?? {}), [child.key]: child.value };
    void runJob(s.paths, it.kind, it.id, opts);
  }, [choose]);

  /** Which part of the wheel the pointer is on now, using the freshest description. */
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
      const s = latest.current;
      const h = hitOf(pos);

      bumpIdle();
      const centre = h.centre === true;
      if (centre !== s.centre) dispatch({ type: 'centre', on: centre });

      const chip = h.chip ?? -1;
      if (chip !== s.chip) {
        window.clearTimeout(chipTimer.current);
        dispatch({ type: 'chip', index: chip });
        if (chip >= 0 && MODES[chip] !== s.mode) {
          chipTimer.current = window.setTimeout(() => setMode(MODES[chip]), DWELL_MS);
        }
      }

      const item = h.item ?? -1;
      // The sub-ring band keeps its parent item selected; only the item band (or leaving it)
      // changes which item owns the sub-ring.
      if (h.item != null || h.centre || h.chip != null) {
        if (item !== s.hover) {
          const children = s.desc && s.desc[s.mode][item] ? s.desc[s.mode][item].children ?? [] : [];
          const closeOpen = s.open >= 0 && s.open !== item;
          dispatch({ type: 'hover', index: item, closeOpen });
          window.clearTimeout(openTimer.current);
          if (children.length) {
            openTimer.current = window.setTimeout(() => {
              if (latest.current.hover === item) dispatch({ type: 'open', index: item });
            }, OPEN_MS);
          }
        }
      }

      const sub = h.sub ?? -1;
      if (sub !== s.sub) dispatch({ type: 'sub', index: sub });
    },
    [bumpIdle, hitOf, setMode],
  );

  const drop = useCallback(
    async (paths: string[], pos: { x: number; y: number }) => {
      window.clearTimeout(chipTimer.current);
      window.clearTimeout(openTimer.current);
      await load(paths);
      const h = hitOf(pos);
      const s = latest.current;
      const list = s.desc ? s.desc[s.mode] : [];
      const children = s.open >= 0 && list[s.open] ? list[s.open].children ?? [] : [];
      if (h.centre) return hideWheel();
      if (h.sub != null) {
        if (!children.length || s.open < 0) return; // the sub-ring has not opened yet
        return chooseSub(list[s.open], children[h.sub]);
      }
      if (h.item != null && list[h.item]) return choose(list[h.item]);
      if (h.chip != null) return setMode(MODES[h.chip]);
      return hideWheel();
    },
    [choose, chooseSub, hitOf, load, setMode],
  );

  // Native drag-and-drop: enter loads the file types, over tracks the pointer, drop commits.
  useEffect(
    () =>
      onDragDrop(async (p) => {
        if (latest.current.form) return;
        if (p.type === 'enter') {
          window.clearTimeout(hideTimer.current);
          await load(p.paths);
          over(p.position);
        } else if (p.type === 'over') {
          over(p.position);
        } else if (p.type === 'drop') {
          await drop(p.paths, p.position);
        } else {
          dispatch({ type: 'hover', index: -1, closeOpen: false });
          dispatch({ type: 'sub', index: -1 });
          hideTimer.current = window.setTimeout(() => void hideWheel(), LEAVE_MS);
        }
      }),
    [drop, load, over],
  );

  // Click/keyboard mode (Ctrl+Shift+Space on a selection): no drag, so mouse moves, clicks,
  // the mouse wheel and the keys drive the wheel.
  useEffect(() => {
    const offs: Array<() => void> = [];
    offs.push(
      safeListen<string[]>('wheel-open', async (paths) => {
        clickMode.current = true;
        dispatch({ type: 'reset' });
        // A fresh selection must be re-described even when the paths repeat the previous drag.
        latest.current = INITIAL;
        const { desc } = await load(paths);
        const list = desc[latest.current.mode];
        if (list.length) dispatch({ type: 'hover', index: 0, closeOpen: true });
        bumpIdle();
      }),
    );

    offs.push(
      safeListen<string>('wheel-mode', (mode) => {
        if (!latest.current.form) setMode(mode as WheelMode);
      }),
    );

    offs.push(
      safeListen<void>('wheel-hidden', () => {
        clickMode.current = false;
        dispatch({ type: 'reset' });
      }),
    );

    const physical = (e: MouseEvent) => ({
      x: e.clientX * (window.devicePixelRatio || 1),
      y: e.clientY * (window.devicePixelRatio || 1),
    });
    const onMove = (e: MouseEvent) => {
      if (clickMode.current && !latest.current.form) over(physical(e));
    };
    const onClick = (e: MouseEvent) => {
      const target = e.target as Element | null;
      if (!clickMode.current || latest.current.form || target?.closest?.('.params')) return;
      void drop(latest.current.paths, physical(e));
    };
    const onBlur = () => {
      if (clickMode.current && !latest.current.form) void hideWheel();
    };
    const onWheel = (e: WheelEvent) => {
      const s = latest.current;
      if (s.form || !s.desc) return;
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
        if (s.open >= 0) {
          dispatch({ type: 'open', index: -1 });
          dispatch({ type: 'sub', index: -1 });
          return;
        }
        void hideWheel();
        return;
      }
      if (s.form || !s.desc) return;
      const list = s.desc[s.mode];
      const n = list.length;
      const k = Math.max(s.open >= 0 ? list[s.open]?.children?.length ?? 0 : 0, 1);
      if (e.key === 'ArrowDown' || e.key === 'ArrowRight') {
        if (s.open >= 0) dispatch({ type: 'sub', index: (s.sub + 1 + k) % k });
        else if (n) dispatch({ type: 'hover', index: (s.hover + 1 + n) % n, closeOpen: true });
      } else if (e.key === 'ArrowUp' || e.key === 'ArrowLeft') {
        if (s.open >= 0) dispatch({ type: 'sub', index: (s.sub - 1 + k) % k });
        else if (n) dispatch({ type: 'hover', index: (s.hover - 1 + n) % n, closeOpen: true });
      } else if (e.key === 'Tab') {
        e.preventDefault();
        const next = [...MODES.slice(MODES.indexOf(s.mode) + 1), ...MODES].find((m) => s.desc![m].length);
        if (next) setMode(next);
        dispatch({ type: 'hover', index: 0, closeOpen: true });
      } else if ((e.key === 'Enter' || e.key === ' ') && s.hover >= 0) {
        const it = list[s.hover];
        if (!it) return;
        const children = it.children ?? [];
        if (s.open >= 0 && children[s.sub]) return chooseSub(it, children[s.sub]);
        if (children.length) {
          dispatch({ type: 'open', index: s.hover });
          dispatch({ type: 'sub', index: 0 });
          return;
        }
        return choose(it);
      } else {
        return;
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
  }, [bumpIdle, choose, chooseSub, drop, load, over, setMode]);

  useEffect(() => stopTimers, []);

  const caption = useMemo(() => {
    if (state.centre) return t('cancel');
    if (state.sub >= 0 && kids[state.sub]) return zl(kids[state.sub]);
    if (state.hover >= 0 && items[state.hover]) return zl(items[state.hover]);
    if (state.desc?.mixed) return t('mixed');
    return t(state.mode);
  }, [state.centre, state.sub, state.hover, state.desc, state.mode, kids, items, t, zl]);

  const closeForm = () => dispatch({ type: 'form', choice: null });

  return (
    <div className="relative select-none" style={{ width: WHEEL_PX, height: WHEEL_PX }}>
      <svg
        id="wheel"
        className="wheel-svg"
        viewBox="0 0 480 480"
        role="menu"
        aria-label="Zeidex wheel: modes, choices and options"
        onDragStart={(e) => e.preventDefault()}
      >
        {/* Outer ring: the three modes. The current one is filled, a dwelled one is outlined. */}
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
            const [x, y] = polar((MODES_R[0] + MODES_R[1]) / 2 - 4, midAngle(i, MODES.length));
            return (
              <text
                key={`label-${m}`}
                x={x}
                y={y}
                className={`lbl${m === state.mode ? ' hot' : ''}`}
                textAnchor="middle"
                dominantBaseline="middle"
              >
                {`${MODE_ICON[m]} ${t(m)}`}
              </text>
            );
          })}
        </g>

        {/* Middle ring: the choices. Dimmed while a sub-ring is open. */}
        <motion.g
          key={state.mode}
          className={`ring-items${state.open >= 0 ? ' dim' : ''}`}
          initial={{ opacity: 0, scale: 0.9, rotate: -4 }}
          animate={{ opacity: 1, scale: 1, rotate: 0 }}
          transition={MORPH}
          style={ORIGIN}
        >
          {items.map((it, i) => (
            <path
              key={`${it.kind}-${it.id}`}
              data-seg={`item-${it.id}`}
              d={arcPath(i, items.length, ITEMS_R[0], ITEMS_R[1])}
              className={`seg${i === state.hover ? ' hot' : ''}`}
              fillRule="evenodd"
            />
          ))}
          {items.map((it, i) => {
            const [x, y] = polar((ITEMS_R[0] + ITEMS_R[1]) / 2, midAngle(i, items.length));
            return (
              <text
                key={`label-${it.kind}-${it.id}`}
                x={x}
                y={y}
                className={`lbl${i === state.hover ? ' hot' : ''}`}
                textAnchor="middle"
                dominantBaseline="middle"
              >
                {shortLabel(zl(it), itemLabelMax(items.length))}
              </text>
            );
          })}
          {items.map((it, i) =>
            (it.children ?? []).length ? (
              <circle
                key={`dot-${it.id}`}
                cx={polar(ITEMS_R[0] + 10, midAngle(i, items.length))[0]}
                cy={polar(ITEMS_R[0] + 10, midAngle(i, items.length))[1]}
                r={3}
                className="dot"
              />
            ) : null,
          )}
        </motion.g>

        {/* Inner ring: the nested sub-ring, or a dashed hint when nothing has children. The ring
            is mounted/unmounted directly rather than through AnimatePresence: an exiting group
            would keep stale segments in the DOM while the pointer is already hit-testing the
            next ring, and the wheel's segment count is part of its contract. */}
        {kids.length ? (
            <motion.g
              key={`kids-${state.open}`}
              className="ring-children"
              initial={{ opacity: 0, scale: 0.9 }}
              animate={{ opacity: 1, scale: 1 }}
              transition={{ duration: 0.2 }}
              style={ORIGIN}
            >
              {kids.map((k, i) => (
                <path
                  key={`${k.key}-${k.value}-${k.label}`}
                  data-seg={`sub-${k.key}-${k.value}`}
                  d={arcPath(i, kids.length, CHILD_R[0], CHILD_R[1])}
                  className={`seg sub${i === state.sub ? ' hot' : ''}`}
                  fillRule="evenodd"
                />
              ))}
              {kids.map((k, i) => {
                const [x, y] = polar((CHILD_R[0] + CHILD_R[1]) / 2, midAngle(i, kids.length));
                return (
                  <text
                    key={`label-${k.key}-${k.value}-${k.label}`}
                    x={x}
                    y={y}
                    className={`lbl${i === state.sub ? ' hot' : ''}`}
                    textAnchor="middle"
                    dominantBaseline="middle"
                  >
                    {shortLabel(zl(k), childLabelMax(kids.length))}
                  </text>
                );
              })}
            </motion.g>
          ) : (
            <circle key="hint" cx={240} cy={240} r={(CHILD_R[0] + CHILD_R[1]) / 2} className="hintring" />
          )}

        {/* Hub: cancel, plus the hovered choice as the caption. */}
        <circle cx={240} cy={240} r={HUB_R} className={state.centre ? 'hub cancel' : 'hub'} />
        <text x={240} y={232} className="cap" textAnchor="middle">
          {shortLabel(caption, 20)}
        </text>
        <text x={240} y={256} className="hub-x" textAnchor="middle">
          {state.centre ? '✕' : '◎'}
        </text>
      </svg>

      <AnimatePresence>
        {state.form && (
          <motion.div
            initial={{ opacity: 0, scale: 0.94 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.94 }}
            transition={{ duration: 0.14 }}
            className="absolute inset-0 grid place-items-center"
          >
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
                await runJob(s.paths, s.form.kind, s.form.id, opts);
                closeForm();
              }}
            />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
