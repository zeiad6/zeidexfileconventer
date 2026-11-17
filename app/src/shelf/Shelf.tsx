/**
 * The orb: a drop target for sessions without global keys (Wayland) and the Float shelf.
 * Dragging files over it opens the wheel around it; files dropped on the wheel's hub are held
 * here (badge) and can be dragged out of the orb into any other app.
 */

import { motion } from 'framer-motion';
import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react';
import {
  bridge,
  dragIcon,
  floatAdd,
  floatClear,
  floatList,
  onDragDrop,
  openInHome,
  safeListen,
  showHome,
  wheelAtShelf,
} from '../lib/api';
import { useI18n } from '../lib/i18n';

/** Pixels the pointer must travel before a press becomes a file drag. */
const DRAG_START_PX = 6;

export function Shelf() {
  const { t } = useI18n();
  const [held, setHeld] = useState(0);
  const [over, setOver] = useState(false);
  const press = useRef<{ x: number; y: number } | null>(null);

  useEffect(
    () =>
      onDragDrop(async (p) => {
        if (p.type === 'enter' || p.type === 'over') setOver(true);
        else if (p.type === 'leave') setOver(false);
        if (p.type === 'enter') void wheelAtShelf(p.paths);
        if (p.type === 'drop') {
          setOver(false);
          await floatAdd(p.paths);
        }
      }),
    [],
  );

  useEffect(() => {
    const off = safeListen<number>('float-changed', (n) => setHeld(n));
    floatList()
      .then((files) => setHeld(files.length))
      .catch(() => {});
    return off;
  }, []);

  const onPointerDown = (e: ReactPointerEvent) => {
    if (held && e.button === 0) press.current = { x: e.clientX, y: e.clientY };
  };

  const onPointerMove = async (e: ReactPointerEvent) => {
    const start = press.current;
    if (!start || Math.hypot(e.clientX - start.x, e.clientY - start.y) < DRAG_START_PX) return;
    press.current = null;
    const files = await floatList();
    if (!files.length) return;
    const icon = await dragIcon();
    const drag = bridge().drag;
    if (!drag) return;
    await drag.startDrag({ item: files, icon }, (ev) => {
      if (ev.result === 'Dropped') void floatClear();
    });
  };

  return (
    <div
      id="orb"
      className={`orb${over ? ' over' : ''}`}
      title="Zeidex Convert"
      data-tauri-drag-region={held ? undefined : ''}
      aria-label="Zeidex floating shelf"
      onPointerDown={onPointerDown}
      onPointerUp={() => {
        press.current = null;
      }}
      onPointerMove={onPointerMove}
      onDoubleClick={async () => {
        const files = held ? await floatList() : [];
        if (files.length) void openInHome(files);
        else void showHome();
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        void floatClear();
      }}
    >
      <motion.span
        initial={false}
        animate={{ rotate: held ? [0, -8, 8, 0] : 0 }}
        transition={{ duration: 0.4 }}
        aria-hidden="true"
      >
        ⇄
      </motion.span>
      {held > 0 && (
        <span id="badge" className="orb-badge" aria-label={`${held} ${t('selected')}`}>
          {held}
        </span>
      )}
    </div>
  );
}
