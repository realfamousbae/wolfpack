//! The Rust runtime: the module `wolf_rt`, generated into every output project (spec chapter 8,
//! section 2, rule 2).
//!
//! It also holds the declarations of std (`Ordering`, `Error`, `Comparable`, `ParseError`,
//! `Entry`, `FsError`) and its intrinsic functions. The runtime is split into pieces; only the
//! pieces a program uses and their dependencies are written.

use std::collections::BTreeSet;

/// A piece of the runtime.
pub struct Piece {
    /// The name the backend asks for.
    pub name: &'static str,
    /// Pieces this one uses.
    pub deps: &'static [&'static str],
    /// The Rust source, formatted as `rustfmt` formats it.
    pub code: &'static str,
}

/// Every piece, in the order they are written.
pub const PIECES: &[Piece] = &[
    Piece {
        name: "Panic",
        deps: &[],
        code: r#"/// The value of a panic of a Wolfpack program: its whole message.
pub struct Panic(pub String);

/// `panic(message)` at a source location.
pub fn panic_at(message: &str, location: &str) -> ! {
    std::panic::panic_any(Panic(format!("panic at {location}: {message}")))
}

/// A panic raised by the language itself, without a location.
pub fn fail(message: &str) -> ! {
    std::panic::panic_any(Panic(format!("panic: {message}")))
}

/// `assert(condition)`: a panic at the location of the call.
pub fn check(condition: bool, text: &str, location: &str) {
    if !condition {
        std::panic::panic_any(Panic(format!(
            "panic at {location}: assertion failed: {text}"
        )));
    }
}"#,
    },
    Piece {
        name: "message",
        deps: &["Panic"],
        code: r#"/// The canonical message of a panic (spec chapter 7, section 3).
fn message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(p) = payload.downcast_ref::<Panic>() {
        return p.0.clone();
    }
    let text = match payload.downcast_ref::<&str>() {
        Some(s) => s.to_string(),
        None => payload
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_default(),
    };
    match text.as_str() {
        "attempt to add with overflow"
        | "attempt to subtract with overflow"
        | "attempt to multiply with overflow"
        | "attempt to negate with overflow"
        | "attempt to divide with overflow"
        | "attempt to calculate the remainder with overflow" => {
            "panic: integer overflow".to_string()
        }
        "attempt to divide by zero"
        | "attempt to calculate the remainder with a divisor of zero" => {
            "panic: division by zero".to_string()
        }
        _ => format!("panic: {text}"),
    }
}

fn flush() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
}"#,
    },
    Piece {
        name: "run_main",
        deps: &["message"],
        code: r#"/// Runs `main`: a panic prints its message and ends the program with exit code 101.
pub fn run_main(main: fn()) {
    std::panic::set_hook(Box::new(|_| {}));
    if let Err(payload) = std::panic::catch_unwind(main) {
        flush();
        eprintln!("{}", message(&*payload));
        std::process::exit(101);
    }
}"#,
    },
    Piece {
        name: "run_main_result",
        deps: &["run_main", "Error"],
        code: r#"/// Runs a `main` that returns `Result<Void, Error>`.
pub fn run_main_result(main: fn() -> Result<(), Box<dyn Error>>) {
    std::panic::set_hook(Box::new(|_| {}));
    match std::panic::catch_unwind(main) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            flush();
            eprintln!("error: {}", e.message());
            std::process::exit(1);
        }
        Err(payload) => {
            flush();
            eprintln!("{}", message(&*payload));
            std::process::exit(101);
        }
    }
}"#,
    },
    Piece {
        name: "test",
        deps: &["message"],
        code: r#"/// Runs the body of a test; a panic fails the test with its message.
pub fn test(body: impl FnOnce()) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        println!("--- wolf failure: {}", message(&*payload));
        std::panic::resume_unwind(payload);
    }
}"#,
    },
    Piece {
        name: "exit",
        deps: &["Panic", "message"],
        code: r#"/// `std.process.exit`.
pub fn exit(code: i64) -> ! {
    if !(0..=255).contains(&code) {
        fail(&format!("exit code {code} out of range"));
    }
    flush();
    std::process::exit(code as i32)
}"#,
    },
    Piece {
        name: "int",
        deps: &["Panic"],
        code: r#"/// `a << n`: the bits shifted out are discarded.
pub fn shl(a: i64, n: i64) -> i64 {
    if !(0..=63).contains(&n) {
        fail("shift amount out of range");
    }
    a.wrapping_shl(n as u32)
}

/// `a >> n`: an arithmetic shift.
pub fn shr(a: i64, n: i64) -> i64 {
    if !(0..=63).contains(&n) {
        fail("shift amount out of range");
    }
    a >> n
}

/// `Int.pow`.
pub fn int_pow(a: i64, exp: i64) -> i64 {
    if exp < 0 {
        fail("negative exponent");
    }
    let mut result: i64 = 1;
    for _ in 0..exp {
        result *= a;
    }
    result
}"#,
    },
    Piece {
        name: "At",
        deps: &["Panic"],
        code: r#"fn index(i: i64, length: usize) -> usize {
    match usize::try_from(i) {
        Ok(k) if k < length => k,
        _ => fail(&format!("index {i} out of range for length {length}")),
    }
}

