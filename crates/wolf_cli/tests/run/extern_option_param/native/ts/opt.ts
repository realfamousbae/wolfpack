export interface Point {
  x: number;
  y: number;
}

export class Counter {
  n = 7;
}

export function counter(): Counter {
  return new Counter();
}

export function intOr(n: number | undefined, d: number): number {
  return n ?? d;
}

export function floatOr(x: number | undefined): number {
  return x ?? -1;
}

export function textOr(s: string | undefined): string {
  return s ?? "none";
}

export function count(l: string[] | undefined): number {
  return l === undefined ? -1 : l.length;
}

export function total(m: Map<string, number> | undefined): number {
  if (m === undefined) {
    return -1;
  }
  let t = 0;
  for (const v of m.values()) {
    t += v;
  }
  return t;
}

export function xOf(p: Point | undefined): number {
  return p === undefined ? -1 : p.x;
}

export function ticks(c: Counter | undefined): number {
  return c === undefined ? -1 : c.n;
}
