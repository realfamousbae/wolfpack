export function total(m: Map<string, number>): number {
  let sum = 0;
  for (const v of m.values()) {
    sum += v;
  }
  return sum;
}

export function counts(words: string[]): Map<string, number> {
  const out = new Map<string, number>();
  for (const w of words) {
    out.set(w, (out.get(w) ?? 0) + 1);
  }
  return out;
}

export function halves(m: Map<number, number>): Map<number, number> {
  return new Map([...m].map(([k, v]) => [k * 2, v / 2]));
}

export function split(ns: number[]): Map<boolean, number[]> {
  return new Map([
    [true, ns.filter((n) => n % 2 === 0)],
    [false, ns.filter((n) => n % 2 !== 0)],
  ]);
}

// A plain object: its keys are strings.
export function letters(s: string): Record<string, number> {
  const out: Record<string, number> = {};
  for (const c of s) {
    out[c] = (out[c] ?? 0) + 1;
  }
  return out;
}

export function nest(m: Map<string, Map<string, number>>): Map<string, Map<string, number>> {
  return new Map([...m].map(([k, v]) => [k, new Map([...v].reverse())]));
}

export function width(ms: Map<string, number>[]): number {
  return ms.reduce((n, m) => n + m.size, 0);
}
