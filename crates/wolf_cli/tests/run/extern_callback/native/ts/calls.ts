export function each(items: string[], f: (s: string) => void): void {
  for (const s of items) {
    f(s);
  }
}

export function twice(n: number, f: (n: number) => number): number {
  return f(f(n));
}

export function mapped(xs: number[], f: (n: number) => string): string[] {
  return xs.map(f);
}

export function count(items: string[], f: (s: string) => boolean): number {
  if (items.length === 0) {
    throw new Error("nothing to count");
  }
  return items.filter(f).length;
}