/// `items[i]` with the range check of Wolfpack.
pub trait At<T> {
    /// `items[i]`
    fn at(&self, i: i64) -> &T;
    /// `items[i]` as a place.
    fn at_mut(&mut self, i: i64) -> &mut T;
}

impl<T> At<T> for [T] {
    fn at(&self, i: i64) -> &T {
        &self[index(i, self.len())]
    }

    fn at_mut(&mut self, i: i64) -> &mut T {
        let k = index(i, self.len());
        &mut self[k]
    }
}"#,
    },
    Piece {
        name: "bounds",
        deps: &["Panic"],
        code: r#"fn bounds(start: i64, end: i64, length: usize) -> (usize, usize) {
    match (usize::try_from(start), usize::try_from(end)) {
        (Ok(a), Ok(b)) if a <= b && b <= length => (a, b),
        _ => fail(&format!(
            "slice {start}..{end} out of range for length {length}"
        )),
    }
}"#,
    },
    Piece {
        name: "slice",
        deps: &["bounds"],
        code: r#"/// `items[start..end]`: a new list of copies.
pub fn slice<T: Clone>(items: &[T], start: i64, end: i64) -> Vec<T> {
    let (a, b) = bounds(start, end, items.len());
    items[a..b].to_vec()
}

/// `items[start..]`
pub fn slice_from<T: Clone>(items: &[T], start: i64) -> Vec<T> {
    slice(items, start, items.len() as i64)
}"#,
    },
    Piece {
        name: "slice_str",
        deps: &["bounds", "str_len"],
        code: r#"/// `text[start..end]`, counting Unicode scalar values.
pub fn slice_str(text: &str, start: i64, end: i64) -> String {
    let chars: Vec<char> = text.chars().collect();
    let (a, b) = bounds(start, end, chars.len());
    chars[a..b].iter().collect()
}

/// `text[start..]`
pub fn slice_str_from(text: &str, start: i64) -> String {
    slice_str(text, start, str_len(text))
}"#,
    },
    Piece {
        name: "str_len",
        deps: &[],
        code: r#"/// `String.len`: the length in Unicode scalar values.
pub fn str_len(text: &str) -> i64 {
    text.chars().count() as i64
}"#,
    },
    Piece {
        name: "float_to_string",
        deps: &[],
        code: r#"/// `Float.to_string`: the canonical format of spec chapter 7, section 4.1, which is the format
/// of JavaScript with `inf` and a `.0` for whole numbers.
pub fn float_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0" } else { "0.0" }.to_string();
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let e = format!("{:e}", x.abs());
    let (mantissa, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let digits = mantissa.replace('.', "");
    let n = exp.parse::<i64>().unwrap_or(0) + 1;
    let k = digits.len() as i64;
    let text = if k <= n && n <= 21 {
        format!("{digits}{}.0", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        let (a, b) = digits.split_at(n as usize);
        format!("{a}.{b}")
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let (a, b) = digits.split_at(1);
        if b.is_empty() {
            format!("{a}e{}", n - 1)
        } else {
            format!("{a}.{b}e{}", n - 1)
        }
    };
    format!("{sign}{text}")
}"#,
    },
    Piece {
        name: "float_to_int",
        deps: &["Panic"],
        code: r#"/// `Float.to_int`.
pub fn float_to_int(x: f64) -> i64 {
    if !x.is_finite() {
        fail("float to int conversion of a non-finite value");
    }
    let t = x.trunc();
    if !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&t) {
        fail("integer overflow");
    }
    t as i64
}"#,
    },
    Piece {
        name: "strings",
        deps: &["Panic", "str_len"],
        code: r#"/// `String.find`: the position of the first occurrence, in scalar values.
pub fn str_find(text: &str, part: &str) -> Option<i64> {
    text.find(part).map(|i| str_len(&text[..i]))
}

/// `String.split`
pub fn split(text: &str, separator: &str) -> Vec<String> {
    if separator.is_empty() {
        fail("empty separator");
    }
    text.split(separator).map(String::from).collect()
}

/// `String.lines`
pub fn lines(text: &str) -> Vec<String> {
    text.lines().map(String::from).collect()
}

/// `String.chars`
pub fn chars(text: &str) -> Vec<String> {
    text.chars().map(String::from).collect()
}

/// `String.trim`: ASCII whitespace at both ends is removed.
pub fn trim(text: &str) -> String {
    text.trim_matches([' ', '\t', '\n', '\r', '\x0c', '\x0b'])
        .to_string()
}

/// `String.replace`
pub fn replace(text: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        fail("empty pattern");
    }
    text.replace(from, to)
}

