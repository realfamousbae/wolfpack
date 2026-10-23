export function loose(x): number {
  return x;
}

export async function later(): Promise<number> {
  return 1;
}

export function keyed(m: Map<number, number>): number {
  return m.size;
}

export class Counter {
  constructor() {}

  add(n: number): void;
  add(n: string): void;
  add(n: number | string): void {}

  keyed(m: Map<number, number>): number {
    return m.size;
  }
}
