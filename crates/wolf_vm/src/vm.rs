//! The interpreter: frames, the operand stack and the instructions.

use std::cell::RefCell;
use std::rc::Rc;

use crate::Io;
use crate::Stop;
use crate::code::{BinOp, Chunk, Op, UnOp};
use crate::compile::{Func, Image};
use crate::text;
use crate::value::{Closure, MapObj, Obj, Place, Value};

/// The deepest nesting of calls; deeper recursion is `panic: stack overflow` (spec chapter 10,
/// section 2, rule 7).
const MAX_FRAMES: usize = 100_000;

/// The deepest nesting of calls from std back into wolfpack code (`map`, `sort_by`...), which
/// uses the stack of the Rust thread.
const MAX_NESTING: usize = 1_000;

struct Frame {
    func: u32,
    pc: usize,
    base: usize,
    slots: usize,
}

/// A running program.
pub(crate) struct Vm<'a> {
    image: &'a Image,
    io: &'a mut dyn Io,
    stack: Vec<Value>,
    frames: Vec<Frame>,
    nesting: usize,
}

fn internal(what: &str) -> Stop {
    Stop::Internal(what.to_string())
}

/// The `Int` of a value.
pub(crate) fn int(v: &Value) -> Result<i64, Stop> {
    match v {
        Value::Int(i) => Ok(*i),
        _ => Err(internal("an `Int` was expected")),
    }
}

/// The `Float` of a value.
pub(crate) fn float(v: &Value) -> Result<f64, Stop> {
    match v {
        Value::Float(f) => Ok(*f),
        _ => Err(internal("a `Float` was expected")),
    }
}

/// The `Bool` of a value.
pub(crate) fn boolean(v: &Value) -> Result<bool, Stop> {
    match v {
        Value::Bool(b) => Ok(*b),
        _ => Err(internal("a `Bool` was expected")),
    }
}

/// The `String` of a value.
pub(crate) fn string(v: &Value) -> Result<Rc<str>, Stop> {
    match v {
        Value::Str(s) | Value::Shown(s) => Ok(s.clone()),
        _ => Err(internal("a `String` was expected")),
    }
}

/// The `List` of a value.
pub(crate) fn list(v: &Value) -> Result<Rc<RefCell<Vec<Value>>>, Stop> {
    match v {
        Value::List(l) => Ok(l.clone()),
        _ => Err(internal("a `List` was expected")),
    }
}

/// The `Map` of a value.
pub(crate) fn map(v: &Value) -> Result<Rc<RefCell<MapObj>>, Stop> {
    match v {
        Value::Map(m) => Ok(m.clone()),
        _ => Err(internal("a `Map` was expected")),
    }
}

fn obj(v: &Value) -> Result<Rc<Obj>, Stop> {
    match v {
        Value::Obj(o) => Ok(o.clone()),
        _ => Err(internal("a struct or variant was expected")),
    }
}

/// `items[i]`: the position, or the panic of the language.
pub(crate) fn index(i: i64, length: usize) -> Result<usize, Stop> {
    match usize::try_from(i) {
        Ok(k) if k < length => Ok(k),
        _ => Err(Stop::fail(format!(
            "index {i} out of range for length {length}"
        ))),
    }
}

fn overflow() -> Stop {
    Stop::fail("integer overflow")
}