/// `String.repeat`
pub fn repeat(text: &str, times: i64) -> String {
    if times < 0 {
        fail("negative repeat count");
    }
    text.repeat(times as usize)
}"#,
    },
    Piece {
        name: "Error",
        deps: &[],
        code: r#"/// An error that can be shown to a person.
pub trait Error {
    /// The text of the error.
    fn message(&self) -> String;
}

impl<E: Error + 'static> From<E> for Box<dyn Error> {
    fn from(error: E) -> Self {
        Box::new(error)
    }
}"#,
    },
    Piece {
        name: "extern_int",
        deps: &["Panic"],
        code: r#"/// An `Int` passed to extern code as the integer type it takes.
pub fn to_target_int<T: TryFrom<i64>>(value: i64) -> T {
    match T::try_from(value) {
        Ok(v) => v,
        Err(_) => fail("integer too large for extern code"),
    }
}

/// An integer returned by extern code as an `Int`.
pub fn from_target_int<T: TryInto<i64>>(value: T) -> i64 {
    match value.try_into() {
        Ok(v) => v,
        Err(_) => fail("integer overflow"),
    }
}"#,
    },
    Piece {
        name: "ExternError",
        deps: &["Error"],
        code: r#"/// An error of extern code, with the text the target gives it.
pub struct ExternError(pub String);

impl Error for ExternError {
    fn message(&self) -> String {
        self.0.clone()
    }
}

/// An error of extern code as an `Error`.
pub fn extern_error<E: std::fmt::Display>(error: E) -> Box<dyn Error> {
    Box::new(ExternError(error.to_string()))
}"#,
    },
    Piece {
        name: "ParseError",
        deps: &["Error"],
        code: r#"/// The error of `parse_int` and `parse_float`.
#[derive(Clone, PartialEq)]
pub struct ParseError {
    input: String,
}

impl Error for ParseError {
    fn message(&self) -> String {
        format!("cannot parse {}", self.input)
    }
}"#,
    },
    Piece {
        name: "parse",
        deps: &["ParseError"],
        code: r#"fn digits(text: &str, underscores: bool) -> Option<&str> {
    let bytes = text.as_bytes();
    if !bytes.first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            i += 1;
        } else if underscores
            && bytes[i] == b'_'
            && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
        {
            i += 2;
        } else {
            break;
        }
    }
    Some(&text[i..])
}

