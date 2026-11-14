/**
 * Wheel geometry: one 480 × 480 viewBox, three concentric bands and a cancel hub. Kept pure and
 * free of React so the hit-test maths can be unit-tested without a DOM.
 *
 * Angles start at 12 o'clock and run clockwise, which is how the segments are drawn and how a
 * physical pointer position is read back.
 */

export const SIZE = 480;
/** Rendered size of the popup wheel window; the viewBox stays 480 so the maths is unchanged. */
export const WHEEL_PX = 380;
export const SCALE = WHEEL_PX / SIZE;
export const C = SIZE / 2;
export const MODES_R = [186, 228] as const;
export const ITEMS_R = [122, 180] as const;
export const CHILD_R = [56, 116] as const;
export const HUB_R = 52;
export const MODES = ['convert', 'tools', 'actions'] as const;
export type WheelMode = (typeof MODES)[number];
export const MODE_ICON: Record<WheelMode, string> = { convert: '⇄', tools: '✎', actions: '↗' };
/** Hover time on a mode segment before the wheel switches to it. */
export const DWELL_MS = 380;
/** Hover time on an item before its nested sub-ring opens. */
export const OPEN_MS = 260;
/** Grace period after the drag leaves before the wheel hides. */
export const LEAVE_MS = 900;
/** A wheel nobody touches hides itself, so it never sits on top of other windows. */
export const IDLE_MS = 4000;

export function polar(r: number, angle: number): [number, number] {
  return [C + r * Math.cos(angle), C + r * Math.sin(angle)];
}

/** Centre angle of segment `i` of `n`, measured from 12 o'clock. */
export function midAngle(i: number, n: number): number {
  return -Math.PI / 2 + ((i + 0.5) * 2 * Math.PI) / n;
}

/** Donut slice between `r0` and `r1` for segment `i` of `n`. */
export function arcPath(i: number, n: number, r0: number, r1: number): string {
  if (n === 1) {
    const ring = (r: number, sweep: number) =>
      `M ${C} ${C - r} A ${r} ${r} 0 1 ${sweep} ${C} ${C + r} A ${r} ${r} 0 1 ${sweep} ${C} ${C - r} Z`;
    return ring(r1, 1) + ring(r0, 0);
  }
  const gap = n > 16 ? 0.006 : 0.012;
  const a0 = -Math.PI / 2 + (i * 2 * Math.PI) / n + gap;
  const a1 = -Math.PI / 2 + ((i + 1) * 2 * Math.PI) / n - gap;
  const large = a1 - a0 > Math.PI ? 1 : 0;
  const [x0, y0] = polar(r1, a0);
  const [x1, y1] = polar(r1, a1);
  const [x2, y2] = polar(r0, a1);
  const [x3, y3] = polar(r0, a0);
  return `M${x0} ${y0} A${r1} ${r1} 0 ${large} 1 ${x1} ${y1} L${x2} ${y2} A${r0} ${r0} 0 ${large} 0 ${x3} ${y3} Z`;
}

export interface Hit {
  /** Pointer is on the hub: releasing cancels. */
  centre?: boolean;
  /** Segment of the nested ring (only when that ring is open). */
  sub?: number;
  /** Segment of the choices ring. */
  item?: number;
  /** Segment of the outer modes ring. */
  chip?: number;
}

export interface HitOptions {
  /** Physical pixels per CSS pixel; drag positions from the backend are physical. */
  dpr?: number;
  /** Window size divided by the 480 viewBox, for the compact popup wheel. */
  scale?: number;
  itemCount: number;
  childCount: number;
}

/** Which part of the wheel a physical pointer position lands on. */
export function hitTest(pos: { x: number; y: number }, { dpr = 1, scale = 1, itemCount, childCount }: HitOptions): Hit {
  const x = pos.x / dpr / scale;
  const y = pos.y / dpr / scale;
  const d = Math.hypot(x - C, y - C);
  let angle = Math.atan2(y - C, x - C) + Math.PI / 2;
  if (angle < 0) angle += 2 * Math.PI;
  const seg = (n: number) => Math.floor(angle / ((2 * Math.PI) / n)) % n;

  if (d <= HUB_R) return { centre: true };
  if (d >= CHILD_R[0] && d <= CHILD_R[1] && childCount) return { sub: seg(childCount) };
  if (d >= ITEMS_R[0] && d <= ITEMS_R[1] && itemCount) return { item: seg(itemCount) };
  if (d >= MODES_R[0] && d <= MODES_R[1]) return { chip: seg(MODES.length) };
  return {};
}

/** Ellipsis for long labels: the ring never clips text into the segment border. */
export function shortLabel(text: string, max: number): string {
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

/** Label width that keeps a ring readable as its segment count grows. */
export function itemLabelMax(count: number): number {
  return count > 8 ? 10 : count > 5 ? 13 : 16;
}

export function childLabelMax(count: number): number {
  return count > 6 ? 9 : 12;
}
