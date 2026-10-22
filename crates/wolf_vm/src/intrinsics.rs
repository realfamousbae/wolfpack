//! The intrinsic functions of std, written in Rust (spec chapter 7). Their behaviour and panic
//! messages are those of the runtime of the Rust target.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

use crate::text;
use crate::value::{MapObj, Value};
use crate::vm::{Vm, boolean, float, index, int, list, map, string};
use crate::{FsError, Stop};

macro_rules! intrinsics {
    ($($variant:ident = $name:literal,)*) => {
        /// An intrinsic function of std.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Intrinsic {
            $(
                #[doc = $name]
                $variant,
            )*
        }

        impl Intrinsic {
            /// The intrinsic with this qualified name, such as `std.list.List.push`.
            pub fn from_name(name: &str) -> Option<Intrinsic> {
                match name {
                    $($name => Some(Intrinsic::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

intrinsics! {
    Print = "std.io.print",
    Write = "std.io.write",
    Eprint = "std.io.eprint",
    ReadLine = "std.io.read_line",
    Args = "std.io.args",
    Exit = "std.process.exit",
    Env = "std.process.env",
    Error = "std.prelude.error",
    Replace = "std.prelude.replace",
    BoolToString = "std.prelude.Bool.to_string",
    IntToString = "std.math.Int.to_string",
    IntToFloat = "std.math.Int.to_float",
    IntAbs = "std.math.Int.abs",
    IntMin = "std.math.Int.min",
    IntMax = "std.math.Int.max",
    IntPow = "std.math.Int.pow",
    IntCompare = "std.math.Int.compare",
    FloatToString = "std.math.Float.to_string",
    FloatToInt = "std.math.Float.to_int",
    FloatAbs = "std.math.Float.abs",
    FloatMin = "std.math.Float.min",
    FloatMax = "std.math.Float.max",
    FloatPow = "std.math.Float.pow",
    FloatSqrt = "std.math.Float.sqrt",
    FloatFloor = "std.math.Float.floor",
    FloatCeil = "std.math.Float.ceil",
    FloatRound = "std.math.Float.round",
    FloatIsNan = "std.math.Float.is_nan",
    StrLen = "std.string.String.len",
    StrIsEmpty = "std.string.String.is_empty",
    StrContains = "std.string.String.contains",
    StrStartsWith = "std.string.String.starts_with",
    StrEndsWith = "std.string.String.ends_with",
    StrFind = "std.string.String.find",
    StrSplit = "std.string.String.split",
    StrLines = "std.string.String.lines",
    StrChars = "std.string.String.chars",
    StrTrim = "std.string.String.trim",
    StrReplace = "std.string.String.replace",
    StrRepeat = "std.string.String.repeat",
    StrUpper = "std.string.String.to_upper_ascii",
    StrLower = "std.string.String.to_lower_ascii",
    StrParseInt = "std.string.String.parse_int",
    StrParseFloat = "std.string.String.parse_float",
    StrCompare = "std.string.String.compare",
    StrToString = "std.string.String.to_string",
    Join = "std.string.join",
    ListNew = "std.list.List.new",
    ListLen = "std.list.List.len",
    ListIsEmpty = "std.list.List.is_empty",
    ListPush = "std.list.List.push",
    ListPop = "std.list.List.pop",
    ListInsert = "std.list.List.insert",
    ListRemove = "std.list.List.remove",
    ListSwap = "std.list.List.swap",
    ListReverse = "std.list.List.reverse",
    ListClear = "std.list.List.clear",
    ListAppend = "std.list.List.append",
    ListMap = "std.list.List.map",
    ListFilter = "std.list.List.filter",
    ListAny = "std.list.List.any",
    ListAll = "std.list.List.all",
    ListCount = "std.list.List.count",
    Contains = "std.list.contains",
    IndexOf = "std.list.index_of",
    Get = "std.list.get",
    Sort = "std.list.sort",
    SortBy = "std.list.sort_by",
    Min = "std.list.min",
    Max = "std.list.max",
    Sum = "std.list.sum",
    Range = "std.list.range",
    MapNew = "std.map.Map.new",
    MapLen = "std.map.Map.len",
    MapIsEmpty = "std.map.Map.is_empty",
    MapContainsKey = "std.map.Map.contains_key",
    MapInsert = "std.map.Map.insert",
    MapRemove = "std.map.Map.remove",
    MapKeys = "std.map.Map.keys",
    MapEntries = "std.map.Map.entries",
    MapClear = "std.map.Map.clear",
    MapGet = "std.map.get",
    MapValues = "std.map.values",
    ReadText = "std.fs.read_text",
    WriteText = "std.fs.write_text",
    Exists = "std.fs.exists",
}

fn internal(what: &str) -> Stop {
    Stop::Internal(what.to_string())
}

fn count(n: usize) -> Value {
    Value::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

fn strings(items: impl IntoIterator<Item = String>) -> Value {
    Value::list(items.into_iter().map(|s| Value::str(&s)).collect())
}

/// A stable merge sort whose comparison can fail (a panic of wolfpack code). Rust's own sort
/// may panic on a comparison that is not a total order, which a program can give.
fn merge_sort(
    items: Vec<Value>,
    cmp: &mut dyn FnMut(&Value, &Value) -> Result<Ordering, Stop>,
) -> Result<Vec<Value>, Stop> {
    if items.len() <= 1 {
        return Ok(items);
    }
    let mut items = items;
    let right = items.split_off(items.len() / 2);
    let left = merge_sort(items, cmp)?;
    let right = merge_sort(right, cmp)?;
    let mut out = Vec::with_capacity(left.len() + right.len());
    let mut l = left.into_iter().peekable();
    let mut r = right.into_iter().peekable();
    loop {
        let take_left = match (l.peek(), r.peek()) {
            (Some(a), Some(b)) => cmp(a, b)? != Ordering::Greater,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        let next = if take_left { l.next() } else { r.next() };
        if let Some(v) = next {
            out.push(v);
        }
    }
    Ok(out)
}

impl Vm<'_> {
    fn some(&self, v: Value) -> Value {
        Value::obj(self.image().tags.some, vec![v])
    }

    fn none(&self) -> Value {
        Value::obj(self.image().tags.none, Vec::new())
    }

    fn option(&self, v: Option<Value>) -> Value {
        match v {
            Some(v) => self.some(v),
            None => self.none(),
        }
    }

    fn ok(&self, v: Value) -> Value {
        Value::obj(self.image().tags.ok, vec![v])
    }

    fn err(&self, v: Value) -> Value {
        Value::obj(self.image().tags.err, vec![v])
    }

    fn ordering(&self, o: Ordering) -> Value {
        let t = &self.image().tags;
        let tag = match o {
            Ordering::Less => t.less,
            Ordering::Equal => t.equal,
            Ordering::Greater => t.greater,
        };
        Value::obj(tag, Vec::new())
    }

    fn order_of(&self, v: &Value) -> Result<Ordering, Stop> {
        let t = &self.image().tags;
        match v {
            Value::Obj(o) if o.tag == t.less => Ok(Ordering::Less),
            Value::Obj(o) if o.tag == t.equal => Ok(Ordering::Equal),
            Value::Obj(o) if o.tag == t.greater => Ok(Ordering::Greater),
            _ => Err(internal("an `Ordering` was expected")),
        }
    }

    /// `a.compare(b)` of `Comparable`.
    fn compare(&mut self, a: &Value, b: &Value) -> Result<Ordering, Stop> {
        match (a, b) {
            (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
            (Value::Str(x), Value::Str(y)) => Ok(x.cmp(y)),
            _ => {
                let o = self.call_method("compare", a.clone(), vec![b.clone()])?;
                self.order_of(&o)
            }
        }
    }

    fn fs_error(&self, e: FsError, path: &str) -> Value {
        let t = &self.image().tags;
        match e {
            FsError::NotFound => Value::obj(t.not_found, vec![Value::str(path)]),
            FsError::PermissionDenied => Value::obj(t.permission_denied, vec![Value::str(path)]),
            FsError::Other(text) => Value::obj(t.other, vec![Value::str(&text)]),
        }
    }

    fn parse_error(&self, input: &str) -> Value {
        Value::obj(self.image().tags.parse_error, vec![Value::str(input)])
    }

    /// The elements of a list, as they are now: calls back into wolfpack code must not hold a
    /// borrow of the list.
    fn snapshot(&self, v: &Value) -> Result<Vec<Value>, Stop> {
        Ok(list(v)?.borrow().clone())
    }

    /// The function `f` called on one value, as a `Bool`.
    fn test_with(&mut self, f: &Value, x: Value) -> Result<bool, Stop> {
        let r = self.call_value(f, vec![x])?;
        boolean(&r)
    }
}

impl Intrinsic {
    /// Calls the intrinsic. `mut` arguments come as references.
    pub(crate) fn call(self, vm: &mut Vm<'_>, raw: Vec<Value>) -> Result<Value, Stop> {
        use Intrinsic as I;
        // `replace` needs its place; every other intrinsic works on values.
        if self == I::Replace {
            let mut it = raw.into_iter();
            let (Some(Value::Ref(place)), Some(value)) = (it.next(), it.next()) else {
                return Err(internal("`replace` without a place"));
            };
            let value = vm.deref(&value)?;
            let old = vm.read(&place)?;
            vm.write(&place, value)?;
            return Ok(old);
        }
        let args: Vec<Value> = raw.iter().map(|v| vm.deref(v)).collect::<Result<_, _>>()?;
        let arg = |i: usize| -> Result<&Value, Stop> {
            args.get(i)
                .ok_or_else(|| internal("an argument is missing"))
        };
        Ok(match self {
            I::Print => {
                let s = string(arg(0)?)?;
                vm.io().out(&format!("{s}\n"));
                Value::Unit
            }
            I::Write => {
                let s = string(arg(0)?)?;
                vm.io().out(&s);
                Value::Unit
            }
            I::Eprint => {
                let s = string(arg(0)?)?;
                vm.io().err(&format!("{s}\n"));
                Value::Unit
            }
            I::ReadLine => {
                let line = vm.io().read_line();
                vm.option(line.map(|l| Value::str(&l)))
            }
            I::Args => strings(vm.io().args()),
            I::Exit => {
                let code = int(arg(0)?)?;
                let code = i32::try_from(code)
                    .ok()
                    .filter(|c| (0..=255).contains(c))
                    .ok_or_else(|| Stop::fail(format!("exit code {code} out of range")))?;
                vm.io().flush();
                return Err(Stop::Exit(code));
            }
            I::Env => {
                let name = string(arg(0)?)?;
                let v = vm.io().env(&name);
                vm.option(v.map(|v| Value::str(&v)))
            }
            I::Error => Value::Error(string(arg(0)?)?),
            I::Replace => return Err(internal("`replace` handled above")),
            I::BoolToString => Value::str(if boolean(arg(0)?)? { "true" } else { "false" }),
            I::IntToString => Value::str(&int(arg(0)?)?.to_string()),
            // `Int` to `Float` rounds to the nearest float, as on every target.
            #[allow(clippy::cast_precision_loss)]
            I::IntToFloat => Value::Float(int(arg(0)?)? as f64),
            I::IntAbs => Value::Int(
                int(arg(0)?)?
                    .checked_abs()
                    .ok_or_else(|| Stop::fail("integer overflow"))?,
            ),
            I::IntMin => Value::Int(int(arg(0)?)?.min(int(arg(1)?)?)),
            I::IntMax => Value::Int(int(arg(0)?)?.max(int(arg(1)?)?)),
            I::IntPow => {
                let (a, e) = (int(arg(0)?)?, int(arg(1)?)?);
                if e < 0 {
                    return Err(Stop::fail("negative exponent"));
                }
                let r = match u32::try_from(e) {
                    Ok(e) => a.checked_pow(e),
                    // A huge exponent overflows unless the base is 0, 1 or -1.
                    Err(_) => match a {
                        0 | 1 => Some(a),
                        -1 => Some(if e % 2 == 0 { 1 } else { -1 }),
                        _ => None,
                    },
                };
                Value::Int(r.ok_or_else(|| Stop::fail("integer overflow"))?)
            }
            I::IntCompare | I::StrCompare => {
                let (a, b) = (arg(0)?.clone(), arg(1)?.clone());
                let o = vm.compare(&a, &b)?;
                vm.ordering(o)
            }
            I::FloatToString => Value::str(&text::float_to_string(float(arg(0)?)?)),
            I::FloatToInt => Value::Int(text::float_to_int(float(arg(0)?)?).map_err(Stop::fail)?),
            I::FloatAbs => Value::Float(float(arg(0)?)?.abs()),
            I::FloatMin => Value::Float(float(arg(0)?)?.min(float(arg(1)?)?)),
            I::FloatMax => Value::Float(float(arg(0)?)?.max(float(arg(1)?)?)),
            I::FloatPow => Value::Float(float(arg(0)?)?.powf(float(arg(1)?)?)),
            I::FloatSqrt => Value::Float(float(arg(0)?)?.sqrt()),
            I::FloatFloor => Value::Float(float(arg(0)?)?.floor()),
            I::FloatCeil => Value::Float(float(arg(0)?)?.ceil()),
            I::FloatRound => Value::Float(float(arg(0)?)?.round()),
            I::FloatIsNan => Value::Bool(float(arg(0)?)?.is_nan()),
            I::StrLen => Value::Int(text::len(&string(arg(0)?)?)),
            I::StrIsEmpty => Value::Bool(string(arg(0)?)?.is_empty()),
            I::StrContains => Value::Bool(string(arg(0)?)?.contains(&*string(arg(1)?)?)),
            I::StrStartsWith => Value::Bool(string(arg(0)?)?.starts_with(&*string(arg(1)?)?)),
            I::StrEndsWith => Value::Bool(string(arg(0)?)?.ends_with(&*string(arg(1)?)?)),
            I::StrFind => {
                let found = text::find(&string(arg(0)?)?, &string(arg(1)?)?);
                vm.option(found.map(Value::Int))
            }
            I::StrSplit => {
                let (s, sep) = (string(arg(0)?)?, string(arg(1)?)?);
                if sep.is_empty() {
                    return Err(Stop::fail("empty separator"));
                }
                strings(s.split(&*sep).map(String::from))
            }
            I::StrLines => strings(string(arg(0)?)?.lines().map(String::from)),
            I::StrChars => strings(string(arg(0)?)?.chars().map(String::from)),
            I::StrTrim => Value::str(text::trim(&string(arg(0)?)?)),
            I::StrReplace => {
                let (s, from, to) = (string(arg(0)?)?, string(arg(1)?)?, string(arg(2)?)?);
                if from.is_empty() {
                    return Err(Stop::fail("empty pattern"));
                }
                Value::str(&s.replace(&*from, &to))
            }
            I::StrRepeat => {
                let (s, n) = (string(arg(0)?)?, int(arg(1)?)?);
                let n = usize::try_from(n).map_err(|_| Stop::fail("negative repeat count"))?;
                Value::str(&s.repeat(n))
            }
            I::StrUpper => Value::str(&string(arg(0)?)?.to_ascii_uppercase()),
            I::StrLower => Value::str(&string(arg(0)?)?.to_ascii_lowercase()),
            I::StrParseInt => {
                let s = string(arg(0)?)?;
                match text::parse_int(&s) {
                    Some(i) => vm.ok(Value::Int(i)),
                    None => vm.err(vm.parse_error(&s)),
                }
            }
            I::StrParseFloat => {
                let s = string(arg(0)?)?;
                match text::parse_float(&s) {
                    Some(f) => vm.ok(Value::Float(f)),
                    None => vm.err(vm.parse_error(&s)),
                }
            }
            I::StrToString => arg(0)?.clone(),
            I::Join => {
                let parts = vm.snapshot(arg(0)?)?;
                let sep = string(arg(1)?)?;
                let parts: Vec<Rc<str>> = parts.iter().map(string).collect::<Result<_, _>>()?;
                let parts: Vec<&str> = parts.iter().map(|s| &**s).collect();
                Value::str(&parts.join(&sep))
            }
            I::ListNew => Value::list(Vec::new()),
            I::ListLen => count(list(arg(0)?)?.borrow().len()),
            I::ListIsEmpty => Value::Bool(list(arg(0)?)?.borrow().is_empty()),
            I::ListPush => {
                list(arg(0)?)?.borrow_mut().push(arg(1)?.clone());
                Value::Unit
            }
            I::ListPop => {
                let v = list(arg(0)?)?.borrow_mut().pop();
                vm.option(v)
            }
            I::ListInsert => {
                let l = list(arg(0)?)?;
                let i = int(arg(1)?)?;
                let mut items = l.borrow_mut();
                match usize::try_from(i) {
                    Ok(k) if k <= items.len() => items.insert(k, arg(2)?.clone()),
                    _ => {
                        return Err(Stop::fail(format!(
                            "index {i} out of range for length {}",
                            items.len()
                        )));
                    }
                }
                Value::Unit
            }
            I::ListRemove => {
                let l = list(arg(0)?)?;
                let mut items = l.borrow_mut();
                let k = index(int(arg(1)?)?, items.len())?;
                items.remove(k)
            }
            I::ListSwap => {
                let l = list(arg(0)?)?;
                let mut items = l.borrow_mut();
                let a = index(int(arg(1)?)?, items.len())?;
                let b = index(int(arg(2)?)?, items.len())?;
                items.swap(a, b);
                Value::Unit
            }
            I::ListReverse => {
                list(arg(0)?)?.borrow_mut().reverse();
                Value::Unit
            }
            I::ListClear => {
                list(arg(0)?)?.borrow_mut().clear();
                Value::Unit
            }
            I::ListAppend => {
                let other = vm.snapshot(arg(1)?)?;
                list(arg(0)?)?.borrow_mut().extend(other);
                Value::Unit
            }
            I::ListMap => {
                let items = vm.snapshot(arg(0)?)?;
                let f = arg(1)?.clone();
                let mut out = Vec::with_capacity(items.len());
                for x in items {
                    out.push(vm.call_value(&f, vec![x])?);
                }
                Value::list(out)
            }
            I::ListFilter => {
                let items = vm.snapshot(arg(0)?)?;
                let f = arg(1)?.clone();
                let mut out = Vec::new();
                for x in items {
                    if vm.test_with(&f, x.clone())? {
                        out.push(x);
                    }
                }
                Value::list(out)
            }
            I::ListAny | I::ListAll | I::ListCount => {
                let items = vm.snapshot(arg(0)?)?;
                let f = arg(1)?.clone();
                let mut n = 0usize;
                for x in items {
                    let yes = vm.test_with(&f, x)?;
                    match self {
                        I::ListAny if yes => return Ok(Value::Bool(true)),
                        I::ListAll if !yes => return Ok(Value::Bool(false)),
                        _ if yes => n += 1,
                        _ => {}
                    }
                }
                match self {
                    I::ListAny => Value::Bool(false),
                    I::ListAll => Value::Bool(true),
                    _ => count(n),
                }
            }
            I::Contains => {
                let x = arg(1)?;
                Value::Bool(list(arg(0)?)?.borrow().iter().any(|y| y.equals(x)))
            }
            I::IndexOf => {
                let x = arg(1)?;
                let at = list(arg(0)?)?.borrow().iter().position(|y| y.equals(x));
                vm.option(at.map(count))
            }
            I::Get => {
                let i = int(arg(1)?)?;
                let v = usize::try_from(i).ok().and_then(|k| {
                    list(arg(0).ok()?)
                        .ok()?
                        .borrow()
                        .get(k)
                        .map(Value::deep_copy)
                });
                vm.option(v)
            }
            I::Sort | I::SortBy => {
                let l = list(arg(0)?)?;
                let items = std::mem::take(&mut *l.borrow_mut());
                let sorted = if self == I::Sort {
                    merge_sort(items, &mut |a, b| vm.compare(a, b))
                } else {
                    let f = arg(1)?.clone();
                    merge_sort(items, &mut |a, b| {
                        let o = vm.call_value(&f, vec![a.clone(), b.clone()])?;
                        vm.order_of(&o)
                    })
                };
                *l.borrow_mut() = sorted?;
                Value::Unit
            }
            I::Min | I::Max => {
                let items = vm.snapshot(arg(0)?)?;
                let want = if self == I::Min {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
                let mut best: Option<Value> = None;
                for item in items {
                    best = match best {
                        None => Some(item),
                        Some(b) => {
                            if vm.compare(&item, &b)? == want {
                                Some(item)
                            } else {
                                Some(b)
                            }
                        }
                    };
                }
                vm.option(best.map(|b| b.deep_copy()))
            }
            I::Sum => {
                let mut total: i64 = 0;
                for x in list(arg(0)?)?.borrow().iter() {
                    total = total
                        .checked_add(int(x)?)
                        .ok_or_else(|| Stop::fail("integer overflow"))?;
                }
                Value::Int(total)
            }
            I::Range => {
                let (a, b) = (int(arg(0)?)?, int(arg(1)?)?);
                Value::list((a..b).map(Value::Int).collect())
            }
            I::MapNew => Value::Map(Rc::new(RefCell::new(MapObj::default()))),
            I::MapLen => count(map(arg(0)?)?.borrow().keys.len()),
            I::MapIsEmpty => Value::Bool(map(arg(0)?)?.borrow().keys.is_empty()),
            I::MapContainsKey => Value::Bool(map(arg(0)?)?.borrow().position(arg(1)?).is_some()),
            I::MapInsert => {
                let m = map(arg(0)?)?;
                let old = m.borrow_mut().insert(arg(1)?.clone(), arg(2)?.clone());
                vm.option(old)
            }
            I::MapRemove => {
                let m = map(arg(0)?)?;
                let old = m.borrow_mut().remove(arg(1)?);
                vm.option(old)
            }
            I::MapKeys => Value::list(map(arg(0)?)?.borrow().keys.clone()),
            I::MapEntries => {
                let entry = vm.image().tags.entry;
                let m = map(arg(0)?)?;
                let m = m.borrow();
                Value::list(
                    m.keys
                        .iter()
                        .zip(m.values.iter())
                        .map(|(k, v)| Value::obj(entry, vec![k.clone(), v.clone()]))
                        .collect(),
                )
            }
            I::MapClear => {
                map(arg(0)?)?.borrow_mut().clear();
                Value::Unit
            }
            I::MapGet => {
                let m = map(arg(0)?)?;
                let m = m.borrow();
                let v = m
                    .position(arg(1)?)
                    .and_then(|k| m.values.get(k))
                    .map(Value::deep_copy);
                vm.option(v)
            }
            I::MapValues => Value::list(
                map(arg(0)?)?
                    .borrow()
                    .values
                    .iter()
                    .map(Value::deep_copy)
                    .collect(),
            ),
            I::ReadText => {
                let path = string(arg(0)?)?;
                match vm.io().read_text(&path) {
                    Ok(t) => vm.ok(Value::str(&t)),
                    Err(e) => vm.err(vm.fs_error(e, &path)),
                }
            }
            I::WriteText => {
                let (path, content) = (string(arg(0)?)?, string(arg(1)?)?);
                match vm.io().write_text(&path, &content) {
                    Ok(()) => vm.ok(Value::Unit),
                    Err(e) => vm.err(vm.fs_error(e, &path)),
                }
            }
            I::Exists => {
                let path = string(arg(0)?)?;
                Value::Bool(vm.io().exists(&path))
            }
        })
    }
}