fn parse_error(text: &str) -> ParseError {
    ParseError {
        input: text.to_string(),
    }
}

/// `String.parse_int`: an optional `-` and decimal digits.
pub fn parse_int(text: &str) -> Result<i64, ParseError> {
    match digits(text.strip_prefix('-').unwrap_or(text), false) {
        Some("") => text.parse::<i64>().map_err(|_| parse_error(text)),
        _ => Err(parse_error(text)),
    }
}

/// `String.parse_float`: the float literal syntax with an optional `-`, `NaN` or `inf`.
pub fn parse_float(text: &str) -> Result<f64, ParseError> {
    match text {
        "NaN" => return Ok(f64::NAN),
        "inf" => return Ok(f64::INFINITY),
        "-inf" => return Ok(f64::NEG_INFINITY),
        _ => {}
    }
    let mut rest = digits(text.strip_prefix('-').unwrap_or(text), true);
    if let Some(r) = rest.and_then(|r| r.strip_prefix('.')) {
        rest = digits(r, true);
    }
    if let Some(r) = rest.and_then(|r| r.strip_prefix(['e', 'E'])) {
        rest = digits(r.strip_prefix(['+', '-']).unwrap_or(r), true);
    }
    match rest {
        Some("") => text
            .replace('_', "")
            .parse::<f64>()
            .map_err(|_| parse_error(text)),
        _ => Err(parse_error(text)),
    }
}"#,
    },
    Piece {
        name: "Ordering",
        deps: &[],
        code: r#"/// The result of `compare`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ordering {
    Less,
    Equal,
    Greater,
}

fn order(o: Ordering) -> std::cmp::Ordering {
    match o {
        Ordering::Less => std::cmp::Ordering::Less,
        Ordering::Equal => std::cmp::Ordering::Equal,
        Ordering::Greater => std::cmp::Ordering::Greater,
    }
}

