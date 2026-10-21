export interface Point {
  x: number;
  y: number;
}

export interface Label {
  text: string;
  maxSize: number;
  tags: string[];
  at: Point;
}

export function distance(a: Point, b: Point): number {
  return Math.hypot(b.x - a.x, b.y - a.y);
}

export function moved(p: Point, dx: number): Point {
  return { x: p.x + dx, y: p.y };
}

export function describe(l: Label): string {
  return `${l.text} (${l.maxSize}) ${l.tags.join(",")} at ${l.at.x},${l.at.y}`;
}

let grownCalls = 0;

export function grown(l: Label): Label {
  grownCalls++;
  return { ...l, text: l.text.toUpperCase(), maxSize: l.maxSize * 2, tags: [...l.tags, "c"] };
}

export function corners(n: number): Point[] {
  return Array.from({ length: n }, (_, i) => ({ x: i, y: 10 - i }));
}

export function find(name: string): Point | undefined {
  return name === "origin" ? { x: 0, y: 0 } : undefined;
}

export function shifted(m: Map<string, Point>): Map<string, Point> {
  const out = new Map<string, Point>();
  for (const [k, p] of m) {
    out.set(k, { x: p.x + 1, y: p.y });
  }
  return out;
}

export function calls(): number {
  return grownCalls;
}
