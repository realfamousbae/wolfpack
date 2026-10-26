//! Bytecode files (spec chapter 10, section 1b): `wolf build --vm` writes the compiled image of a
//! program to a file, and `wolf run <file>.wbc` reads it back and runs it without a project.
//!
//! The format is not stable (section 4, rule 6): a file is read only by the version of `wolf`
//! that wrote it. It is, in order:
//!
//! 1. the header line `wolfpack bytecode <version>\n`;
//! 2. the FNV-1a (64 bits) checksum of the rest of the file, as 8 bytes, little endian, so that a
//!    damaged file is found before it runs;
//! 3. the image: the `main` to run, the tags of std, the built-in types, every function, the
//!    methods for the dispatch of interface calls, the types that implement `Display` and the
//!    enum of each variant.
//!
//! Numbers are LEB128 varints (signed ones zigzag-encoded), a `Float` is its 8 bytes in little
//! endian, a string is its length in bytes and its UTF-8 text, and a list is its length and its
//! elements. Maps and sets are written sorted, so a build writes the same bytes every time.
//!
//! Reading checks everything the VM relies on: the numbers of functions, constants, slots and
//! jump targets, and the kinds of the constants that instructions take. The VM itself then
//! never meets a broken image, which it would report as an internal error.

use std::collections::{HashMap, HashSet};

use crate::code::{BinOp, Chunk, Op, UnOp};
use crate::compile::{Builtins, Func, Image, Tags, compile};
use crate::intrinsics::Intrinsic;
use crate::value::Value;

/// The start of the header line of a bytecode file; the version of `wolf` follows it.
pub const HEADER: &str = "wolfpack bytecode";

/// The most slots a function of a bytecode file may have: the VM makes room for all of them on
/// every call.
const MAX_SLOTS: u32 = 1 << 20;

/// The longest version a header may name.
const MAX_VERSION: usize = 64;

/// What `main` a file runs: the function, whether it takes the arguments and whether it returns
/// a `Result` (as [`Image::main_of`] gives it).
type Main = (u32, bool, bool);

/// A program read from a bytecode file, ready to run.
#[derive(Clone, Debug)]
pub struct Executable {
    image: Image,
    main: Main,
}

impl Executable {
    /// Runs the program and returns its exit code, as [`crate::run_main`] does.
    pub fn run(&self, io: &mut dyn crate::Io) -> Result<i32, String> {
        crate::run_image(&self.image, self.main, io)
    }
}

/// Why a bytecode file cannot be run (E0809).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The file does not start with the header: it is not a bytecode file.
    NotBytecode,
    /// The file was written by another version of `wolf`, named here.
    OtherVersion(String),
    /// The header line is broken, or it names this version but the rest of the file is damaged:
    /// what is wrong.
    Damaged(String),
}

/// Compiles a program and gives the bytecode file that runs the `main` of the module `module`
/// (the entry module or an example), with `version` in its header. An `Err` is a bug of the
/// compiler, with its description.
pub fn write_file(
    program: &wolf_ir::Program,
    module: usize,
    version: &str,
) -> Result<Vec<u8>, String> {
    let image = compile(program)?;
    let main = image
        .main_of(module)
        .ok_or_else(|| "the module has no `main`".to_string())?;
    let mut body = Writer::default();
    body.image(&image, main)?;
    let mut out = format!("{HEADER} {version}\n").into_bytes();
    out.extend(checksum(&body.out).to_le_bytes());
    out.extend(body.out);
    Ok(out)
}

/// Reads a bytecode file written by the version `version` of `wolf`.
pub fn read_file(bytes: &[u8], version: &str) -> Result<Executable, ReadError> {
    let prefix = format!("{HEADER} ");
    let Some(rest) = bytes.strip_prefix(prefix.as_bytes()) else {
        return Err(ReadError::NotBytecode);
    };
    let broken = || ReadError::Damaged("its header line is broken".to_string());
    let end = rest
        .iter()
        .take(MAX_VERSION + 1)
        .position(|b| *b == b'\n')
        .ok_or_else(broken)?;
    let (found, rest) = rest.split_at_checked(end).ok_or_else(broken)?;
    let found = std::str::from_utf8(found).map_err(|_| broken())?;
    let sane = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+');
    if found.is_empty() || !found.chars().all(sane) {
        return Err(broken());
    }
    if found != version {
        return Err(ReadError::OtherVersion(found.to_string()));
    }
    let rest = rest.get(1..).unwrap_or_default();
    let (sum, body) = rest
        .split_first_chunk::<8>()
        .ok_or_else(|| ReadError::Damaged("it ends after its header".to_string()))?;
    if u64::from_le_bytes(*sum) != checksum(body) {
        return Err(ReadError::Damaged(
            "its checksum does not match its contents".to_string(),
        ));
    }
    read_body(body).map_err(ReadError::Damaged)
}

