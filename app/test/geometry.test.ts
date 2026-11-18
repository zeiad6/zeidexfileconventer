import { describe, expect, it } from 'vitest';
import {
  C,
  CHILD_R,
  HUB_R,
  ITEMS_R,
  MODES_R,
  WHEEL_PX,
  arcPath,
  childLabelMax,
  hitTest,
  itemLabelMax,
  midAngle,
  polar,
  polarAngleDeg,
  shortLabel,
  sourceExtLabel,
  wedgePopTransform,
} from '../src/lib/geometry';

/** Middle of segment `i` of `n` on the ring at radius `r`, starting at 12 o'clock. */
const at = (i: number, n: number, r: number) => {
  const angle = -Math.PI / 2 + ((i + 0.5) * 2 * Math.PI) / n;
  return { x: C + r * Math.cos(angle), y: C + r * Math.sin(angle) };
};

describe('wheel geometry', () => {
  it('starts the first segment at twelve o’clock and numbers clockwise', () => {
    // Segment 0 of 4 spans -90°…0°, so its mid-angle is -45°; the *start edge* is straight up.
    expect(midAngle(0, 4)).toBeCloseTo(-Math.PI / 4, 6);
    const [xt, yt] = polar(100, -Math.PI / 2);
    expect(xt).toBeCloseTo(C, 6);
    expect(yt).toBeCloseTo(C - 100, 6);
    // Clockwise: segment 1 of 4 is the bottom-right quadrant, not the top-left one.
    const [x1, y1] = polar(100, midAngle(1, 4));
    expect(x1).toBeGreaterThan(C);
    expect(y1).toBeGreaterThan(C);
  });

  it('hit-tests the three rings and the hub', () => {
    const items = at(1, 10, (ITEMS_R[0] + ITEMS_R[1]) / 2);
    expect(hitTest(items, { itemCount: 10, childCount: 3 })).toEqual({ item: 1 });

    const chips = at(2, 3, (MODES_R[0] + MODES_R[1]) / 2);
    expect(hitTest(chips, { itemCount: 10, childCount: 3 })).toEqual({ chip: 2 });

    const kids = at(0, 3, (CHILD_R[0] + CHILD_R[1]) / 2);
    expect(hitTest(kids, { itemCount: 10, childCount: 3 })).toEqual({ sub: 0 });

    expect(hitTest({ x: C, y: C }, { itemCount: 10, childCount: 3 })).toEqual({ centre: true });
  });

  it('ignores the sub-ring when nothing has children', () => {
    const kids = at(0, 3, (CHILD_R[0] + CHILD_R[1]) / 2);
    // The band between hub and items is empty without children, so it hits nothing.
    expect(hitTest(kids, { itemCount: 10, childCount: 0 })).toEqual({});
  });

  it('outside every ring hits nothing', () => {
    expect(hitTest({ x: C + 260, y: C }, { itemCount: 10, childCount: 3 })).toEqual({});
    expect(hitTest({ x: C, y: C - HUB_R - 1 }, { itemCount: 0, childCount: 0 })).toEqual({});
  });

  it('scales physical pixels down by the device pixel ratio', () => {
    const point = at(0, 10, (ITEMS_R[0] + ITEMS_R[1]) / 2);
    expect(hitTest({ x: point.x * 2, y: point.y * 2 }, { dpr: 2, itemCount: 10, childCount: 0 })).toEqual({
      item: 0,
    });
  });

  it('builds a full ring for a single segment', () => {
    const path = arcPath(0, 1, CHILD_R[0], CHILD_R[1]);
    expect(path.startsWith('M')).toBe(true);
    expect(path.match(/A/g)?.length).toBe(4); // two arcs per circle, inner and outer
    expect(path.endsWith('Z')).toBe(true);
  });

  it('ellipsises long labels instead of clipping them', () => {
    expect(shortLabel('WEBM', 6)).toBe('WEBM');
    expect(shortLabel('Very long label', 6)).toBe('Very …');
    expect(itemLabelMax(3)).toBe(16);
    expect(itemLabelMax(6)).toBe(13);
    expect(itemLabelMax(12)).toBe(10);
    expect(childLabelMax(3)).toBe(12);
    expect(childLabelMax(9)).toBe(9);
  });

  it('computes polar angles, magnetic pop transforms, and source extension labels for 380px wheel', () => {
    expect(WHEEL_PX).toBe(380);
    expect(polarAngleDeg({ x: C, y: C - 100 })).toBeCloseTo(0, 4);
    expect(polarAngleDeg({ x: C + 100, y: C })).toBeCloseTo(90, 4);
    expect(polarAngleDeg({ x: C, y: C + 100 })).toBeCloseTo(180, 4);
    expect(polarAngleDeg({ x: C - 100, y: C })).toBeCloseTo(270, 4);
    expect(wedgePopTransform(0, 8, false)).toBeUndefined();
    expect(wedgePopTransform(0, 8, true)).toMatch(/^translate\(/);
    expect(sourceExtLabel(['/v/movie.mkv'])).toBe('MKV');
    expect(sourceExtLabel(['/v/a.mkv', '/v/b.mkv'])).toBe('MKV');
    expect(sourceExtLabel(['/v/a.mkv', '/i/b.png'])).toBe('2');
  });
});
