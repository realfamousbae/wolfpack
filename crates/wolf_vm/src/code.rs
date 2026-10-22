//! The bytecode: instructions of a stack machine and the compiled functions.
//!
//! Every function has a frame of numbered slots (parameters first, then captured values, then
//! locals and temporaries) and an operand stack above it. Every expression leaves exactly one
//! value on the operand stack; a `Void` expression leaves `()`.

use std::fmt::Write as _;

use crate::value::Value;

/// An operator on values of known kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    /// `Int` `+` with an overflow check.
    Add,
    /// `Int` `-`
    Sub,
    /// `Int` `*`
    Mul,
    /// `Int` `/`
    Div,
    /// `Int` `%`
    Rem,
    /// `<<`
    Shl,
    /// `>>`
    Shr,
    /// `&`
    BitAnd,
    /// `|`
    BitOr,
    /// `^`
    BitXor,
    /// `Float` `+`
    FAdd,
    /// `Float` `-`
    FSub,
    /// `Float` `*`
    FMul,
    /// `Float` `/`
    FDiv,
    /// `Float` `%`
    FRem,
    /// `String` `+`
    Concat,
    /// `==` on any `Equatable` values.
    Eq,
    /// `!=`
    Ne,
    /// `<` on `Int`, `Float` or `String`.
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
}

/// A unary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    /// `Int` `-`
    Neg,
    /// `Float` `-`
    FNeg,
    /// `!`
    Not,
    /// `~`
    BitNot,
}

/// An instruction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    /// Pushes a constant of the function.
    Const(u32),
    /// Pushes `()`.
    Unit,
    /// Pushes an `Int`.
    Int(i64),
    /// Pushes a `Bool`.
    Bool(bool),
    /// Pushes the value of a slot.
    Load(u32),
    /// Pushes the value of the place that the reference in a slot points to.
    LoadRef(u32),
    /// Pops a value into a slot.
    Store(u32),
    /// Pops a value into the place that the reference in a slot points to.
    StoreRef(u32),
    /// Pushes a reference to a slot, or the reference the slot holds.
    RefSlot(u32),
    /// Pops a struct or variant and pushes a reference to its field.
    RefField(u32),
    /// Pops an index and a list and pushes a reference to the element.
    RefIndex,
    /// Pops a key and a map and pushes a reference to the value of the key.
    RefKey,
    /// Pops a value.
    Pop,
    /// Drops the operand stack down to this many values above the slots.
    PopTo(u32),
    /// Pops a struct or variant and pushes a field.
    Field(u32),
    /// Pops a value and a struct and sets a field.
    SetField(u32),
    /// Pops an index and a list and pushes the element.
    Index,
    /// Pops a key and a map and pushes the value.
    IndexKey,
    /// Pops a value, an index and a list and replaces the element.
    SetIndex,
    /// Pops a value, a key and a map and replaces the value of the key.
    SetKey,
    /// A slice of a list or string: pops the end if `to`, the start if `from`, then the value.
    Slice {
        /// A start was given.
        from: bool,
        /// An end was given.
        to: bool,
        /// `..=`
        inclusive: bool,
        /// A string, not a list.
        string: bool,
    },
    /// Pops the fields and makes a struct or variant.
    Make {
        /// The struct or variant.
        tag: u32,
        /// The number of fields.
        n: u32,
    },
    /// Pops written field values and a base struct and makes a struct with the base's other
    /// fields; the constant is the list of the written field positions.
    Update {
        /// The struct.
        tag: u32,
        /// A constant: a list of the positions of the written fields.
        fields: u32,
    },
    /// Pops elements and makes a list.
    List(u32),
    /// Pops keys and values (key first) and makes a map.
    Map(u32),
    /// Pops captured values and makes a function value.
    Closure {
        /// The function.
        func: u32,
        /// The number of captured values.
        n: u32,
    },
    /// Pops two values and pushes the result.
    Binary(BinOp),
    /// Pops a value and pushes the result.
    Unary(UnOp),
    /// Pops strings and pushes them joined.
    Concat(u32),
    /// Pops a value and pushes a deep copy.
    Copy,
    /// Pops a value and pushes whether it is this variant.
    IsTag(u32),
    /// Jumps.
    Jump(u32),
    /// Pops a `Bool` and jumps if it is `true`.
    JumpIf(u32),
    /// Pops a `Bool` and jumps if it is `false`.
    JumpUnless(u32),
    /// Calls a function with the values on top of the stack.
    Call {
        /// The function.
        func: u32,
        /// The number of arguments, the receiver included.
        argc: u32,
    },
    /// Calls the function value under the arguments.
    CallValue(u32),
    /// Calls a method of an interface on the receiver, the first of the arguments.
    CallMethod {
        /// A constant: the method name.
        name: u32,
        /// The number of arguments, the receiver included.
        argc: u32,
    },
    /// Pops the result and returns it.
    Return,
    /// Pops a message and panics at the location in the constant.
    Panic(u32),
    /// Panics with the whole message in the constant.
    Fail(u32),
    /// An arm that cannot be reached, after an exhaustive `match`.
    Unreachable,
}

/// A compiled function.
#[derive(Clone, Debug, Default)]
pub struct Chunk {
    /// Its name, for listings and internal errors.
    pub name: String,
    /// The number of parameters (the receiver included).
    pub params: u32,
    /// The number of captured values, after the parameters.
    pub captures: u32,
    /// The number of slots.
    pub slots: u32,
    /// The instructions.
    pub code: Vec<Op>,
    /// The constants.
    pub consts: Vec<Value>,
}

impl Chunk {
    /// A listing of the instructions, one per line; `tag` names a struct or variant and `func`
    /// a function.
    pub fn listing(&self, tag: &dyn Fn(u32) -> String, func: &dyn Fn(u32) -> String) -> String {
        let mut out = format!(
            "fn {} (params {}, captures {}, slots {})\n",
            self.name, self.params, self.captures, self.slots
        );
        for (i, op) in self.code.iter().enumerate() {
            let text = match op {
                Op::Const(c) => format!("Const {}", self.show_const(*c)),
                Op::Panic(c) => format!("Panic {}", self.show_const(*c)),
                Op::Fail(c) => format!("Fail {}", self.show_const(*c)),
                Op::CallMethod { name, argc } => {
                    format!("CallMethod {} {argc}", self.show_const(*name))
                }
                Op::Make { tag: t, n } => format!("Make {} {n}", tag(*t)),
                Op::Update { tag: t, fields } => {
                    format!("Update {} {}", tag(*t), self.show_const(*fields))
                }
                Op::IsTag(t) => format!("IsTag {}", tag(*t)),
                Op::Call { func: f, argc } => format!("Call {} {argc}", func(*f)),
                Op::Closure { func: f, n } => format!("Closure {} {n}", func(*f)),
                other => format!("{other:?}"),
            };
            let _ = writeln!(out, "{i:4}  {text}");
        }
        out
    }

    fn show_const(&self, c: u32) -> String {
        match self.consts.get(c as usize) {
            Some(Value::Str(s)) => format!("{s:?}"),
            Some(Value::Int(i)) => i.to_string(),
            Some(Value::Float(f)) => format!("{f:?}"),
            Some(Value::Bool(b)) => b.to_string(),
            Some(Value::List(l)) => format!(
                "{:?}",
                l.borrow()
                    .iter()
                    .map(|v| match v {
                        Value::Int(i) => *i,
                        _ => -1,
                    })
                    .collect::<Vec<_>>()
            ),
            Some(other) => format!("{other:?}"),
            None => format!("#{c}"),
        }
    }
}