/// Reads and checks the image of a file whose header and checksum are right.
fn read_body(body: &[u8]) -> Result<Executable, String> {
    let mut r = Reader { bytes: body, at: 0 };
    let (image, main) = r.image()?;
    if r.remaining() > 0 {
        return Err("it has bytes after the end of the program".to_string());
    }
    check(&image, main)?;
    Ok(Executable { image, main })
}

/// The FNV-1a hash of 64 bits: a change of any one byte changes it.
fn checksum(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

// ----- writing ------------------------------------------------------------------------------------

#[derive(Default)]
struct Writer {
    out: Vec<u8>,
}

impl Writer {
    fn byte(&mut self, b: u8) {
        self.out.push(b);
    }

    fn u64(&mut self, mut v: u64) {
        loop {
            let low = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                self.byte(low);
                return;
            }
            self.byte(low | 0x80);
        }
    }

    fn u32(&mut self, v: u32) {
        self.u64(u64::from(v));
    }

    fn i64(&mut self, v: i64) {
        self.u64(((v << 1) ^ (v >> 63)) as u64);
    }

    fn bool(&mut self, b: bool) {
        self.byte(u8::from(b));
    }

    fn len(&mut self, n: usize) -> Result<(), String> {
        let n = u32::try_from(n).map_err(|_| "too many values for a bytecode file".to_string())?;
        self.u32(n);
        Ok(())
    }

    fn str(&mut self, s: &str) -> Result<(), String> {
        self.len(s.len())?;
        self.out.extend(s.as_bytes());
        Ok(())
    }

    fn image(&mut self, image: &Image, main: Main) -> Result<(), String> {
        let (func, takes_args, result) = main;
        self.u32(func);
        self.bool(takes_args);
        self.bool(result);
        let t = &image.tags;
        for tag in [
            t.some,
            t.none,
            t.ok,
            t.err,
            t.less,
            t.equal,
            t.greater,
            t.entry,
            t.parse_error,
            t.not_found,
            t.permission_denied,
            t.other,
        ] {
            self.u32(tag);
        }
        let b = &image.builtins;
        for builtin in [b.int, b.float, b.bool, b.string, b.list, b.map, b.void] {
            self.u32(builtin);
        }
        self.len(image.funcs.len())?;
        for f in &image.funcs {
            match f {
                Func::Code(chunk) => {
                    self.byte(0);
                    self.chunk(chunk)?;
                }
                Func::Intrinsic(i) => {
                    self.byte(1);
                    self.str(i.name())?;
                }
                Func::Missing(what) => {
                    self.byte(2);
                    self.str(what)?;
                }
            }
        }
        let mut dispatch: Vec<_> = image.dispatch.iter().collect();
        dispatch.sort();
        self.len(dispatch.len())?;
        for ((ty, name), func) in dispatch {
            self.u32(*ty);
            self.str(name)?;
            self.u32(*func);
        }
        let mut display: Vec<u32> = image.display.iter().copied().collect();
        display.sort_unstable();
        self.len(display.len())?;
        for ty in display {
            self.u32(ty);
        }
        let mut parent: Vec<(u32, u32)> = image.parent.iter().map(|(v, e)| (*v, *e)).collect();
        parent.sort_unstable();
        self.len(parent.len())?;
        for (variant, owner) in parent {
            self.u32(variant);
            self.u32(owner);
        }
        Ok(())
    }

    fn chunk(&mut self, c: &Chunk) -> Result<(), String> {
        self.str(&c.name)?;
        self.u32(c.params);
        self.u32(c.captures);
        self.u32(c.slots);
        self.len(c.code.len())?;
        for op in &c.code {
            self.op(*op);
        }
        self.len(c.consts.len())?;
        for v in &c.consts {
            match v {
                Value::List(items) => {
                    self.byte(6);
                    let items = items.borrow();
                    self.len(items.len())?;
                    for item in items.iter() {
                        self.scalar(item)?;
                    }
                }
                other => self.scalar(other)?,
            }
        }
        Ok(())
    }

    /// A constant that is not a list.
    fn scalar(&mut self, v: &Value) -> Result<(), String> {
        match v {
            Value::Unit => self.byte(0),
            Value::Int(i) => {
                self.byte(1);
                self.i64(*i);
            }
            Value::Float(f) => {
                self.byte(2);
                self.out.extend(f.to_bits().to_le_bytes());
            }
            Value::Bool(b) => {
                self.byte(3);
                self.bool(*b);
            }
            Value::Str(s) => {
                self.byte(4);
                self.str(s)?;
            }
            Value::Shown(s) => {
                self.byte(5);
                self.str(s)?;
            }
            _ => return Err("a constant of this kind cannot be written to a file".to_string()),
        }
        Ok(())
    }

    fn op(&mut self, op: Op) {
        match op {
            Op::Const(k) => self.op1(0, k),
            Op::Unit => self.byte(1),
            Op::Int(i) => {
                self.byte(2);
                self.i64(i);
            }
            Op::Bool(b) => {
                self.byte(3);
                self.bool(b);
            }
            Op::Load(s) => self.op1(4, s),
            Op::LoadRef(s) => self.op1(5, s),
            Op::Store(s) => self.op1(6, s),
            Op::StoreRef(s) => self.op1(7, s),
            Op::RefSlot(s) => self.op1(8, s),
            Op::RefField(i) => self.op1(9, i),
            Op::RefIndex => self.byte(10),
            Op::RefKey => self.byte(11),
            Op::Pop => self.byte(12),
            Op::PopTo(n) => self.op1(13, n),
            Op::Field(i) => self.op1(14, i),
            Op::SetField(i) => self.op1(15, i),
            Op::Index => self.byte(16),
            Op::IndexKey => self.byte(17),
            Op::SetIndex => self.byte(18),
            Op::SetKey => self.byte(19),
            Op::Slice {
                from,
                to,
                inclusive,
                string,
            } => {
                self.byte(20);
                let flags = u8::from(from)
                    | u8::from(to) << 1
                    | u8::from(inclusive) << 2
                    | u8::from(string) << 3;
                self.byte(flags);
            }
            Op::Make { tag, n } => self.op2(21, tag, n),
            Op::Update { tag, fields } => self.op2(22, tag, fields),
            Op::List(n) => self.op1(23, n),
            Op::Map(n) => self.op1(24, n),
            Op::Closure { func, n } => self.op2(25, func, n),
            Op::Binary(b) => {
                self.byte(26);
                self.byte(binary_code(b));
            }
            Op::Unary(u) => {
                self.byte(27);
                self.byte(unary_code(u));
            }
            Op::Concat(n) => self.op1(28, n),
            Op::Copy => self.byte(29),
            Op::Shown => self.byte(30),
            Op::IsTag(t) => self.op1(31, t),
            Op::Jump(t) => self.op1(32, t),
            Op::JumpIf(t) => self.op1(33, t),
            Op::JumpUnless(t) => self.op1(34, t),
            Op::Call { func, argc } => self.op2(35, func, argc),
            Op::CallValue(argc) => self.op1(36, argc),
            Op::CallMethod { name, argc } => self.op2(37, name, argc),
            Op::Return => self.byte(38),
            Op::Panic(k) => self.op1(39, k),
            Op::Fail(k) => self.op1(40, k),
            Op::Unreachable => self.byte(41),
        }
    }

    fn op1(&mut self, code: u8, a: u32) {
        self.byte(code);
        self.u32(a);
    }

    fn op2(&mut self, code: u8, a: u32, b: u32) {
        self.byte(code);
        self.u32(a);
        self.u32(b);
    }
}

