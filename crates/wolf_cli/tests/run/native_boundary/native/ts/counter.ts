export class Counter {
  constructor(readonly count: number) {}
}

export function start(): Counter {
  return new Counter(0);
}

export function bump(c: Counter, n: number): Counter {
  return new Counter(c.count + n);
}

export function value(c: Counter): number {
  return c.count;
}

export function parse(s: string): Counter {
  const n = Number.parseInt(s, 10);
  if (Number.isNaN(n)) {
    throw new Error(`not a number: ${s}`);
  }
  return new Counter(n);
}