fn ordering(o: std::cmp::Ordering) -> Ordering {
    match o {
        std::cmp::Ordering::Less => Ordering::Less,
        std::cmp::Ordering::Equal => Ordering::Equal,
        std::cmp::Ordering::Greater => Ordering::Greater,
    }
}"#,
    },
    Piece {
        name: "Comparable",
        deps: &["Ordering"],
        code: r#"/// A total order.
pub trait Comparable {
    /// Compares `self` with `other`.
    fn compare(&self, other: &Self) -> Ordering;
}

impl Comparable for i64 {
    fn compare(&self, other: &Self) -> Ordering {
        ordering(self.cmp(other))
    }
}

impl Comparable for String {
    fn compare(&self, other: &Self) -> Ordering {
        ordering(self.cmp(other))
    }
}"#,
    },
    Piece {
        name: "sort",
        deps: &["Comparable"],
        code: r#"/// `std.list.sort`: a stable sort.
pub fn sort<T: Comparable>(items: &mut [T]) {
    items.sort_by(|a, b| order(a.compare(b)));
}

/// `std.list.sort_by`: a stable sort with a comparison function.
pub fn sort_by<T>(items: &mut [T], compare: &dyn Fn(&T, &T) -> Ordering) {
    items.sort_by(|a, b| order(compare(a, b)));
}

/// `std.list.min`
pub fn min<T: Comparable + Clone>(items: &[T]) -> Option<T> {
    best(items, Ordering::Less)
}

/// `std.list.max`
pub fn max<T: Comparable + Clone>(items: &[T]) -> Option<T> {
    best(items, Ordering::Greater)
}

fn best<T: Comparable + Clone>(items: &[T], want: Ordering) -> Option<T> {
    let mut best = items.first()?;
    for item in items {
        if item.compare(best) == want {
            best = item;
        }
    }
    Some(best.clone())
}"#,
    },
    Piece {
        name: "list",
        deps: &["At", "Panic"],
        code: r#"/// `List.insert`
pub fn insert<T>(items: &mut Vec<T>, i: i64, item: T) {
    match usize::try_from(i) {
        Ok(k) if k <= items.len() => items.insert(k, item),
        _ => fail(&format!(
            "index {i} out of range for length {}",
            items.len()
        )),
    }
}

/// `List.remove`
pub fn remove<T>(items: &mut Vec<T>, i: i64) -> T {
    let k = index(i, items.len());
    items.remove(k)
}

/// `List.swap`
pub fn swap<T>(items: &mut [T], i: i64, j: i64) {
    let a = index(i, items.len());
    let b = index(j, items.len());
    items.swap(a, b);
}

/// `std.list.index_of`
pub fn index_of<T: PartialEq>(items: &[T], item: &T) -> Option<i64> {
    items.iter().position(|x| x == item).map(|i| i as i64)
}

/// `std.list.get`
pub fn get<T: Clone>(items: &[T], i: i64) -> Option<T> {
    usize::try_from(i).ok().and_then(|k| items.get(k)).cloned()
}"#,
    },
    Piece {
        name: "Entry",
        deps: &[],
        code: r#"/// One key and its value.
#[derive(Clone, PartialEq)]
pub struct Entry<K, V> {
    pub key: K,
    pub value: V,
}"#,
    },
    Piece {
        name: "Map",
        deps: &["Entry", "Panic"],
        code: r#"/// A map that keeps the order in which keys were first inserted (ADR 0003).
#[derive(Clone)]
pub struct Map<K, V> {
    keys: Vec<K>,
    values: Vec<V>,
    index: std::collections::HashMap<K, usize>,
}

impl<K: std::hash::Hash + Eq + Clone, V> Default for Map<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: std::hash::Hash + Eq + Clone, V> Map<K, V> {
    /// An empty map.
    pub fn new() -> Self {
        Map {
            keys: Vec::new(),
            values: Vec::new(),
            index: std::collections::HashMap::new(),
        }
    }

    /// `Map.len`
    pub fn len(&self) -> i64 {
        self.keys.len() as i64
    }

    /// `Map.is_empty`
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// `Map.contains_key`
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.index.contains_key(key)
    }

    /// `Map.insert`: it returns the replaced value.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if let Some(&i) = self.index.get(&key) {
            return Some(std::mem::replace(&mut self.values[i], value));
        }
        self.index.insert(key.clone(), self.keys.len());
        self.keys.push(key);
        self.values.push(value);
        None
    }

    /// `Map.remove`
    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        let i = self.index.remove(key)?;
        self.keys.remove(i);
        let value = self.values.remove(i);
        for (j, k) in self.keys.iter().enumerate().skip(i) {
            self.index.insert(k.clone(), j);
        }
        Some(value)
    }

    /// `Map.keys`
    pub fn keys(&self) -> Vec<K> {
        self.keys.clone()
    }

    /// `Map.entries`
    pub fn entries(self) -> Vec<Entry<K, V>> {
        self.keys
            .into_iter()
            .zip(self.values)
            .map(|(key, value)| Entry { key, value })
            .collect()
    }

    /// `Map.clear`
    pub fn clear(&mut self) {
        self.keys.clear();
        self.values.clear();
        self.index.clear();
    }

    fn position<Q>(&self, key: &Q) -> usize
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        match self.index.get(key) {
            Some(&i) => i,
            None => fail("key not found in map"),
        }
    }

    /// `map[key]`
    pub fn at<Q>(&self, key: &Q) -> &V
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        &self.values[self.position(key)]
    }

    /// `map[key]` as a place.
    pub fn at_mut<Q>(&mut self, key: &Q) -> &mut V
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        let i = self.position(key);
        &mut self.values[i]
    }
}