/// The operators in the order of their codes in a file.
const BINARY: [BinOp; 22] = [
    BinOp::Add,
    BinOp::Sub,
    BinOp::Mul,
    BinOp::Div,
    BinOp::Rem,
    BinOp::Shl,
    BinOp::Shr,
    BinOp::BitAnd,
    BinOp::BitOr,
    BinOp::BitXor,
    BinOp::FAdd,
    BinOp::FSub,
    BinOp::FMul,
    BinOp::FDiv,
    BinOp::FRem,
    BinOp::Concat,
    BinOp::Eq,
    BinOp::Ne,
    BinOp::Lt,
    BinOp::Le,
    BinOp::Gt,
    BinOp::Ge,
];

const UNARY: [UnOp; 4] = [UnOp::Neg, UnOp::FNeg, UnOp::Not, UnOp::BitNot];

fn binary_code(op: BinOp) -> u8 {
    let at = BINARY.iter().position(|b| *b == op).unwrap_or(0);
    u8::try_from(at).unwrap_or(0)
}

fn unary_code(op: UnOp) -> u8 {
    let at = UNARY.iter().position(|u| *u == op).unwrap_or(0);
    u8::try_from(at).unwrap_or(0)
}

// ----- reading ------------------------------------------------------------------------------------

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

fn too_short() -> String {
    "it ends in the middle of the program".to_string()
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).ok_or_else(too_short)?;
        let part = self.bytes.get(self.at..end).ok_or_else(too_short)?;
        self.at = end;
        Ok(part)
    }

    fn byte(&mut self) -> Result<u8, String> {
        let b = *self.bytes.get(self.at).ok_or_else(too_short)?;
        self.at += 1;
        Ok(b)
    }

    fn u64(&mut self) -> Result<u64, String> {
        let mut v: u64 = 0;
        for i in 0..10 {
            let b = self.byte()?;
            if i == 9 && b > 1 {
                return Err("a number is too large".to_string());
            }
            v |= u64::from(b & 0x7f) << (7 * i);
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err("a number is too large".to_string())
    }

    fn u32(&mut self) -> Result<u32, String> {
        u32::try_from(self.u64()?).map_err(|_| "a number is too large".to_string())
    }

    fn i64(&mut self) -> Result<i64, String> {
        let u = self.u64()?;
        Ok(((u >> 1) as i64) ^ -((u & 1) as i64))
    }

    fn f64(&mut self) -> Result<f64, String> {
        let bytes: [u8; 8] = self.take(8)?.try_into().map_err(|_| too_short())?;
        Ok(f64::from_bits(u64::from_le_bytes(bytes)))
    }

    fn bool(&mut self) -> Result<bool, String> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err("a flag is neither 0 nor 1".to_string()),
        }
    }

    /// The length of a list whose elements take at least a byte each.
    fn len(&mut self) -> Result<usize, String> {
        let n = self.u32()? as usize;
        if n > self.remaining() {
            return Err("a length is larger than the rest of the file".to_string());
        }
        Ok(n)
    }

    fn str(&mut self) -> Result<String, String> {
        let n = self.len()?;
        let bytes = self.take(n)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| "a text is not valid UTF-8".to_string())
    }

    fn image(&mut self) -> Result<(Image, Main), String> {
        let main = (self.u32()?, self.bool()?, self.bool()?);
        let tags = Tags {
            some: self.u32()?,
            none: self.u32()?,
            ok: self.u32()?,
            err: self.u32()?,
            less: self.u32()?,
            equal: self.u32()?,
            greater: self.u32()?,
            entry: self.u32()?,
            parse_error: self.u32()?,
            not_found: self.u32()?,
            permission_denied: self.u32()?,
            other: self.u32()?,
        };
        let builtins = Builtins {
            int: self.u32()?,
            float: self.u32()?,
            bool: self.u32()?,
            string: self.u32()?,
            list: self.u32()?,
            map: self.u32()?,
            void: self.u32()?,
        };
        let mut funcs = Vec::new();
        for _ in 0..self.len()? {
            let f =
                match self.byte()? {
                    0 => Func::Code(self.chunk()?),
                    1 => {
                        let name = self.str()?;
                        Func::Intrinsic(Intrinsic::from_name(&name).ok_or_else(|| {
                            format!("it names an unknown function `{name}` of std")
                        })?)
                    }
                    2 => Func::Missing(self.str()?),
                    _ => return Err("a function is of an unknown kind".to_string()),
                };
            funcs.push(f);
        }
        let mut dispatch = HashMap::new();
        for _ in 0..self.len()? {
            let ty = self.u32()?;
            let name = self.str()?;
            dispatch.insert((ty, name), self.u32()?);
        }
        let mut display = HashSet::new();
        for _ in 0..self.len()? {
            display.insert(self.u32()?);
        }
        let mut parent = HashMap::new();
        for _ in 0..self.len()? {
            let variant = self.u32()?;
            parent.insert(variant, self.u32()?);
        }
        let image = Image {
            funcs,
            dispatch,
            display,
            parent,
            tags,
            builtins,
            mains: HashMap::new(),
            tests: Vec::new(),
            names: HashMap::new(),
            listed: Vec::new(),
        };
        Ok((image, main))
    }

    fn chunk(&mut self) -> Result<Chunk, String> {
        let name = self.str()?;
        let params = self.u32()?;
        let captures = self.u32()?;
        let slots = self.u32()?;
        let mut code = Vec::new();
        for _ in 0..self.len()? {
            code.push(self.op()?);
        }
        let mut consts = Vec::new();
        for _ in 0..self.len()? {
            let v = match self.byte()? {
                6 => {
                    let mut items = Vec::new();
                    for _ in 0..self.len()? {
                        let kind = self.byte()?;
                        items.push(self.scalar(kind)?);
                    }
                    Value::list(items)
                }
                kind => self.scalar(kind)?,
            };
            consts.push(v);
        }
        Ok(Chunk {
            name,
            params,
            captures,
            slots,
            code,
            consts,
        })
    }

    fn scalar(&mut self, kind: u8) -> Result<Value, String> {
        Ok(match kind {
            0 => Value::Unit,
            1 => Value::Int(self.i64()?),
            2 => Value::Float(self.f64()?),
            3 => Value::Bool(self.bool()?),
            4 => Value::str(&self.str()?),
            5 => Value::Shown(self.str()?.into()),
            _ => return Err("a constant is of an unknown kind".to_string()),
        })
    }

    fn op(&mut self) -> Result<Op, String> {
        Ok(match self.byte()? {
            0 => Op::Const(self.u32()?),
            1 => Op::Unit,
            2 => Op::Int(self.i64()?),
            3 => Op::Bool(self.bool()?),
            4 => Op::Load(self.u32()?),
            5 => Op::LoadRef(self.u32()?),
            6 => Op::Store(self.u32()?),
            7 => Op::StoreRef(self.u32()?),
            8 => Op::RefSlot(self.u32()?),
            9 => Op::RefField(self.u32()?),
            10 => Op::RefIndex,
            11 => Op::RefKey,
            12 => Op::Pop,
            13 => Op::PopTo(self.u32()?),
            14 => Op::Field(self.u32()?),
            15 => Op::SetField(self.u32()?),
            16 => Op::Index,
            17 => Op::IndexKey,
            18 => Op::SetIndex,
            19 => Op::SetKey,
            20 => {
                let flags = self.byte()?;
                if flags > 0b1111 {
                    return Err("a slice has unknown flags".to_string());
                }
                Op::Slice {
                    from: flags & 1 != 0,
                    to: flags & 2 != 0,
                    inclusive: flags & 4 != 0,
                    string: flags & 8 != 0,
                }
            }
            21 => Op::Make {
                tag: self.u32()?,
                n: self.u32()?,
            },
            22 => Op::Update {
                tag: self.u32()?,
                fields: self.u32()?,
            },
            23 => Op::List(self.u32()?),
            24 => Op::Map(self.u32()?),
            25 => Op::Closure {
                func: self.u32()?,
                n: self.u32()?,
            },
            26 => {
                let b = self.byte()?;
                Op::Binary(
                    *BINARY
                        .get(usize::from(b))
                        .ok_or_else(|| "an operator is unknown".to_string())?,
                )
            }
            27 => {
                let u = self.byte()?;
                Op::Unary(
                    *UNARY
                        .get(usize::from(u))
                        .ok_or_else(|| "an operator is unknown".to_string())?,
                )
            }
            28 => Op::Concat(self.u32()?),
            29 => Op::Copy,
            30 => Op::Shown,
            31 => Op::IsTag(self.u32()?),
            32 => Op::Jump(self.u32()?),
            33 => Op::JumpIf(self.u32()?),
            34 => Op::JumpUnless(self.u32()?),
            35 => Op::Call {
                func: self.u32()?,
                argc: self.u32()?,
            },
            36 => Op::CallValue(self.u32()?),
            37 => Op::CallMethod {
                name: self.u32()?,
                argc: self.u32()?,
            },
            38 => Op::Return,
            39 => Op::Panic(self.u32()?),
            40 => Op::Fail(self.u32()?),
            41 => Op::Unreachable,
            _ => return Err("an instruction is unknown".to_string()),
        })
    }
}

