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
        code: r#"/** Structural `==` for lists, maps, structs, enums, `Option` and `Result`. */
export function eq(a: unknown, b: unknown): boolean {
  if (a === b) {
    return true;
  }
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) {
    return false;
  }
  if (Array.isArray(a)) {
    return Array.isArray(b) && a.length === b.length && a.every((x, i) => eq(x, b[i]));
  }
  if (a instanceof Map) {
    if (!(b instanceof Map) || a.size !== b.size) {
      return false;
    }
    for (const [k, v] of a) {
      if (!b.has(k) || !eq(v, b.get(k))) {
        return false;
      }
    }
    return true;
  }
  if (Object.getPrototypeOf(a) !== Object.getPrototypeOf(b)) {
    return false;
  }
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  if (ka.length !== kb.length) {
    return false;
  }
  const ra = a as Record<string, unknown>;
  const rb = b as Record<string, unknown>;
  return ka.every((k) => eq(ra[k], rb[k]));
}"#,
    },
    Piece {
        name: "copy",
        deps: &[],
        code: r#"/** `.copy()`: a deep copy that keeps classes. */
export function copy<T>(value: T): T {
  if (typeof value !== "object" || value === null) {
    return value;
  }
  if (Array.isArray(value)) {
    return value.map(copy) as T;
  }
  if (value instanceof Map) {
    const out = new Map();
    for (const [k, v] of value) {
      out.set(copy(k), copy(v));
    }
    return out as T;
  }
  const out = Object.create(Object.getPrototypeOf(value)) as Record<string, unknown>;
  for (const [k, v] of Object.entries(value)) {
    out[k] = copy(v);
  }
  return out as T;
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
        deps: &["report"],
        code: r#"/** Runs `main`. */
export function runMain(main: () => void): void {
  try {
    main();
  } catch (error) {
    report(error);
  }
}"#,
    },
    Piece {
        name: "runMainResult",
        deps: &["report"],
        code: r#"/** Runs a `main` that returns `Result<Void, Error>`. */
export function runMainResult(main: () => { ok: boolean; error?: { message(): string } }): void {
  try {
    const result = main();
    if (!result.ok && result.error !== undefined) {
      process.stderr.write("error: " + result.error.message() + "\n");
      process.exitCode = 1;
    }
  } catch (error) {
    report(error);
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
        deps: &["compare", "order"],
        code: r#"/** Stable sort by `compare`. */
export function listSort<T>(items: T[]): void {
  items.sort((a, b) => order(compare(a, b)));
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
        deps: &["compare", "copy"],
        code: r#"export function listMin<T>(items: T[]): T | undefined {
  let best: T | undefined;
  for (const item of items) {
    if (best === undefined || compare(item, best) === "Less") {
      best = item;
    }
  }
  return best === undefined ? undefined : copy(best);
}"#,
    },
    Piece {
        name: "listMax",
        deps: &["compare", "copy"],
        code: r#"export function listMax<T>(items: T[]): T | undefined {
  let best: T | undefined;
  for (const item of items) {
    if (best === undefined || compare(item, best) === "Greater") {
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