fn binary(op: BinOp, a: &Value, b: &Value) -> Result<Value, Stop> {
    Ok(match op {
        BinOp::Add => Value::Int(int(a)?.checked_add(int(b)?).ok_or_else(overflow)?),
        BinOp::Sub => Value::Int(int(a)?.checked_sub(int(b)?).ok_or_else(overflow)?),
        BinOp::Mul => Value::Int(int(a)?.checked_mul(int(b)?).ok_or_else(overflow)?),
        BinOp::Div | BinOp::Rem => {
            let (x, y) = (int(a)?, int(b)?);
            if y == 0 {
                return Err(Stop::fail("division by zero"));
            }
            let r = if op == BinOp::Div {
                x.checked_div(y)
            } else {
                x.checked_rem(y)
            };
            Value::Int(r.ok_or_else(overflow)?)
        }
        BinOp::Shl | BinOp::Shr => {
            let (x, n) = (int(a)?, int(b)?);
            let n = u32::try_from(n)
                .ok()
                .filter(|n| *n <= 63)
                .ok_or_else(|| Stop::fail("shift amount out of range"))?;
            Value::Int(if op == BinOp::Shl {
                x.wrapping_shl(n)
            } else {
                x >> n
            })
        }
        BinOp::BitAnd => Value::Int(int(a)? & int(b)?),
        BinOp::BitOr => Value::Int(int(a)? | int(b)?),
        BinOp::BitXor => Value::Int(int(a)? ^ int(b)?),
        BinOp::FAdd => Value::Float(float(a)? + float(b)?),
        BinOp::FSub => Value::Float(float(a)? - float(b)?),
        BinOp::FMul => Value::Float(float(a)? * float(b)?),
        BinOp::FDiv => Value::Float(float(a)? / float(b)?),
        BinOp::FRem => Value::Float(float(a)? % float(b)?),
        BinOp::Concat => {
            let (x, y) = (string(a)?, string(b)?);
            let mut s = String::with_capacity(x.len() + y.len());
            s.push_str(&x);
            s.push_str(&y);
            Value::str(&s)
        }
        BinOp::Eq => Value::Bool(a.equals(b)),
        BinOp::Ne => Value::Bool(!a.equals(b)),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            let order = match (a, b) {
                (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
                (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
                (Value::Str(x), Value::Str(y)) => Some(x.cmp(y)),
                _ => return Err(internal("a comparison of values without an order")),
            };
            let yes = match order {
                None => false,
                Some(o) => match op {
                    BinOp::Lt => o.is_lt(),
                    BinOp::Le => o.is_le(),
                    BinOp::Gt => o.is_gt(),
                    _ => o.is_ge(),
                },
            };
            Value::Bool(yes)
        }
    })
}

fn unary(op: UnOp, v: &Value) -> Result<Value, Stop> {
    Ok(match op {
        UnOp::Neg => Value::Int(int(v)?.checked_neg().ok_or_else(overflow)?),
        UnOp::FNeg => Value::Float(-float(v)?),
        UnOp::Not => Value::Bool(!boolean(v)?),
        UnOp::BitNot => Value::Int(!int(v)?),
    })
}

impl<'a> Vm<'a> {
    pub(crate) fn new(image: &'a Image, io: &'a mut dyn Io) -> Vm<'a> {
        Vm {
            image,
            io,
            stack: Vec::new(),
            frames: Vec::new(),
            nesting: 0,
        }
    }

    pub(crate) fn io(&mut self) -> &mut dyn Io {
        &mut *self.io
    }

    pub(crate) fn image(&self) -> &'a Image {
        self.image
    }

    /// Runs `main`; with a `Result`, an `Err` gives the message of its error.
    pub(crate) fn main(
        &mut self,
        func: u32,
        takes_args: bool,
        result: bool,
    ) -> Result<Option<String>, Stop> {
        let args = if takes_args {
            let list = self.io.args().iter().map(|a| Value::str(a)).collect();
            vec![Value::list(list)]
        } else {
            Vec::new()
        };
        let value = self.invoke(func, args)?;
        if !result {
            return Ok(None);
        }
        let o = obj(&value)?;
        if o.tag != self.image.tags.err {
            return Ok(None);
        }
        let error = o
            .fields
            .borrow()
            .first()
            .cloned()
            .ok_or_else(|| internal("an `Err` without a value"))?;
        let message = self.call_method("message", error, Vec::new())?;
        Ok(Some(string(&message)?.to_string()))
    }

    /// Runs a test.
    pub(crate) fn test(&mut self, func: u32) -> Result<(), Stop> {
        self.invoke(func, Vec::new()).map(|_| ())
    }

    // ----- calls ------------------------------------------------------------------------------

    fn chunk(&self, func: u32) -> Result<&'a Chunk, Stop> {
        match self.image.funcs.get(func as usize) {
            Some(Func::Code(c)) => Ok(c),
            _ => Err(internal("a call of a function without code")),
        }
    }

    /// Calls a function with these arguments and runs it to its end.
    pub(crate) fn invoke(&mut self, func: u32, args: Vec<Value>) -> Result<Value, Stop> {
        self.invoke_with(func, args, &[])
    }

    fn invoke_with(
        &mut self,
        func: u32,
        args: Vec<Value>,
        captures: &[Value],
    ) -> Result<Value, Stop> {
        match self.image.funcs.get(func as usize) {
            Some(Func::Code(_)) => {
                if self.nesting >= MAX_NESTING {
                    return Err(Stop::fail("stack overflow"));
                }
                let depth = self.frames.len();
                let argc = args.len();
                self.stack.extend(args);
                self.stack.extend(captures.iter().cloned());
                self.enter(func, argc + captures.len())?;
                self.nesting += 1;
                let result = self.execute(depth);
                self.nesting -= 1;
                result
            }
            Some(Func::Intrinsic(i)) => i.call(self, args),
            Some(Func::Missing(what)) => {
                Err(Stop::Internal(format!("{what} cannot run in the VM")))
            }
            None => Err(internal("a call of an unknown function")),
        }
    }

    /// Calls a function value.
    pub(crate) fn call_value(&mut self, f: &Value, args: Vec<Value>) -> Result<Value, Stop> {
        match f {
            Value::Closure(c) => {
                let c: Rc<Closure> = c.clone();
                self.invoke_with(c.func, args, &c.captures)
            }
            _ => Err(internal("a call of a value that is not a function")),
        }
    }

    /// The type of a value for the dispatch of interface methods.
    fn type_of(&self, v: &Value) -> Option<u32> {
        let b = &self.image.builtins;
        Some(match v {
            Value::Unit => b.void,
            Value::Int(_) => b.int,
            Value::Float(_) => b.float,
            Value::Bool(_) => b.bool,
            Value::Str(_) | Value::Shown(_) => b.string,
            Value::List(_) => b.list,
            Value::Map(_) => b.map,
            Value::Obj(o) => self.image.parent.get(&o.tag).copied().unwrap_or(o.tag),
            Value::Closure(_) | Value::Error(_) | Value::Ref(_) => return None,
        })
    }

    fn method(&self, recv: &Value, name: &str) -> Result<u32, Stop> {
        self.type_of(recv)
            .and_then(|t| self.image.dispatch.get(&(t, name.to_string())).copied())
            .ok_or_else(|| Stop::Internal(format!("no method `{name}` for a value")))
    }

    /// Calls a method of an interface on `recv`.
    pub(crate) fn call_method(
        &mut self,
        name: &str,
        recv: Value,
        args: Vec<Value>,
    ) -> Result<Value, Stop> {
        let target = self.deref(&recv)?;
        if let Value::Error(text) = &target {
            if name == "message" {
                return Ok(Value::Str(text.clone()));
            }
        }
        let func = self.method(&target, name)?;
        let mut all = Vec::with_capacity(args.len() + 1);
        all.push(recv);
        all.extend(args);
        self.invoke(func, all)
    }

    /// Pushes a frame for `func`, whose `argc` arguments (and captures) are on the stack.
    fn enter(&mut self, func: u32, argc: usize) -> Result<(), Stop> {
        if self.frames.len() >= MAX_FRAMES {
            return Err(Stop::fail("stack overflow"));
        }
        let chunk = self.chunk(func)?;
        let given = chunk.params as usize + chunk.captures as usize;
        if argc != given {
            return Err(Stop::Internal(format!(
                "`{}` takes {given} values and got {argc}",
                chunk.name
            )));
        }
        let base = self
            .stack
            .len()
            .checked_sub(argc)
            .ok_or_else(|| internal("too few arguments on the stack"))?;
        let slots = chunk.slots as usize;
        self.stack.resize(base + slots.max(argc), Value::Unit);
        self.frames.push(Frame {
            func,
            pc: 0,
            base,
            slots,
        });
        Ok(())
    }

    /// A call from bytecode: the arguments are on the stack.
    fn call_op(&mut self, func: u32, argc: usize, captures: &[Value]) -> Result<(), Stop> {
        match self.image.funcs.get(func as usize) {
            Some(Func::Code(_)) => {
                self.stack.extend(captures.iter().cloned());
                self.enter(func, argc + captures.len())
            }
            Some(Func::Intrinsic(i)) => {
                let at = self
                    .stack
                    .len()
                    .checked_sub(argc)
                    .ok_or_else(|| internal("too few arguments on the stack"))?;
                let args = self.stack.split_off(at);
                let result = i.call(self, args)?;
                self.stack.push(result);
                Ok(())
            }
            Some(Func::Missing(what)) => {
                Err(Stop::Internal(format!("{what} cannot run in the VM")))
            }
            None => Err(internal("a call of an unknown function")),
        }
    }

    // ----- places -----------------------------------------------------------------------------

    /// The value of a place.
    pub(crate) fn read(&self, p: &Place) -> Result<Value, Stop> {
        let v = match p {
            Place::Slot(i) => self.stack.get(*i).cloned(),
            Place::Field(o, i) => o.fields.borrow().get(*i).cloned(),
            Place::Elem(l, i) => l.borrow().get(*i).cloned(),
            Place::Entry(m, i) => m.borrow().values.get(*i).cloned(),
        };
        v.ok_or_else(|| internal("a reference to a place that is gone"))
    }

    /// Stores a value in a place.
    pub(crate) fn write(&mut self, p: &Place, v: Value) -> Result<(), Stop> {
        let ok = match p {
            Place::Slot(i) => self.stack.get_mut(*i).map(|s| *s = v).is_some(),
            Place::Field(o, i) => o.fields.borrow_mut().get_mut(*i).map(|s| *s = v).is_some(),
            Place::Elem(l, i) => l.borrow_mut().get_mut(*i).map(|s| *s = v).is_some(),
            Place::Entry(m, i) => m.borrow_mut().values.get_mut(*i).map(|s| *s = v).is_some(),
        };
        if ok {
            Ok(())
        } else {
            Err(internal("a reference to a place that is gone"))
        }
    }

    /// The value itself, or the value of the place it refers to.
    pub(crate) fn deref(&self, v: &Value) -> Result<Value, Stop> {
        match v {
            Value::Ref(p) => self.read(p),
            other => Ok(other.clone()),
        }
    }

    // ----- the loop ---------------------------------------------------------------------------

    fn pop(&mut self) -> Result<Value, Stop> {
        let v = self
            .stack
            .pop()
            .ok_or_else(|| internal("the stack is empty"))?;
        self.deref(&v)
    }

    fn pop_n(&mut self, n: u32) -> Result<Vec<Value>, Stop> {
        let at = self
            .stack
            .len()
            .checked_sub(n as usize)
            .ok_or_else(|| internal("the stack is too short"))?;
        let raw = self.stack.split_off(at);
        raw.iter().map(|v| self.deref(v)).collect()
    }

    fn push(&mut self, v: Value) {
        self.stack.push(v);
    }

    /// Runs until the frame at `depth` returns, and gives its result.
    fn execute(&mut self, depth: usize) -> Result<Value, Stop> {
        self.run(depth).map_err(|stop| self.locate(stop))
    }

    /// In a debug build, a panic of the language gets the location of the instruction that
    /// raised it (spec chapter 7, section 3, rule 1): the frame where an error happens stays on
    /// the stack, with its position just after the instruction. A panic of an inner frame has
    /// its location already, and stack overflow has none.
    fn locate(&self, stop: Stop) -> Stop {
        let located = match &stop {
            Stop::Panic(message) => message
                .strip_prefix("panic: ")
                .filter(|rest| *rest != "stack overflow")
                .and_then(|rest| {
                    let frame = self.frames.last()?;
                    let chunk = self.chunk(frame.func).ok()?;
                    match chunk.location(frame.pc.checked_sub(1)?)? {
                        Value::Str(at) => Some(format!("panic at {at}: {rest}")),
                        _ => None,
                    }
                }),
            _ => None,
        };
        match located {
            Some(message) => Stop::Panic(message),
            None => stop,
        }
    }

    /// The loop of [`Vm::execute`].
    fn run(&mut self, depth: usize) -> Result<Value, Stop> {
        loop {
            let (func, pc, base, slots) = match self.frames.last() {
                Some(f) => (f.func, f.pc, f.base, f.slots),
                None => return Err(internal("no frame to run")),
            };
            let chunk = self.chunk(func)?;
            let op = *chunk
                .code
                .get(pc)
                .ok_or_else(|| internal("the code ends without a return"))?;
            if let Some(f) = self.frames.last_mut() {
                f.pc = pc + 1;
            }
            let slot = |s: u32| base + s as usize;
            let konst = |k: u32| {
                chunk
                    .consts
                    .get(k as usize)
                    .cloned()
                    .ok_or_else(|| internal("no such constant"))
            };
            match op {
                Op::Const(k) => {
                    let v = konst(k)?;
                    self.push(v);
                }
                Op::Unit => self.push(Value::Unit),
                Op::Int(i) => self.push(Value::Int(i)),
                Op::Bool(b) => self.push(Value::Bool(b)),
                Op::Load(s) => {
                    let v = self
                        .stack
                        .get(slot(s))
                        .cloned()
                        .ok_or_else(|| internal("no such slot"))?;
                    self.push(v);
                }
                Op::LoadRef(s) => {
                    let v = self
                        .stack
                        .get(slot(s))
                        .cloned()
                        .ok_or_else(|| internal("no such slot"))?;
                    let v = self.deref(&v)?;
                    self.push(v);
                }
                Op::Store(s) => {
                    let v = self
                        .stack
                        .pop()
                        .ok_or_else(|| internal("the stack is empty"))?;
                    let place = self
                        .stack
                        .get_mut(slot(s))
                        .ok_or_else(|| internal("no such slot"))?;
                    *place = v;
                }
                Op::StoreRef(s) => {
                    let v = self.pop()?;
                    match self.stack.get(slot(s)).cloned() {
                        Some(Value::Ref(p)) => self.write(&p, v)?,
                        Some(_) => self.write(&Place::Slot(slot(s)), v)?,
                        None => return Err(internal("no such slot")),
                    }
                }
                Op::RefSlot(s) => {
                    let r = match self.stack.get(slot(s)) {
                        Some(Value::Ref(p)) => Value::Ref(p.clone()),
                        Some(_) => Value::Ref(Place::Slot(slot(s))),
                        None => return Err(internal("no such slot")),
                    };
                    self.push(r);
                }
                Op::RefField(i) => {
                    let o = obj(&self.pop()?)?;
                    if i as usize >= o.fields.borrow().len() {
                        return Err(internal("no such field"));
                    }
                    self.push(Value::Ref(Place::Field(o, i as usize)));
                }
                Op::RefIndex => {
                    let i = int(&self.pop()?)?;
                    let l = list(&self.pop()?)?;
                    let k = index(i, l.borrow().len())?;
                    self.push(Value::Ref(Place::Elem(l, k)));
                }
                Op::RefKey => {
                    let key = self.pop()?;
                    let m = map(&self.pop()?)?;
                    let k = m
                        .borrow()
                        .position(&key)
                        .ok_or_else(|| Stop::fail("key not found in map"))?;
                    self.push(Value::Ref(Place::Entry(m, k)));
                }
                Op::Pop => {
                    self.stack
                        .pop()
                        .ok_or_else(|| internal("the stack is empty"))?;
                }
                Op::PopTo(n) => self.stack.truncate(base + slots + n as usize),
                Op::Field(i) => {
                    let o = obj(&self.pop()?)?;
                    let v = o
                        .fields
                        .borrow()
                        .get(i as usize)
                        .cloned()
                        .ok_or_else(|| internal("no such field"))?;
                    self.push(v);
                }
                Op::SetField(i) => {
                    let v = self.pop()?;
                    let o = obj(&self.pop()?)?;
                    let mut fields = o.fields.borrow_mut();
                    let f = fields
                        .get_mut(i as usize)
                        .ok_or_else(|| internal("no such field"))?;
                    *f = v;
                }
                Op::Index => {
                    let i = int(&self.pop()?)?;
                    let l = list(&self.pop()?)?;
                    let v = {
                        let items = l.borrow();
                        let k = index(i, items.len())?;
                        items.get(k).cloned()
                    };
                    self.push(v.ok_or_else(|| internal("no such element"))?);
                }
                Op::IndexKey => {
                    let key = self.pop()?;
                    let m = map(&self.pop()?)?;
                    let v = {
                        let m = m.borrow();
                        m.position(&key).and_then(|k| m.values.get(k).cloned())
                    };
                    self.push(v.ok_or_else(|| Stop::fail("key not found in map"))?);
                }
                Op::SetIndex => {
                    let v = self.pop()?;
                    let i = int(&self.pop()?)?;
                    let l = list(&self.pop()?)?;
                    let mut items = l.borrow_mut();
                    let k = index(i, items.len())?;
                    if let Some(e) = items.get_mut(k) {
                        *e = v;
                    }
                }
                Op::SetKey => {
                    let v = self.pop()?;
                    let key = self.pop()?;
                    let m = map(&self.pop()?)?;
                    let mut m = m.borrow_mut();
                    let k = m
                        .position(&key)
                        .ok_or_else(|| Stop::fail("key not found in map"))?;
                    if let Some(e) = m.values.get_mut(k) {
                        *e = v;
                    }
                }
                Op::Slice {
                    from,
                    to,
                    inclusive,
                    string: is_string,
                } => {
                    let end = if to { Some(int(&self.pop()?)?) } else { None };
                    let start = if from { int(&self.pop()?)? } else { 0 };
                    let v = self.pop()?;
                    let length = match &v {
                        Value::Str(s) => text::len(s),
                        Value::List(l) => i64::try_from(l.borrow().len()).unwrap_or(i64::MAX),
                        _ => return Err(internal("a slice of a value without elements")),
                    };
                    let end = match end {
                        Some(e) if inclusive => e.checked_add(1).ok_or_else(overflow)?,
                        Some(e) => e,
                        None => length,
                    };
                    let out_of_range = || {
                        Stop::fail(format!(
                            "slice {start}..{end} out of range for length {length}"
                        ))
                    };
                    let result = if is_string {
                        let s = string(&v)?;
                        Value::str(&text::slice(&s, start, end).ok_or_else(out_of_range)?)
                    } else {
                        let l = list(&v)?;
                        let items = l.borrow();
                        let (a, b) =
                            text::bounds(start, end, items.len()).ok_or_else(out_of_range)?;
                        Value::list(
                            items
                                .get(a..b)
                                .unwrap_or(&[])
                                .iter()
                                .map(Value::deep_copy)
                                .collect(),
                        )
                    };
                    self.push(result);
                }
                Op::Make { tag, n } => {
                    let fields = self.pop_n(n)?;
                    self.push(Value::obj(tag, fields));
                }
                Op::Update { tag, fields } => {
                    let positions = list(&konst(fields)?)?;
                    let positions: Vec<i64> = positions
                        .borrow()
                        .iter()
                        .map(int)
                        .collect::<Result<_, _>>()?;
                    let n = u32::try_from(positions.len()).unwrap_or(u32::MAX);
                    let base_obj = obj(&self.pop()?)?;
                    let values = self.pop_n(n)?;
                    let mut all = base_obj.fields.borrow().clone();
                    for (p, v) in positions.iter().zip(values) {
                        let slot = usize::try_from(*p)
                            .ok()
                            .and_then(|p| all.get_mut(p))
                            .ok_or_else(|| internal("no such field"))?;
                        *slot = v;
                    }
                    self.push(Value::obj(tag, all));
                }
                Op::List(n) => {
                    let items = self.pop_n(n)?;
                    self.push(Value::list(items));
                }
                Op::Map(n) => {
                    let flat = self.pop_n(n.saturating_mul(2))?;
                    let mut m = MapObj::default();
                    let mut it = flat.into_iter();
                    while let (Some(k), Some(v)) = (it.next(), it.next()) {
                        m.insert(k, v);
                    }
                    self.push(Value::Map(Rc::new(RefCell::new(m))));
                }
                Op::Closure { func, n } => {
                    let captures = self.pop_n(n)?;
                    self.push(Value::Closure(Rc::new(Closure { func, captures })));
                }
                Op::Binary(b) => {
                    let y = self.pop()?;
                    let x = self.pop()?;
                    let v = binary(b, &x, &y)?;
                    self.push(v);
                }
                Op::Unary(u) => {
                    let x = self.pop()?;
                    let v = unary(u, &x)?;
                    self.push(v);
                }
                Op::Concat(n) => {
                    let parts = self.pop_n(n)?;
                    let mut s = String::new();
                    for p in &parts {
                        s.push_str(&string(p)?);
                    }
                    self.push(Value::str(&s));
                }
                Op::Copy => {
                    let v = self.pop()?;
                    self.push(v.deep_copy());
                }
                Op::Shown => {
                    let v = self.pop()?;
                    self.push(Value::Shown(string(&v)?));
                }
                Op::IsTag(t) => {
                    let v = self.pop()?;
                    let yes = matches!(&v, Value::Obj(o) if o.tag == t);
                    self.push(Value::Bool(yes));
                }
                Op::Jump(t) => self.jump(t),
                Op::JumpIf(t) => {
                    if boolean(&self.pop()?)? {
                        self.jump(t);
                    }
                }
                Op::JumpUnless(t) => {
                    if !boolean(&self.pop()?)? {
                        self.jump(t);
                    }
                }
                Op::Call { func, argc } => self.call_op(func, argc as usize, &[])?,
                Op::CallValue(argc) => {
                    let at = self
                        .stack
                        .len()
                        .checked_sub(argc as usize + 1)
                        .ok_or_else(|| internal("the stack is too short"))?;
                    let f = self.stack.remove(at);
                    let f = self.deref(&f)?;
                    let Value::Closure(c) = f else {
                        return Err(internal("a call of a value that is not a function"));
                    };
                    self.call_op(c.func, argc as usize, &c.captures)?;
                }
                Op::CallMethod { name, argc } => {
                    let name = string(&konst(name)?)?;
                    let at = self
                        .stack
                        .len()
                        .checked_sub(argc as usize)
                        .ok_or_else(|| internal("the stack is too short"))?;
                    let recv = self
                        .stack
                        .get(at)
                        .cloned()
                        .ok_or_else(|| internal("no receiver"))?;
                    let recv = self.deref(&recv)?;
                    if let Value::Error(text) = &recv {
                        if &*name == "message" {
                            self.stack.truncate(at);
                            self.push(Value::Str(text.clone()));
                            continue;
                        }
                    }
                    let func = self.method(&recv, &name)?;
                    self.call_op(func, argc as usize, &[])?;
                }
                Op::Return => {
                    let result = self.pop()?;
                    let frame = self.frames.pop().ok_or_else(|| internal("no frame"))?;
                    self.stack.truncate(frame.base);
                    if self.frames.len() <= depth {
                        return Ok(result);
                    }
                    self.push(result);
                }
                Op::Panic(k) => {
                    let message = string(&self.pop()?)?;
                    let location = string(&konst(k)?)?;
                    return Err(Stop::Panic(format!("panic at {location}: {message}")));
                }
                Op::Fail(k) => {
                    return Err(Stop::Panic(string(&konst(k)?)?.to_string()));
                }
                Op::Unreachable => return Err(internal("no arm of a `match` matched")),
            }
        }
    }

    fn jump(&mut self, to: u32) {
        if let Some(f) = self.frames.last_mut() {
            f.pc = to as usize;
        }
    }
}