// ----- checking -----------------------------------------------------------------------------------

/// Checks that the VM can run the image: every number that names a function, a constant, a
/// slot or an instruction names one that exists, and is of the kind the instruction needs.
fn check(image: &Image, main: Main) -> Result<(), String> {
    let funcs = image.funcs.len();
    let (entry, takes_args, _) = main;
    match image.funcs.get(entry as usize) {
        Some(Func::Code(c)) if c.params == u32::from(takes_args) && c.captures == 0 => {}
        _ => return Err("its `main` is not a function of the program".to_string()),
    }
    for f in &image.funcs {
        if let Func::Code(c) = f {
            check_chunk(c, funcs).map_err(|e| format!("in `{}`, {e}", c.name))?;
        }
    }
    if image.dispatch.values().any(|f| *f as usize >= funcs) {
        return Err("a method is not a function of the program".to_string());
    }
    Ok(())
}

fn check_chunk(c: &Chunk, funcs: usize) -> Result<(), String> {
    let given = c.params.checked_add(c.captures);
    if c.slots > MAX_SLOTS || given.is_none_or(|g| g > MAX_SLOTS) {
        return Err("a function has too many slots".to_string());
    }
    let len = c.code.len();
    let slot = |s: u32| {
        if s < c.slots {
            Ok(())
        } else {
            Err("an instruction names a slot that does not exist".to_string())
        }
    };
    let func = |f: u32| {
        if (f as usize) < funcs {
            Ok(())
        } else {
            Err("an instruction calls a function that does not exist".to_string())
        }
    };
    // Every value on the stack was pushed by an instruction of the function, so no count of
    // values can be larger than the number of instructions.
    let values = |n: u32| {
        if (n as usize) <= len {
            Ok(())
        } else {
            Err("an instruction takes more values than the function can have".to_string())
        }
    };
    let konst = |k: u32| {
        c.consts
            .get(k as usize)
            .ok_or_else(|| "an instruction names a constant that does not exist".to_string())
    };
    let text = |k: u32| match konst(k)? {
        Value::Str(_) => Ok(()),
        _ => Err("an instruction needs a text constant".to_string()),
    };
    for op in &c.code {
        match *op {
            Op::Const(k) => konst(k).map(|_| ())?,
            Op::Load(s) | Op::LoadRef(s) | Op::Store(s) | Op::StoreRef(s) | Op::RefSlot(s) => {
                slot(s)?;
            }
            Op::PopTo(n) | Op::List(n) | Op::Concat(n) | Op::Make { n, .. } => values(n)?,
            Op::Map(n) => values(n.saturating_mul(2))?,
            Op::Closure { func: f, n } => {
                func(f)?;
                values(n)?;
            }
            Op::Call { func: f, argc } => {
                func(f)?;
                values(argc)?;
            }
            Op::CallValue(argc) => values(argc.saturating_add(1))?,
            Op::CallMethod { name, argc } => {
                text(name)?;
                values(argc)?;
            }
            Op::Update { fields, .. } => match konst(fields)? {
                Value::List(l) if l.borrow().iter().all(|v| matches!(v, Value::Int(_))) => {}
                _ => return Err("an update names no list of fields".to_string()),
            },
            Op::Panic(k) | Op::Fail(k) => text(k)?,
            Op::Jump(t) | Op::JumpIf(t) | Op::JumpUnless(t) => {
                if t as usize >= len {
                    return Err("a jump leaves the function".to_string());
                }
            }
            Op::Unit
            | Op::Int(_)
            | Op::Bool(_)
            | Op::RefField(_)
            | Op::RefIndex
            | Op::RefKey
            | Op::Pop
            | Op::Field(_)
            | Op::SetField(_)
            | Op::Index
            | Op::IndexKey
            | Op::SetIndex
            | Op::SetKey
            | Op::Slice { .. }
            | Op::Binary(_)
            | Op::Unary(_)
            | Op::Copy
            | Op::Shown
            | Op::IsTag(_)
            | Op::Return
            | Op::Unreachable => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One instruction of each kind, with operands that are not all zero.
    fn every_op() -> Vec<Op> {
        let mut ops = vec![
            Op::Const(1),
            Op::Unit,
            Op::Int(-300),
            Op::Int(i64::MIN),
            Op::Int(i64::MAX),
            Op::Bool(true),
            Op::Load(2),
            Op::LoadRef(3),
            Op::Store(4),
            Op::StoreRef(5),
            Op::RefSlot(6),
            Op::RefField(7),
            Op::RefIndex,
            Op::RefKey,
            Op::Pop,
            Op::PopTo(8),
            Op::Field(9),
            Op::SetField(10),
            Op::Index,
            Op::IndexKey,
            Op::SetIndex,
            Op::SetKey,
            Op::Make { tag: 11, n: 12 },
            Op::Update {
                tag: 13,
                fields: 14,
            },
            Op::List(15),
            Op::Map(16),
            Op::Closure { func: 17, n: 18 },
            Op::Concat(19),
            Op::Copy,
            Op::Shown,
            Op::IsTag(u32::MAX),
            Op::Jump(20),
            Op::JumpIf(21),
            Op::JumpUnless(22),
            Op::Call { func: 23, argc: 24 },
            Op::CallValue(25),
            Op::CallMethod { name: 26, argc: 27 },
            Op::Return,
            Op::Panic(28),
            Op::Fail(29),
            Op::Unreachable,
        ];
        for flags in 0..16u8 {
            ops.push(Op::Slice {
                from: flags & 1 != 0,
                to: flags & 2 != 0,
                inclusive: flags & 4 != 0,
                string: flags & 8 != 0,
            });
        }
        ops.extend(BINARY.iter().map(|b| Op::Binary(*b)));
        ops.extend(UNARY.iter().map(|u| Op::Unary(*u)));
        ops
    }

    #[test]
    fn every_instruction_is_read_back_as_written() {
        let ops = every_op();
        let mut w = Writer::default();
        for op in &ops {
            w.op(*op);
        }
        let mut r = Reader {
            bytes: &w.out,
            at: 0,
        };
        for op in &ops {
            assert_eq!(r.op(), Ok(*op));
        }
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn numbers_are_read_back_as_written() {
        let mut w = Writer::default();
        let ints = [0, 1, -1, 63, -64, 64, i64::MIN, i64::MAX];
        for i in ints {
            w.i64(i);
        }
        w.u32(u32::MAX);
        let mut r = Reader {
            bytes: &w.out,
            at: 0,
        };
        for i in ints {
            assert_eq!(r.i64(), Ok(i));
        }
        assert_eq!(r.u32(), Ok(u32::MAX));
        // Eleven bytes with the continuation bit: too long for any number.
        let mut r = Reader {
            bytes: &[0xff; 11],
            at: 0,
        };
        assert!(r.u64().is_err());
    }

    #[test]
    fn the_header_decides_the_kind_of_error() {
        assert_eq!(
            read_file(b"module main\n", "1.0.0").err(),
            Some(ReadError::NotBytecode)
        );
        assert_eq!(
            read_file(b"wolfpack bytecode 0.9.0\n", "1.0.0").err(),
            Some(ReadError::OtherVersion("0.9.0".to_string()))
        );
        assert!(matches!(
            read_file(b"wolfpack bytecode 1.0.0", "1.0.0"),
            Err(ReadError::Damaged(_))
        ));
        assert!(matches!(
            read_file(b"wolfpack bytecode 1.0.0\n1234", "1.0.0"),
            Err(ReadError::Damaged(_))
        ));
        assert!(matches!(
            read_file(b"wolfpack bytecode \x01\n", "1.0.0"),
            Err(ReadError::Damaged(_))
        ));
    }
}