impl<K: std::hash::Hash + Eq + Clone, V: Clone> Map<K, V> {
    /// `std.map.get`
    pub fn get<Q>(&self, key: &Q) -> Option<V>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.index.get(key).map(|&i| self.values[i].clone())
    }

    /// `std.map.values`
    pub fn values(&self) -> Vec<V> {
        self.values.clone()
    }
}

impl<K: std::hash::Hash + Eq + Clone, V, const N: usize> From<[(K, V); N]> for Map<K, V> {
    fn from(entries: [(K, V); N]) -> Self {
        let mut map = Map::new();
        for (k, v) in entries {
            map.insert(k, v);
        }
        map
    }
}

impl<K: std::hash::Hash + Eq, V: PartialEq> PartialEq for Map<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.keys.len() == other.keys.len()
            && self
                .keys
                .iter()
                .zip(&self.values)
                .all(|(k, v)| other.index.get(k).is_some_and(|&j| other.values[j] == *v))
    }
}"#,
    },
    Piece {
        name: "FsError",
        deps: &["Error"],
        code: r#"/// The error of `std.fs`.
#[derive(Clone, PartialEq)]
pub enum FsError {
    /// The path.
    NotFound(String),
    PermissionDenied(String),
    /// A message from the target.
    Other(String),
}

impl Error for FsError {
    fn message(&self) -> String {
        match self {
            FsError::NotFound(path) => format!("file not found: {path}"),
            FsError::PermissionDenied(path) => format!("permission denied: {path}"),
            FsError::Other(text) => text.clone(),
        }
    }
}"#,
    },
    Piece {
        name: "fs",
        deps: &["FsError"],
        code: r#"fn fs_error(error: std::io::Error, path: &str) -> FsError {
    match error.kind() {
        std::io::ErrorKind::NotFound => FsError::NotFound(path.to_string()),
        std::io::ErrorKind::PermissionDenied => FsError::PermissionDenied(path.to_string()),
        _ => FsError::Other(error.to_string()),
    }
}

/// `std.fs.read_text`
pub fn read_text(path: &str) -> Result<String, FsError> {
    std::fs::read_to_string(path).map_err(|e| fs_error(e, path))
}

/// `std.fs.write_text`
pub fn write_text(path: &str, content: &str) -> Result<(), FsError> {
    std::fs::write(path, content).map_err(|e| fs_error(e, path))
}

/// `std.fs.exists`
pub fn exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}"#,
    },
    Piece {
        name: "io",
        deps: &[],
        code: r#"/// `std.io.read_line`: a line without its terminator; `None` at the end of input.
pub fn read_line() -> Option<String> {
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => {
            let line = line.strip_suffix('\n').unwrap_or(&line);
            Some(line.strip_suffix('\r').unwrap_or(line).to_string())
        }
    }
}

/// `std.io.args`
pub fn args() -> Vec<String> {
    std::env::args().skip(1).collect()
}"#,
    },
];

/// The runtime module with the given pieces and their dependencies, in a fixed order.
pub fn file(used: &BTreeSet<&'static str>, version: &str) -> String {
    let mut needed: BTreeSet<&'static str> = BTreeSet::new();
    let mut stack: Vec<&'static str> = used.iter().copied().collect();
    while let Some(name) = stack.pop() {
        if !needed.insert(name) {
            continue;
        }
        if let Some(piece) = PIECES.iter().find(|p| p.name == name) {
            stack.extend(piece.deps.iter().copied());
        }
    }
    let mut out = format!(
        "// Generated by wolf {version}. Do not edit.\n\n//! The runtime of Wolfpack programs in Rust, with the declarations of std.\n\n#![allow(dead_code)]\n"
    );
    for piece in PIECES.iter().filter(|p| needed.contains(p.name)) {
        out.push('\n');
        out.push_str(piece.code);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_dependency_exists() {
        for p in super::PIECES {
            for d in p.deps {
                assert!(
                    super::PIECES.iter().any(|q| q.name == *d),
                    "{} needs {d}",
                    p.name
                );
            }
        }
    }
}
