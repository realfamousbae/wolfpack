//! The TypeScript runtime: support code generated into every output project (spec chapter 8,
//! section 2, rule 2).
//!
//! The runtime is split into pieces. Each piece has a name, the names of the pieces it needs, and
//! its TypeScript text. The backend records the pieces generated code uses; only those pieces and
//! their dependencies are written into `src/wolf_rt.ts`.

/// A piece of the runtime.
pub struct Piece {
    /// The name generated code refers to.
    pub name: &'static str,
    /// Pieces this one uses.
    pub deps: &'static [&'static str],
    /// The TypeScript source.
    pub code: &'static str,
}

/// Every piece, in the order they are written.
pub const PIECES: &[Piece] = &[
    Piece {
        name: "host",
        deps: &[],
        code: r#"// The parts of Node.js that the runtime uses.
declare const process: {
  stdout: { write(text: string): boolean };
  stderr: { write(text: string): boolean };
  exitCode: number | undefined;
  argv: string[];
  env: Record<string, string | undefined>;
  getBuiltinModule(id: string): any;
};"#,
    },
    Piece {
        name: "Option",
        deps: &[],
        code: r#"/** An `Option` stored as data. */
export type Option<T> = { some: true; value: T } | { some: false };"#,
    },
    Piece {
        name: "some",
        deps: &["Option"],
        code: r#"export function some<T>(value: T): Option<T> {
  return { some: true, value };
}"#,
    },
    Piece {
        name: "none",
        deps: &["Option"],
        code: r#"export const none: Option<never> = { some: false };"#,
    },
    Piece {
        name: "tagged",
        deps: &["some", "none"],
        code: r#"/** An `Option` returned by a function (`T | undefined`) as data. */
export function tagged<T>(value: T | undefined): Option<T> {
  return value === undefined ? none : some(value);
}"#,
    },
    Piece {
        name: "untag",
        deps: &["Option"],
        code: r#"/** An `Option` stored as data as a function result (`T | undefined`). */
export function untag<T>(option: Option<T>): T | undefined {
  return option.some ? option.value : undefined;
}"#,
    },
    Piece {
        name: "Result",
        deps: &[],
        code: r#"/** A `Result`. */
export type Result<T, E> = { ok: true; value: T } | { ok: false; error: E };"#,
    },
    Piece {
        name: "ok",
        deps: &["Result"],
        code: r#"export function ok<T>(value: T): Result<T, never> {
  return { ok: true, value };
}"#,
    },
    Piece {
        name: "err",
        deps: &["Result"],
        code: r#"export function err<E>(error: E): Result<never, E> {
  return { ok: false, error };
}"#,
    },
    Piece {
        name: "Test",
        deps: &[],
        code: r#"/** A test: its name (file and test name) and its body. */
export type Test = { name: string; run: () => void };"#,
    },
    Piece {
        name: "Ref",
        deps: &[],
        code: r#"/** A place passed to a `mut` parameter. */
export type Ref<T> = { v: T };"#,
    },
    Piece {
        name: "viewOf",
        deps: &["Ref"],
        code: r#"/**
 * The wrapper class of a payload-less enum, at `proto`, whose value is the place `ref`: a
 * `mut self` method of an interface changes the place itself.
 */
export function viewOf<C>(proto: object, ref: Ref<unknown>): C {
  return Object.create(proto, {
    v: {
      get: () => ref.v,
      set: (x: unknown) => {
        ref.v = x;
      },
      enumerable: true,
    },
  }) as C;
}"#,
    },
    Piece {
        name: "withDicts",
        deps: &[],
        code: r#"/**
 * `value` with the dictionaries that an implementation of an interface takes from the head of its
 * block (`Bag<T: Named>: Named`), set where the type of the value is known. A dictionary of a
 * type is the same for every value, so setting it again changes nothing; `==` skips them.
 */
export function withDicts<T extends object>(value: T, dicts: object): T {
  return Object.assign(value, dicts);
}"#,
    },
    Piece {
        name: "become",
        deps: &[],
        code: r#"/**
 * `self = value` in a `mut self` method of an enum with payloads: the object takes the variant
 * and the fields of `value`, so that the place that holds it sees the new value. The ownership
 * rules guarantee that `value` holds no part of the object.
 */
export function become<T extends object>(target: T, value: T): void {
  if (target === value) {
    return;
  }
  const fields = target as Record<string, unknown>;
  for (const k of Object.keys(fields)) {
    delete fields[k];
  }
  Object.setPrototypeOf(target, Object.getPrototypeOf(value));
  Object.assign(target, value);
}"#,
    },
    Piece {
        name: "Ordering",
        deps: &[],
        code: r#"type Ordering = "Less" | "Equal" | "Greater";"#,
    },
    Piece {
        name: "WolfPanic",
        deps: &[],
        code: r#"/** A panic: the program stops with this message and exit code 101. */
export class WolfPanic extends Error {}

/** `std.process.exit`. */
export class WolfExit extends Error {
  constructor(readonly code: number) {
    super(`exit(${code}) was called`);
  }
}"#,
    },
    Piece {
        name: "panic",
        deps: &["WolfPanic"],
        code: r#"/** `panic(message)` at a source location. */
export function panic(message: string, location: string): never {
  throw new WolfPanic(`panic at ${location}: ${message}`);
}"#,
    },
    Piece {
        name: "fail",
        deps: &["WolfPanic"],
        code: r#"/** A panic raised by the language itself, without a location. */
export function fail(message: string): never {
  throw new WolfPanic(`panic: ${message}`);
}"#,
    },
    Piece {
        name: "unreachable",
        deps: &[],
        code: r#"/** The error for code that the compiler knows is never reached. */
export function unreachable(): Error {
  return new Error("unreachable code reached; please report this compiler bug");
}"#,
    },
    Piece {
        name: "assert",
        deps: &["WolfPanic"],
        code: r#"/** `assert(condition)`: a panic at the location of the call. */
export function assert(condition: boolean, text: string, location: string): void {
  if (!condition) {
    throw new WolfPanic(`panic at ${location}: assertion failed: ${text}`);
  }
}"#,
    },
    Piece {
        name: "int",
        deps: &["fail"],
        code: r#"const MIN = -(2n ** 63n);
const MAX = 2n ** 63n - 1n;

function int(value: bigint): bigint {
  if (value < MIN || value > MAX) {
    fail("integer overflow");
  }
  return value;
}"#,
    },
    Piece {
        name: "add",
        deps: &["int"],
        code: r#"export function add(a: bigint, b: bigint): bigint {
  return int(a + b);
}"#,
    },
    Piece {
        name: "sub",
        deps: &["int"],
        code: r#"export function sub(a: bigint, b: bigint): bigint {
  return int(a - b);
}"#,
    },
    Piece {
        name: "mul",
        deps: &["int"],
        code: r#"export function mul(a: bigint, b: bigint): bigint {
  return int(a * b);
}"#,
    },
    Piece {
        name: "neg",
        deps: &["int"],
        code: r#"export function neg(a: bigint): bigint {
  return int(-a);
}"#,
    },
    Piece {
        name: "div",
        deps: &["int"],
        code: r#"/** Division truncating toward zero. */
export function div(a: bigint, b: bigint): bigint {
  if (b === 0n) {
    fail("division by zero");
  }
  return int(a / b);
}"#,
    },
    Piece {
        name: "rem",
        deps: &["int"],
        code: r#"/** Remainder with the sign of the dividend. */
export function rem(a: bigint, b: bigint): bigint {
  if (b === 0n) {
    fail("division by zero");
  }
  if (b === -1n && a === MIN) {
    fail("integer overflow");
  }
  return a % b;
}"#,
    },
    Piece {
        name: "shl",
        deps: &["int"],
        code: r#"/** `<<`: the bits shifted out are discarded. */
export function shl(a: bigint, n: bigint): bigint {
  if (n < 0n || n > 63n) {
    fail("shift amount out of range");
  }
  return BigInt.asIntN(64, a << n);
}"#,
    },
    Piece {
        name: "shr",
        deps: &["int"],
        code: r#"/** `>>`: arithmetic shift. */
export function shr(a: bigint, n: bigint): bigint {
  if (n < 0n || n > 63n) {
    fail("shift amount out of range");
  }
  return a >> n;
}"#,
    },
    Piece {
        name: "index",
        deps: &["fail"],
        code: r#"function index(i: bigint, length: number): number {
  if (i < 0n || i >= BigInt(length)) {
    fail(`index ${i} out of range for length ${length}`);
  }
  return Number(i);
}"#,
    },
    Piece {
        name: "at",
        deps: &["index"],
        code: r#"/** `items[i]` */
export function at<T>(items: T[], i: bigint): T {
  return items[index(i, items.length)] as T;
}"#,
    },
    Piece {
        name: "setAt",
        deps: &["index"],
        code: r#"/** `items[i] = value` */
export function setAt<T>(items: T[], i: bigint, value: T): void {
  items[index(i, items.length)] = value;
}"#,
    },
    Piece {
        name: "refAt",
        deps: &["at", "setAt", "Ref"],
        code: r#"/** `items[i]` as a place for a `mut` parameter. */
export function refAt<T>(items: T[], i: bigint): Ref<T> {
  return {
    get v() {
      return at(items, i);
    },
    set v(value: T) {
      setAt(items, i, value);
    },
  };
}"#,
    },
    Piece {
        name: "mapGet",
        deps: &["fail"],
        code: r#"/** `map[key]` */
export function mapGet<K, V>(map: Map<K, V>, key: K): V {
  if (!map.has(key)) {
    fail("key not found in map");
  }
  return map.get(key) as V;
}"#,
    },
    Piece {
        name: "mapReplace",
        deps: &["fail"],
        code: r#"/** `map[key] = value`: replaces the value of an existing key. */
export function mapReplace<K, V>(map: Map<K, V>, key: K, value: V): void {
  if (!map.has(key)) {
    fail("key not found in map");
  }
  map.set(key, value);
}"#,
    },
    Piece {
        name: "refKey",
        deps: &["mapGet", "mapReplace", "Ref"],
        code: r#"/** `map[key]` as a place for a `mut` parameter. */
export function refKey<K, V>(map: Map<K, V>, key: K): Ref<V> {
  return {
    get v() {
      return mapGet(map, key);
    },
    set v(value: V) {
      mapReplace(map, key, value);
    },
  };
}"#,
    },
    Piece {
        name: "bounds",
        deps: &["fail"],
        code: r#"function bounds(start: bigint | undefined, end: bigint | undefined, inclusive: boolean, length: number): [number, number] {
  const a = start ?? 0n;
  const b = end === undefined ? BigInt(length) : inclusive ? end + 1n : end;
  if (a < 0n || b > BigInt(length) || a > b) {
    fail(`slice ${a}..${b} out of range for length ${length}`);
  }
  return [Number(a), Number(b)];
}"#,
    },
    Piece {
        name: "sliceList",
        deps: &["bounds", "copy"],
        code: r#"/** `items[a..b]`: a new list of copies. */
export function sliceList<T>(items: T[], start: bigint | undefined, end: bigint | undefined, inclusive: boolean): T[] {
  const [a, b] = bounds(start, end, inclusive, items.length);
  return items.slice(a, b).map(copy);
}"#,
    },
    Piece {
        name: "sliceStr",
        deps: &["bounds"],
        code: r#"/** `text[a..b]`, counting Unicode scalar values. */
export function sliceStr(text: string, start: bigint | undefined, end: bigint | undefined, inclusive: boolean): string {
  const chars = Array.from(text);
  const [a, b] = bounds(start, end, inclusive, chars.length);
  return chars.slice(a, b).join("");
}"#,
    },
    Piece {
        name: "eq",
        deps: &[],
        code: r#"/**
 * Structural `==` for lists, maps, structs, enums, `Option` and `Result`. It keeps the pairs of
 * parts still to compare in a list instead of calling itself, so the depth of a value does not
 * matter. Fields whose names start with `$` are skipped.
 */
export function eq(a: unknown, b: unknown): boolean {
  const pending: unknown[] = [a, b];
  while (pending.length > 0) {
    const y = pending.pop();
    const x = pending.pop();
    if (x === y) {
      continue;
    }
    if (typeof x !== "object" || typeof y !== "object" || x === null || y === null) {
      return false;
    }
    if (Array.isArray(x)) {
      if (!Array.isArray(y) || x.length !== y.length) {
        return false;
      }
      for (let i = 0; i < x.length; i++) {
        pending.push(x[i], y[i]);
      }
    } else if (x instanceof Map) {
      if (!(y instanceof Map) || x.size !== y.size) {
        return false;
      }
      for (const [k, v] of x) {
        if (!y.has(k)) {
          return false;
        }
        pending.push(v, y.get(k));
      }
    } else {
      if (Object.getPrototypeOf(x) !== Object.getPrototypeOf(y)) {
        return false;
      }
      // The dictionaries of a generic type (`$asNamedT`) are not part of its value.
      const own = (o: object) => Object.keys(o).filter((k) => !k.startsWith("$"));
      const keys = own(x);
      if (keys.length !== own(y).length) {
        return false;
      }
      const rx = x as Record<string, unknown>;
      const ry = y as Record<string, unknown>;
      for (const k of keys) {
        pending.push(rx[k], ry[k]);
      }
    }
  }
  return true;
}"#,
    },
    Piece {
        name: "copy",
        deps: &[],
        code: r#"/**
 * `.copy()`: a deep copy that keeps classes. Each list, map or object is first copied with the
 * parts of the original in it, and its parts are then replaced by their own copies, one at a
 * time, so the depth of a value does not matter.
 */
export function copy<T>(value: T): T {
  const out = copyOneLevel(value);
  const pending: unknown[] = out === value ? [] : [out];
  for (let next = pending.pop(); next !== undefined; next = pending.pop()) {
    if (Array.isArray(next)) {
      for (let i = 0; i < next.length; i++) {
        const part = copyOneLevel(next[i]);
        if (part !== next[i]) {
          next[i] = part;
          pending.push(part);
        }
      }
    } else if (next instanceof Map) {
      for (const [k, v] of next) {
        const part = copyOneLevel(v);
        if (part !== v) {
          next.set(k, part);
          pending.push(part);
        }
      }
    } else {
      const parts = next as Record<string, unknown>;
      for (const k of Object.keys(parts)) {
        const part = copyOneLevel(parts[k]);
        if (part !== parts[k]) {
          parts[k] = part;
          pending.push(part);
        }
      }
    }
  }
  return out;
}

/** A new list, map or object whose parts are still those of `value`; other values as they are. */
function copyOneLevel<T>(value: T): T {
  if (typeof value !== "object" || value === null) {
    return value;
  }
  if (Array.isArray(value)) {
    return value.slice() as T;
  }
  if (value instanceof Map) {
    return new Map(value) as T;
  }
  return Object.assign(Object.create(Object.getPrototypeOf(value)), value) as T;
}"#,
    },
    Piece {
        name: "cmpStr",
        deps: &[],
        code: r#"/** Compares strings by Unicode scalar values (not by UTF-16 units). */
export function cmpStr(a: string, b: string): number {
  const x = Array.from(a);
  const y = Array.from(b);
  for (let i = 0; i < x.length && i < y.length; i++) {
    const p = x[i]!.codePointAt(0)!;
    const q = y[i]!.codePointAt(0)!;
    if (p !== q) {
      return p < q ? -1 : 1;
    }
  }
  return x.length - y.length;
}"#,
    },
    Piece {
        name: "compare",
        deps: &["cmpStr", "Ordering"],
        code: r#"/** `a.compare(b)` for `Comparable` values: `Int`, `String` or a type with `compare`. */
export function compare<T>(a: T, b: T): Ordering {
  if (typeof a === "bigint" && typeof b === "bigint") {
    return a < b ? "Less" : a > b ? "Greater" : "Equal";
  }
  if (typeof a === "string" && typeof b === "string") {
    const c = cmpStr(a, b);
    return c < 0 ? "Less" : c > 0 ? "Greater" : "Equal";
  }
  return (a as unknown as { compare(other: T): Ordering }).compare(b);
}"#,
    },
    Piece {
        name: "display",
        deps: &["floatToString", "quote"],
        code: r#"/**
 * `x.to_string()` for `Display` values: built-in types, a type with `toString`, and the lists,
 * maps, options and results that generic code holds without their types (spec chapter 7,
 * section 4.1): a string inside them is quoted.
 */
export function display(x: unknown): string {
  if (typeof x === "number") {
    return floatToString(x);
  }
  if (typeof x === "bigint" || typeof x === "boolean" || typeof x === "string") {
    return String(x);
  }
  const part = (v: unknown): string => (typeof v === "string" ? quote(v) : display(v));
  if (Array.isArray(x)) {
    return "[" + x.map(part).join(", ") + "]";
  }
  if (x instanceof Map) {
    if (x.size === 0) {
      return "[:]";
    }
    const parts: string[] = [];
    for (const [k, v] of x) {
      parts.push(part(k) + ": " + part(v));
    }
    return "[" + parts.join(", ") + "]";
  }
  const o = x as { toString(): string; message(): string };
  if (typeof x === "object" && x !== null && o.toString === Object.prototype.toString) {
    const r = x as { some?: boolean; ok?: boolean; value?: unknown; error?: unknown };
    if (typeof r.some === "boolean") {
      return r.some ? "Some(" + part(r.value) + ")" : "None";
    }
    if (typeof r.ok === "boolean") {
      if (r.ok) {
        return "Ok(" + part(r.value) + ")";
      }
      const e = r.error as { toString(): string; message(): string };
      const shown =
        typeof r.error === "object" && r.error !== null && e.toString === Object.prototype.toString
          ? e.message()
          : part(r.error);
      return "Err(" + shown + ")";
    }
  }
  return o.toString();
}"#,
    },
    Piece {
        name: "quote",
        deps: &[],
        code: r#"/** A `String` inside a collection: in quotes and escaped (spec chapter 7, section 4.1). */
export function quote(s: string): string {
  let out = '"';
  for (const c of s) {
    const n = c.codePointAt(0) ?? 0;
    if (c === '"') {
      out += '\\"';
    } else if (c === "\\") {
      out += "\\\\";
    } else if (c === "\n") {
      out += "\\n";
    } else if (c === "\r") {
      out += "\\r";
    } else if (c === "\t") {
      out += "\\t";
    } else if (n === 0) {
      out += "\\0";
    } else if (n < 0x20 || n === 0x7f) {
      out += "\\u{" + n.toString(16) + "}";
    } else {
      out += c;
    }
  }
  return out + '"';
}"#,
    },
    Piece {
        name: "Shown",
        deps: &[],
        code: r#"/**
 * A value of the interface `Display` whose text needs its type: a `String`, which a collection
 * writes without quotes, or a list, a map, an option or a result.
 */
export class Shown<T> {
  v: T;
  f: (x: T) => string;

  constructor(v: T, f: (x: T) => string) {
    this.v = v;
    this.f = f;
  }

  toString(): string {
    return this.f(this.v);
  }
}"#,
    },
    Piece {
        name: "part",
        deps: &["display", "quote"],
        code: r#"/** The text of a part of type `T`, from the dictionary of `T`: a `String` is quoted. */
export function part<T>(show: (x: T) => string): (x: T) => string {
  if (show !== (display as unknown)) {
    return show;
  }
  return (x: T) => (typeof x === "string" ? quote(x) : display(x));
}"#,
    },
    Piece {
        name: "listText",
        deps: &[],
        code: r#"/** `[1, 2, 3]` */
export function listText<T>(items: T[], part: (x: T) => string): string {
  return "[" + items.map(part).join(", ") + "]";
}"#,
    },
    Piece {
        name: "mapText",
        deps: &[],
        code: r#"/** `["Ann": 31]`, `[:]` when empty. */
export function mapText<K, V>(
  m: Map<K, V>,
  key: (x: K) => string,
  value: (x: V) => string,
): string {
  if (m.size === 0) {
    return "[:]";
  }
  const parts: string[] = [];
  for (const [k, v] of m) {
    parts.push(key(k) + ": " + value(v));
  }
  return "[" + parts.join(", ") + "]";
}"#,
    },
    Piece {
        name: "optionText",
        deps: &["Option"],
        code: r#"/** `Some(...)` or `None`. */
export function optionText<T>(o: Option<T>, part: (x: T) => string): string {
  return o.some ? "Some(" + part(o.value) + ")" : "None";
}"#,
    },
    Piece {
        name: "resultText",
        deps: &["Result"],
        code: r#"/** `Ok(...)` or `Err(...)`. */
export function resultText<T, E>(
  r: Result<T, E>,
  part: (x: T) => string,
  err: (e: E) => string,
): string {
  return r.ok ? "Ok(" + part(r.value) + ")" : "Err(" + err(r.error) + ")";
}"#,
    },
    Piece {
        name: "errText",
        deps: &[],
        code: r#"/**
 * The text of an error whose type is known only to implement `Error`: its `to_string` if its
 * class has one (`Display`), else its message.
 */
export function errText(e: unknown): string {
  const o = e as { toString(): string; message(): string };
  if (typeof e === "object" && e !== null && o.toString !== Object.prototype.toString) {
    return o.toString();
  }
  return o.message();
}"#,
    },
    Piece {
        name: "fixed",
        deps: &["floatToString"],
        code: r#"/**
 * `x` with `digits` digits after the point, rounded to the nearest with ties to even on its exact
 * value (spec chapter 7, section 4.4); `toFixed` rounds halves up and has no plain notation from
 * 1e21 on.
 */
export function fixed(x: number, digits: bigint): string {
  if (!Number.isFinite(x)) {
    return floatToString(x);
  }
  const view = new DataView(new ArrayBuffer(8));
  view.setFloat64(0, Math.abs(x));
  const bits = view.getBigUint64(0);
  const exp = Number((bits >> 52n) & 0x7ffn);
  let mantissa = bits & ((1n << 52n) - 1n);
  let e = -1074;
  if (exp !== 0) {
    mantissa |= 1n << 52n;
    e = exp - 1075;
  }
  const scaled = mantissa * 10n ** digits;
  let q: bigint;
  if (e >= 0) {
    q = scaled << BigInt(e);
  } else {
    const den = 1n << BigInt(-e);
    q = scaled / den;
    const twice = (scaled % den) * 2n;
    if (twice > den || (twice === den && (q & 1n) === 1n)) {
      q += 1n;
    }
  }
  const p = Number(digits);
  let s = q.toString();
  if (p > 0) {
    s = s.padStart(p + 1, "0");
    s = s.slice(0, s.length - p) + "." + s.slice(s.length - p);
  }
  return (x < 0 || Object.is(x, -0) ? "-" : "") + s;
}"#,
    },
    Piece {
        name: "pad",
        deps: &[],
        code: r#"/**
 * `text` padded with `fill` to `width` Unicode scalar values: `align` 0 pads on the right, 1 on
 * both sides with the extra character on the right, 2 on the left. `zero` pads a number with
 * zeros after its sign, and `NaN` and the infinities with spaces on the left.
 */
export function pad(text: string, width: bigint, fill: string, align: bigint, zero: boolean): string {
  const n = [...text].length;
  const missing = Number(width) - n;
  if (missing <= 0) {
    return text;
  }
  if (zero) {
    if (text === "NaN" || text === "inf" || text === "-inf") {
      return " ".repeat(missing) + text;
    }
    const sign = text.startsWith("-") ? "-" : "";
    return sign + "0".repeat(missing) + text.slice(sign.length);
  }
  if (align === 0n) {
    return text + fill.repeat(missing);
  }
  if (align === 1n) {
    const left = Math.floor(missing / 2);
    return fill.repeat(left) + text + fill.repeat(missing - left);
  }
  return fill.repeat(missing) + text;
}"#,
    },
    Piece {
        name: "floatToString",
        deps: &[],
        code: r#"/** The canonical format of `Float` (spec chapter 7, section 4.1). */
export function floatToString(x: number): string {
  if (Number.isNaN(x)) {
    return "NaN";
  }
  if (x === Infinity) {
    return "inf";
  }
  if (x === -Infinity) {
    return "-inf";
  }
  if (x === 0) {
    return Object.is(x, -0) ? "-0.0" : "0.0";
  }
  const text = String(x).replace("e+", "e");
  return text.includes(".") || text.includes("e") ? text : text + ".0";
}"#,
    },
    Piece {
        name: "print",
        deps: &["host"],
        code: r#"export function print(text: string): void {
  process.stdout.write(text + "\n");
}"#,
    },
    Piece {
        name: "write",
        deps: &["host"],
        code: r#"export function write(text: string): void {
  process.stdout.write(text);
}"#,
    },
    Piece {
        name: "eprint",
        deps: &["host"],
        code: r#"export function eprint(text: string): void {
  process.stderr.write(text + "\n");
}"#,
    },
    Piece {
        name: "readLine",
        deps: &["host"],
        code: r#"let input: string[] | undefined;

/** A line of standard input without its terminator, or `undefined` at the end. */
export function readLine(): string | undefined {
  if (input === undefined) {
    const fs = process.getBuiltinModule("node:fs");
    const text: string = fs.readFileSync(0, "utf8");
    input = text.split(/\r?\n/);
    if (input[input.length - 1] === "") {
      input.pop();
    }
  }
  return input.shift();
}"#,
    },
    Piece {
        name: "args",
        deps: &["host"],
        code: r#"export function args(): string[] {
  return process.argv.slice(2);
}"#,
    },
    Piece {
        name: "env",
        deps: &["host"],
        code: r#"export function env(name: string): string | undefined {
  return process.env[name];
}"#,
    },
    Piece {
        name: "nodeFs",
        deps: &["host"],
        code: r#"/** The `node:fs` module. */
export function nodeFs(): any {
  return process.getBuiltinModule("node:fs");
}"#,
    },
    Piece {
        name: "exit",
        deps: &["WolfPanic", "fail"],
        code: r#"export function exit(code: bigint): never {
  if (code < 0n || code > 255n) {
    fail(`exit code ${code} out of range`);
  }
  throw new WolfExit(Number(code));
}"#,
    },
    Piece {
        name: "replace",
        deps: &["Ref"],
        code: r#"/** `replace(mut place, own value)`: stores the value and returns the old one. */
export function replace<T>(place: Ref<T>, value: T): T {
  const old = place.v;
  place.v = value;
  return old;
}"#,
    },
    Piece {
        name: "report",
        deps: &["host", "WolfPanic"],
        code: r#"function report(error: unknown): void {
  if (error instanceof WolfExit) {
    process.exitCode = error.code;
  } else if (error instanceof WolfPanic) {
    process.stderr.write(error.message + "\n");
    process.exitCode = 101;
  } else if (error instanceof RangeError) {
    process.stderr.write("panic: stack overflow\n");
    process.exitCode = 101;
  } else if (error instanceof Error) {
    // An exception of extern code is a panic (spec chapter 7, section 8.4).
    process.stderr.write("panic: " + error.message + "\n");
    process.exitCode = 101;
  } else {
    throw error;
  }
}"#,
    },
    Piece {
        name: "runMain",
        deps: &["report", "args"],
        code: r#"/** Runs `main`; a `main` with a parameter gets the command-line arguments. */
export function runMain(main: (args: string[]) => void): void {
  try {
    main(args());
  } catch (error) {
    report(error);
  }
}"#,
    },
    Piece {
        name: "runMainResult",
        deps: &["report", "args"],
        code: r#"/** Runs a `main` that returns `Result<Void, Error>`. */
export function runMainResult(main: (args: string[]) => { ok: boolean; error?: { message(): string } }): void {
  try {
    const result = main(args());
    if (!result.ok && result.error !== undefined) {
      process.stderr.write("error: " + result.error.message() + "\n");
      process.exitCode = 1;
    }
  } catch (error) {
    report(error);
  }
}"#,
    },
    Piece {
        name: "runTest",
        deps: &["WolfPanic"],
        code: r#"/**
 * Runs the body of a test. `exit` fails the test with its code instead of ending the run (spec
 * chapter 8, section 2, rule 7).
 */
export function runTest(run: () => void): void {
  try {
    run();
  } catch (error) {
    if (error instanceof WolfExit) {
      throw new Error(`the test exited with code ${error.code}`);
    }
    throw error;
  }
}"#,
    },
    // ----- std intrinsics ---------------------------------------------------------------------
    Piece {
        name: "strLen",
        deps: &[],
        code: r#"/** The length in Unicode scalar values. */
export function strLen(text: string): bigint {
  let n = 0n;
  for (const _ of text) {
    n++;
  }
  return n;
}"#,
    },
    Piece {
        name: "strFind",
        deps: &[],
        code: r#"/** The position of the first occurrence, in scalar values. */
export function strFind(text: string, part: string): bigint | undefined {
  const i = text.indexOf(part);
  return i < 0 ? undefined : BigInt(Array.from(text.slice(0, i)).length);
}"#,
    },
    Piece {
        name: "strSplit",
        deps: &["fail"],
        code: r#"export function strSplit(text: string, separator: string): string[] {
  if (separator === "") {
    fail("empty separator");
  }
  return text.split(separator);
}"#,
    },
    Piece {
        name: "strLines",
        deps: &[],
        code: r#"export function strLines(text: string): string[] {
  const lines = text.split(/\r?\n/);
  if (lines[lines.length - 1] === "") {
    lines.pop();
  }
  return lines;
}"#,
    },
    Piece {
        name: "strTrim",
        deps: &[],
        code: r#"/** Removes ASCII whitespace at both ends. */
export function strTrim(text: string): string {
  return text.replace(/^[ \t\n\r\f\v]+|[ \t\n\r\f\v]+$/g, "");
}"#,
    },
    Piece {
        name: "strReplace",
        deps: &["fail"],
        code: r#"export function strReplace(text: string, from: string, to: string): string {
  if (from === "") {
    fail("empty pattern");
  }
  return text.split(from).join(to);
}"#,
    },
    Piece {
        name: "strRepeat",
        deps: &["fail"],
        code: r#"export function strRepeat(text: string, times: bigint): string {
  if (times < 0n) {
    fail("negative repeat count");
  }
  return text.repeat(Number(times));
}"#,
    },
    Piece {
        name: "strUpper",
        deps: &[],
        code: r#"/** Upper-cases ASCII letters only. */
export function strUpper(text: string): string {
  return text.replace(/[a-z]/g, (c) => c.toUpperCase());
}"#,
    },
    Piece {
        name: "strLower",
        deps: &[],
        code: r#"/** Lower-cases ASCII letters only. */
export function strLower(text: string): string {
  return text.replace(/[A-Z]/g, (c) => c.toLowerCase());
}"#,
    },
    Piece {
        name: "parseIntText",
        deps: &["int"],
        code: r#"/** The value of an optional `-` and decimal digits, or `undefined`. */
export function parseIntText(text: string): bigint | undefined {
  if (!/^-?[0-9]+$/.test(text)) {
    return undefined;
  }
  const value = BigInt(text);
  return value < MIN || value > MAX ? undefined : value;
}"#,
    },
    Piece {
        name: "parseFloatText",
        deps: &[],
        code: r#"/** The value of the float literal syntax with an optional `-`, `NaN` or `inf`, or `undefined`. */
export function parseFloatText(text: string): number | undefined {
  if (text === "NaN") {
    return NaN;
  }
  if (text === "inf") {
    return Infinity;
  }
  if (text === "-inf") {
    return -Infinity;
  }
  if (!/^-?[0-9](_?[0-9])*(\.[0-9](_?[0-9])*)?([eE][+-]?[0-9](_?[0-9])*)?$/.test(text)) {
    return undefined;
  }
  return Number(text.replace(/_/g, ""));
}"#,
    },
    Piece {
        name: "listInsert",
        deps: &["fail"],
        code: r#"export function listInsert<T>(items: T[], i: bigint, item: T): void {
  if (i < 0n || i > BigInt(items.length)) {
    fail(`index ${i} out of range for length ${items.length}`);
  }
  items.splice(Number(i), 0, item);
}"#,
    },
    Piece {
        name: "listRemove",
        deps: &["index"],
        code: r#"export function listRemove<T>(items: T[], i: bigint): T {
  return items.splice(index(i, items.length), 1)[0] as T;
}"#,
    },
    Piece {
        name: "listSwap",
        deps: &["index"],
        code: r#"export function listSwap<T>(items: T[], i: bigint, j: bigint): void {
  const a = index(i, items.length);
  const b = index(j, items.length);
  const t = items[a] as T;
  items[a] = items[b] as T;
  items[b] = t;
}"#,
    },
    Piece {
        name: "listCount",
        deps: &[],
        code: r#"export function listCount<T>(items: T[], f: (item: T) => boolean): bigint {
  return BigInt(items.filter(f).length);
}"#,
    },
    Piece {
        name: "listContains",
        deps: &["eq"],
        code: r#"export function listContains<T>(items: T[], item: T): boolean {
  return items.some((x) => eq(x, item));
}"#,
    },
    Piece {
        name: "listIndexOf",
        deps: &["eq"],
        code: r#"export function listIndexOf<T>(items: T[], item: T): bigint | undefined {
  const i = items.findIndex((x) => eq(x, item));
  return i < 0 ? undefined : BigInt(i);
}"#,
    },
    Piece {
        name: "listGet",
        deps: &["copy"],
        code: r#"export function listGet<T>(items: T[], i: bigint): T | undefined {
  return i < 0n || i >= BigInt(items.length) ? undefined : copy(items[Number(i)] as T);
}"#,
    },
    Piece {
        name: "order",
        deps: &["Ordering"],
        code: r#"function order(o: Ordering): number {
  return o === "Less" ? -1 : o === "Greater" ? 1 : 0;
}"#,
    },
    Piece {
        name: "listSort",
        deps: &["compare", "order", "Ordering"],
        code: r#"/** Stable sort by `compare`, or by `cmp` for a payload-less enum (a string at run time). */
export function listSort<T>(items: T[], cmp: (a: T, b: T) => Ordering = compare): void {
  items.sort((a, b) => order(cmp(a, b)));
}"#,
    },
    Piece {
        name: "listSortBy",
        deps: &["order", "Ordering"],
        code: r#"/** Stable sort by a comparison function. */
export function listSortBy<T>(items: T[], f: (a: T, b: T) => Ordering): void {
  items.sort((a, b) => order(f(a, b)));
}"#,
    },
    Piece {
        name: "listMin",
        deps: &["compare", "copy", "Ordering"],
        code: r#"export function listMin<T>(
  items: T[],
  cmp: (a: T, b: T) => Ordering = compare,
): T | undefined {
  let best: T | undefined;
  for (const item of items) {
    if (best === undefined || cmp(item, best) === "Less") {
      best = item;
    }
  }
  return best === undefined ? undefined : copy(best);
}"#,
    },
    Piece {
        name: "listMax",
        deps: &["compare", "copy", "Ordering"],
        code: r#"export function listMax<T>(
  items: T[],
  cmp: (a: T, b: T) => Ordering = compare,
): T | undefined {
  let best: T | undefined;
  for (const item of items) {
    if (best === undefined || cmp(item, best) === "Greater") {
      best = item;
    }
  }
  return best === undefined ? undefined : copy(best);
}"#,
    },
    Piece {
        name: "listSum",
        deps: &["int"],
        code: r#"export function listSum(items: bigint[]): bigint {
  let total = 0n;
  for (const item of items) {
    total = int(total + item);
  }
  return total;
}"#,
    },
    Piece {
        name: "listRange",
        deps: &[],
        code: r#"export function listRange(from: bigint, to: bigint): bigint[] {
  const out: bigint[] = [];
  for (let i = from; i < to; i++) {
    out.push(i);
  }
  return out;
}"#,
    },
    Piece {
        name: "mapInsert",
        deps: &[],
        code: r#"/** Inserts or replaces; returns the replaced value. */
export function mapInsert<K, V>(map: Map<K, V>, key: K, value: V): V | undefined {
  const old = map.get(key);
  map.set(key, value);
  return old;
}"#,
    },
    Piece {
        name: "mapRemove",
        deps: &[],
        code: r#"export function mapRemove<K, V>(map: Map<K, V>, key: K): V | undefined {
  const old = map.get(key);
  map.delete(key);
  return old;
}"#,
    },
    Piece {
        name: "mapGetCopy",
        deps: &["copy"],
        code: r#"export function mapGetCopy<K, V>(map: Map<K, V>, key: K): V | undefined {
  return map.has(key) ? copy(map.get(key) as V) : undefined;
}"#,
    },
    Piece {
        name: "mapValues",
        deps: &["copy"],
        code: r#"export function mapValues<K, V>(map: Map<K, V>): V[] {
  return [...map.values()].map(copy);
}"#,
    },
    Piece {
        name: "intAbs",
        deps: &["int"],
        code: r#"export function intAbs(a: bigint): bigint {
  return int(a < 0n ? -a : a);
}"#,
    },
    Piece {
        name: "intPow",
        deps: &["int", "fail"],
        code: r#"export function intPow(a: bigint, exp: bigint): bigint {
  if (exp < 0n) {
    fail("negative exponent");
  }
  let result = 1n;
  for (let i = 0n; i < exp; i++) {
    result = int(result * a);
  }
  return result;
}"#,
    },
    Piece {
        name: "floatToInt",
        deps: &["int", "fail"],
        code: r#"export function floatToInt(x: number): bigint {
  if (!Number.isFinite(x)) {
    fail("float to int conversion of a non-finite value");
  }
  return int(BigInt(Math.trunc(x)));
}"#,
    },
    Piece {
        name: "floatMin",
        deps: &[],
        code: r#"export function floatMin(a: number, b: number): number {
  return Number.isNaN(a) ? b : Number.isNaN(b) ? a : Math.min(a, b);
}"#,
    },
    Piece {
        name: "floatMax",
        deps: &[],
        code: r#"export function floatMax(a: number, b: number): number {
  return Number.isNaN(a) ? b : Number.isNaN(b) ? a : Math.max(a, b);
}"#,
    },
    Piece {
        name: "floatRound",
        deps: &[],
        code: r#"/** Rounds half away from zero. */
export function floatRound(x: number): number {
  return x < 0 ? -Math.round(-x) : Math.round(x);
}"#,
    },
    Piece {
        name: "toNumber",
        deps: &["fail"],
        code: r#"/** An `Int` passed to extern code as a `number`. */
export function toNumber(value: bigint): number {
  const n = Number(value);
  if (!Number.isSafeInteger(n)) {
    fail("integer too large for extern code");
  }
  return n;
}"#,
    },
    Piece {
        name: "same",
        deps: &[],
        code: r#"/** A value that crosses the extern boundary as it is. */
export function same<T>(value: T): T {
  return value;
}"#,
    },
    Piece {
        name: "toExternMap",
        deps: &[],
        code: r#"/** A `Map` passed to extern code: a new map with its keys and values converted. */
export function toExternMap<K, V, K2, V2>(
  map: Map<K, V>,
  key: (k: K) => K2,
  value: (v: V) => V2,
): Map<K2, V2> {
  return new Map([...map].map(([k, v]) => [key(k), value(v)]));
}"#,
    },
    Piece {
        name: "fromExternMap",
        deps: &["cmpStr"],
        code: r#"/** A map returned by extern code, a `Map` or a plain object: converted and sorted by key. */
export function fromExternMap<K, V>(
  map: unknown,
  key: (k: any) => K,
  value: (v: any) => V,
): Map<K, V> {
  const entries: [unknown, unknown][] =
    map instanceof Map ? [...map] : Object.entries(map as object);
  const out = entries.map(([k, v]) => [key(k), value(v)] as [K, V]);
  out.sort(([a], [b]) =>
    typeof a === "string" && typeof b === "string" ? cmpStr(a, b) : a < b ? -1 : a > b ? 1 : 0,
  );
  return new Map(out);
}"#,
    },
    Piece {
        name: "textError",
        deps: &[],
        code: r#"/** `error(message)`: an `Error` with this message. */
export function textError(text: string): { message(): string } {
  return { message: () => text };
}"#,
    },
    Piece {
        name: "externError",
        deps: &["WolfPanic"],
        code: r#"/** An exception of extern code as an `Error`, with its text. A panic or an exit of a callback
 * passes through (spec chapter 7, section 8.4, rule 7). */
export function externError(e: unknown): { message(): string } {
  if (e instanceof WolfPanic || e instanceof WolfExit) {
    throw e;
  }
  const text = e instanceof Error ? e.message : String(e);
  return { message: () => text };
}"#,
    },
    Piece {
        name: "fromNullable",
        deps: &[],
        code: r#"/** An optional value of extern code: `null` and `undefined` are `None`. */
export function fromNullable<T, U>(value: T | null | undefined, f: (v: T) => U): U | undefined {
  return value === null || value === undefined ? undefined : f(value);
}"#,
    },
    Piece {
        name: "fromNumber",
        deps: &["int", "fail"],
        code: r#"/** A `number` returned by extern code as an `Int`. */
export function fromNumber(value: number): bigint {
  if (!Number.isInteger(value)) {
    fail("extern code returned a number that is not an integer");
  }
  return int(BigInt(value));
}"#,
    },
];

/// The runtime file with the given pieces and their dependencies, in a fixed order.
pub fn file(used: &std::collections::BTreeSet<&'static str>) -> String {
    let mut needed: std::collections::BTreeSet<&'static str> = std::collections::BTreeSet::new();
    let mut stack: Vec<&'static str> = used.iter().copied().collect();
    while let Some(name) = stack.pop() {
        if !needed.insert(name) {
            continue;
        }
        if let Some(piece) = PIECES.iter().find(|p| p.name == name) {
            stack.extend(piece.deps.iter().copied());
        }
    }
    let mut out = String::new();
    for piece in PIECES {
        if needed.contains(piece.name) {
            out.push_str(piece.code);
            out.push_str("\n\n");
        }
    }
    out
}
