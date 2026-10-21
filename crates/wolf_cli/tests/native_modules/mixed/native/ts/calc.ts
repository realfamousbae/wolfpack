export interface Point {
  x: number;
  count: number;
}

export function scale(n: number, by: number): number {
  return n * by;
}

export function mean(v: number[]): number {
  return v.reduce((a, b) => a + b, 0) / v.length;
}

export function bump(p: Point): Point {
  return { x: p.x, count: p.count + 1 };
}

export function label(n: number): string {
  return `#${n}`;
}
