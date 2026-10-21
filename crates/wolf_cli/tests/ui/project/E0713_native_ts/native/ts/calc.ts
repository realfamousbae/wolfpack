export function loose(x): number {
  return x;
}

export async function later(): Promise<number> {
  return 1;
}

export function keyed(m: Map<number, number>): number {
  return m.size;
}
