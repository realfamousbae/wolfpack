// The TypeScript code of the native module native.text.slug.

export interface Size {
  width: bigint;
  height: number;
  tags: readonly string[];
  maxSize: bigint;
}

export type Frame = { size: Size; name: string };

export class Counter {
  n: bigint;

  constructor(start: bigint) {
    this.n = start;
  }

  static zero(): Counter {
    return new Counter(0n);
  }

  value(): bigint {
    return this.n;
  }

  get doubled(): bigint {
    return this.n * 2n;
  }
}

export function slugify(title: string): string {
  return title.toLowerCase().replaceAll(" ", "-");
}

export function parseURL(s: string): string {
  return s + "rl";
}

export function halve(x: number): number {
  return x / 2;
}

export function sum(v: bigint[]): bigint {
  return v.reduce((a, b) => a + b, 0n);
}

export function words(v: readonly string[]): string[] {
  return v.map((s) => s.toUpperCase());
}

export function keys(m: Map<string, bigint>): Map<string, bigint> {
  return new Map([...m].map(([k, v]) => [k, v * 2n]));
}

export function total(m: ReadonlyMap<string, bigint>): bigint {
  let t = 0n;
  for (const v of m.values()) {
    t += v;
  }
  return t;
}

export function counts(): Record<string, bigint> {
  return { b: 2n, a: 1n };
}

export function first(v: string[]): string | undefined {
  return v[0];
}

export function lookup(name: string): string | null {
  return name === "x" ? "found" : null;
}

export function orZero(n?: bigint): bigint {
  return n ?? 0n;
}

export function newCounter(): Counter {
  return new Counter(7n);
}

export function count(c: Counter): bigint {
  return c.n;
}

export function each(items: string[], f: (s: string) => boolean): bigint {
  return BigInt(items.filter(f).length);
}

export function area(s: Size): number {
  return Number(s.width) * s.height;
}

export function grow(s: Size, by: bigint): Size {
  return { ...s, width: s.width + by, tags: [...s.tags, "grown"] };
}

export function frames(n: bigint): Frame[] {
  const out: Frame[] = [];
  for (let i = 0n; i < n; i++) {
    out.push({ size: { width: i, height: 0.5, tags: [], maxSize: 9n }, name: `f${i}` });
  }
  return out;
}

export function findFrame(name: string): Frame | undefined {
  return frames(3n).find((f) => f.name === name);
}

export function fail(): bigint {
  throw new Error("boom");
}
