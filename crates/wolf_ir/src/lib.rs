//! The intermediate representation that every backend reads (spec chapter 8, section 1).
//!
//! The IR is the program after all checks: names are resolved to declarations and locals, every
//! expression has its type, method calls know their function, implicit conversions (interface
//! coercion, `?`) are explicit, and constants are evaluated. It still has the structure of the
//! source (nested expressions, `if` and `match` as expressions), because all three targets are
//! high-level languages. It is target-neutral: `Option` is the semantic `Option`; how a target
//! represents it is the backend's decision.

mod lower;

use std::collections::BTreeMap;

pub use lower::{LowerError, ProjectMeta, lower};
pub use wolf_ast::{BinaryOp, Mode, UnaryOp};
pub use wolf_types::ConstValue;

/// Identifies a declaration: a type, variant, function, method or constant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DefRef(pub u32);

/// Identifies a local variable, parameter or binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalRef(pub u32);

/// A whole program.
#[derive(Clone, Debug)]
pub struct Program {
    /// The package name from `wolfpack.toml`.
    pub name: String,
    /// The package version from `wolfpack.toml`.
    pub version: String,
    /// `cli` or `lib`.
    pub kind: ProgramKind,
    /// All modules, std first.
    pub modules: Vec<Module>,
    /// Every declaration.
    pub defs: BTreeMap<DefRef, DefInfo>,
    /// Every local, by [`LocalRef`].
    pub locals: BTreeMap<LocalRef, LocalInfo>,
}

impl Program {
    /// The declaration with this reference.
    pub fn def(&self, def: DefRef) -> Option<&DefInfo> {
        self.defs.get(&def)
    }
}

/// What a program is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramKind {
    /// A program with `main`.
    Cli,
    /// A library.
    Lib,
}

/// Facts about a declaration that code referring to it needs.
#[derive(Clone, Debug)]
pub struct DefInfo {
    /// Its name.
    pub name: String,
    /// The index of its module in [`Program::modules`].
    pub module: usize,
    /// `pub`.
    pub public: bool,
    /// What it is.
    pub kind: DefKind,
    /// For an `intrinsic` declaration of std: its qualified name, such as `std.list.List.push`.
    pub intrinsic: Option<String>,
}

/// The kinds of declarations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefKind {
    /// A struct.
    Struct,
    /// An enum.
    Enum,
    /// A variant of an enum.
    Variant {
        /// The enum.
        parent: DefRef,
    },
    /// An interface.
    Interface,
    /// A built-in type of std (`Int`, `List`...).
    BuiltinType,
    /// A free function.
    Fn,
    /// A function of an `impl` block.
    Method {
        /// The type of the block.
        owner: DefRef,
        /// Whether it takes `self`.
        has_self: bool,
    },
    /// A `config` constant.
    Const,
    /// A function from `extern`.
    ExternFn,
    /// An opaque type from `extern`.
    ExternType,
}

/// A local variable.
#[derive(Clone, Debug)]
pub struct LocalInfo {
    /// Its name in the source.
    pub name: String,
    /// Its type.
    pub ty: Type,
}

/// What a module is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleRole {
    /// A module of std.
    Std,
    /// `src/main.wolf` or `src/lib.wolf`.
    Entry,
    /// Another module under `src/`.
    Src,
    /// A module under `tests/`.
    Test,
    /// A module under `examples/`.
    Example,
}

/// A module.
#[derive(Clone, Debug)]
pub struct Module {
    /// The module path, such as `["todo", "model"]`.
    pub path: Vec<String>,
    /// The source file as shown to people, such as `src/todo/model.wolf`.
    pub file: String,
    /// What the module is for.
    pub role: ModuleRole,
    /// Inner doc comments.
    pub docs: Vec<String>,
    /// The declarations, in source order.
    pub items: Vec<Item>,
    /// `pub` imports of `src/lib.wolf`: names this module exports from other modules.
    pub reexports: Vec<Reexport>,
}

/// A name exported again by `src/lib.wolf`.
#[derive(Clone, Debug)]
pub struct Reexport {
    /// The exported name (the alias if there is one).
    pub name: String,
    /// The declaration.
    pub def: DefRef,
}

/// A declaration of a module.
#[derive(Clone, Debug)]
pub enum Item {
    /// A struct with its functions.
    Struct(Struct),
    /// An enum with its functions.
    Enum(Enum),
    /// An interface.
    Interface(Interface),
    /// A free function.
    Fn(Function),
    /// A `config` constant.
    Const(Const),
    /// An `extern` function.
    ExternFn(ExternFn),
    /// An opaque `extern` type.
    ExternType(ExternType),
    /// A test.
    Test(Test),
    /// A built-in type of std with its (intrinsic) functions.
    Builtin(Builtin),
}

/// A built-in type of std (`List`, `String`...) and the functions declared for it.
#[derive(Clone, Debug)]
pub struct Builtin {
    /// The declaration.
    pub def: DefRef,
    /// Its name.
    pub name: String,
    /// Its functions, usually intrinsic.
    pub methods: Vec<Function>,
}

/// A type parameter.
#[derive(Clone, Debug)]
pub struct Generic {
    /// Its name.
    pub name: String,
    /// The interfaces it must implement.
    pub bounds: Vec<Type>,
}

/// A struct.
#[derive(Clone, Debug)]
pub struct Struct {
    /// The declaration.
    pub def: DefRef,
    /// Its name.
    pub name: String,
    /// Doc comments.
    pub docs: Vec<String>,
    /// `pub`.
    pub public: bool,
    /// Type parameters.
    pub generics: Vec<Generic>,
    /// Fields in declaration order.
    pub fields: Vec<Field>,
    /// Functions of its `impl` blocks, inherent and from interface implementations.
    pub methods: Vec<Function>,
    /// Interfaces it implements.
    pub implements: Vec<Type>,
}

/// A field.
#[derive(Clone, Debug)]
pub struct Field {
    /// Its name.
    pub name: String,
    /// Its type.
    pub ty: Type,
    /// `pub`.
    pub public: bool,
    /// Doc comments.
    pub docs: Vec<String>,
}

/// An enum.
#[derive(Clone, Debug)]
pub struct Enum {
    /// The declaration.
    pub def: DefRef,
    /// Its name.
    pub name: String,
    /// Doc comments.
    pub docs: Vec<String>,
    /// `pub`.
    pub public: bool,
    /// Type parameters.
    pub generics: Vec<Generic>,
    /// Variants in declaration order.
    pub variants: Vec<Variant>,
    /// Functions of its `impl` blocks.
    pub methods: Vec<Function>,
    /// Interfaces it implements.
    pub implements: Vec<Type>,
}

/// A variant.
#[derive(Clone, Debug)]
pub struct Variant {
    /// The declaration.
    pub def: DefRef,
    /// Its name.
    pub name: String,
    /// Doc comments.
    pub docs: Vec<String>,
    /// Its payload.
    pub payload: Payload,
}

/// The payload of a variant.
#[derive(Clone, Debug)]
pub enum Payload {
    /// None.
    None,
    /// Positional values.
    Tuple(Vec<Type>),
    /// Named fields.
    Struct(Vec<Field>),
}

impl Payload {
    /// The number of values in the payload.
    pub fn len(&self) -> usize {
        match self {
            Payload::None => 0,
            Payload::Tuple(t) => t.len(),
            Payload::Struct(f) => f.len(),
        }
    }

    /// Whether the variant has no payload.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// An interface.
#[derive(Clone, Debug)]
pub struct Interface {
    /// The declaration.
    pub def: DefRef,
    /// Its name.
    pub name: String,
    /// Doc comments.
    pub docs: Vec<String>,
    /// `pub`.
    pub public: bool,
    /// Type parameters.
    pub generics: Vec<Generic>,
    /// Method signatures (functions without bodies).
    pub methods: Vec<Function>,
}

/// A parameter.
#[derive(Clone, Debug)]
pub struct Param {
    /// The local it binds.
    pub local: LocalRef,
    /// Its name.
    pub name: String,
    /// `read`, `mut` or `own`.
    pub mode: Mode,
    /// Its type.
    pub ty: Type,
}

/// A function, a method, or a signature of an interface.
#[derive(Clone, Debug)]
pub struct Function {
    /// The declaration.
    pub def: DefRef,
    /// Its name.
    pub name: String,
    /// Doc comments.
    pub docs: Vec<String>,
    /// `pub`.
    pub public: bool,
    /// Type parameters of the function itself.
    pub generics: Vec<Generic>,
    /// The mode of `self`, for methods.
    pub self_mode: Option<Mode>,
    /// Parameters without `self`.
    pub params: Vec<Param>,
    /// The result type.
    pub ret: Type,
    /// The body; `None` for intrinsic functions and interface signatures.
    pub body: Option<Block>,
    /// The interface, for a function of an interface implementation.
    pub interface: Option<DefRef>,
    /// A helper function of a `tests` section: part of test builds only.
    pub in_tests: bool,
}

/// A `config` constant with its value.
#[derive(Clone, Debug)]
pub struct Const {
    /// The declaration.
    pub def: DefRef,
    /// Its name.
    pub name: String,
    /// Doc comments.
    pub docs: Vec<String>,
    /// `pub`.
    pub public: bool,
    /// Its type.
    pub ty: Type,
    /// Its value, with overrides applied.
    pub value: ConstValue,
}

/// An `extern` function.
#[derive(Clone, Debug)]
pub struct ExternFn {
    /// The declaration.
    pub def: DefRef,
    /// Its name.
    pub name: String,
    /// `pub`.
    pub public: bool,
    /// Parameter types.
    pub params: Vec<(Mode, Type)>,
    /// The result type.
    pub ret: Type,
    /// The path string of each target that declares it (`ts`, `go`, `rust`).
    pub paths: BTreeMap<String, String>,
}

/// An opaque `extern` type.
#[derive(Clone, Debug)]
pub struct ExternType {
    /// The declaration.
    pub def: DefRef,
    /// Its name.
    pub name: String,
    /// The path string of each target that declares it.
    pub paths: BTreeMap<String, String>,
}

/// A test.
#[derive(Clone, Debug)]
pub struct Test {
    /// The test name.
    pub name: String,
    /// The body.
    pub body: Block,
}

/// A type.
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    /// `Int`
    Int,
    /// `Float`
    Float,
    /// `Bool`
    Bool,
    /// `String`
    String,
    /// `Void`
    Void,
    /// The type of expressions that never finish.
    Never,
    /// `List<T>`
    List(Box<Type>),
    /// `Map<K, V>`
    Map(Box<Type>, Box<Type>),
    /// `Option<T>`
    Option(Box<Type>),
    /// `Result<T, E>`
    Result(Box<Type>, Box<Type>),
    /// A struct, enum, interface or extern type with its type arguments.
    Named(DefRef, Vec<Type>),
    /// `fn(A, mut B) -> R`
    Fn(Vec<(Mode, Type)>, Box<Type>),
    /// A type parameter, by name.
    Param(String),
    /// `Self` in an interface.
    SelfType,
}

/// A block: statements and an optional final value.
#[derive(Clone, Debug, Default)]
pub struct Block {
    /// The statements.
    pub stmts: Vec<Stmt>,
    /// The value of the block; `None` if the block is `Void` (the final expression of a block
    /// whose value is discarded is a statement).
    pub tail: Option<Box<Expr>>,
}

/// A statement.
#[derive(Clone, Debug)]
pub enum Stmt {
    /// `let` or `var`.
    Let {
        /// The local.
        local: LocalRef,
        /// `var`
        mutable: bool,
        /// The initializer.
        value: Expr,
    },
    /// `place = value` or `place op= value`.
    Assign {
        /// A local, a field or an index.
        target: Expr,
        /// The operator of a compound assignment.
        op: Option<BinaryOp>,
        /// The value.
        value: Expr,
    },
    /// `while cond { ... }`
    While {
        /// The condition.
        cond: Expr,
        /// The body.
        body: Block,
    },
    /// `for x in list { ... }`
    ForList {
        /// The loop variable.
        var: LocalRef,
        /// `read`, `mut` or `own`.
        mode: Mode,
        /// The list.
        list: Expr,
        /// The body.
        body: Block,
    },
    /// `for i in a..b { ... }`
    ForRange {
        /// The loop variable.
        var: LocalRef,
        /// The start.
        start: Expr,
        /// The end.
        end: Expr,
        /// `..=`
        inclusive: bool,
        /// The body.
        body: Block,
    },
    /// An expression whose value is discarded (also `let _ = e`).
    Expr(Expr),
}

/// An expression with its type.
#[derive(Clone, Debug)]
pub struct Expr {
    /// What it is.
    pub kind: ExprKind,
    /// Its type.
    pub ty: Type,
    /// Where it is in the source.
    pub span: wolf_span::Span,
}

/// A call argument.
#[derive(Clone, Debug)]
pub struct Arg {
    /// The mode marker at the call site.
    pub mode: Mode,
    /// The value.
    pub value: Expr,
}

/// What a call calls.
#[derive(Clone, Debug)]
pub enum Callee {
    /// A free function, an associated function, an extern function or an intrinsic.
    Fn(DefRef),
    /// A method of a known type: `recv.m(...)`.
    Method {
        /// The method.
        method: DefRef,
        /// The receiver.
        recv: Box<Expr>,
    },
    /// A method of an interface, called on an interface value or on a value of a type parameter.
    Interface {
        /// The interface.
        interface: DefRef,
        /// The method name.
        name: String,
        /// The receiver.
        recv: Box<Expr>,
    },
    /// A function value: a closure or a function stored in a variable or field.
    Value(Box<Expr>),
}

/// The kinds of expressions.
#[derive(Clone, Debug)]
pub enum ExprKind {
    /// An `Int` literal.
    Int(i64),
    /// A `Float` literal.
    Float(f64),
    /// A `String` literal.
    Str(String),
    /// A `Bool` literal.
    Bool(bool),
    /// `()`
    Unit,
    /// A local.
    Local(LocalRef),
    /// A function used as a value.
    FnValue(DefRef),
    /// A constant.
    Const(DefRef),
    /// `self`
    SelfValue,
    /// A call.
    Call {
        /// What is called.
        callee: Callee,
        /// The arguments.
        args: Vec<Arg>,
    },
    /// `base.name`
    Field {
        /// The value.
        base: Box<Expr>,
        /// The field.
        name: String,
    },
    /// `base[index]` on a list or a map.
    Index {
        /// The list or map.
        base: Box<Expr>,
        /// The index or key.
        index: Box<Expr>,
    },
    /// `base[start..end]` on a list or a string.
    Slice {
        /// The list or string.
        base: Box<Expr>,
        /// The start (`0` if absent).
        start: Option<Box<Expr>>,
        /// The end (the length if absent).
        end: Option<Box<Expr>>,
        /// `..=`
        inclusive: bool,
    },
    /// A struct literal, fields in declaration order.
    Struct {
        /// The struct.
        def: DefRef,
        /// The field values, in declaration order.
        fields: Vec<(String, Expr)>,
    },
    /// A variant with its payload values, in declaration order.
    Variant {
        /// The variant.
        def: DefRef,
        /// The payload.
        values: Vec<Expr>,
    },
    /// `Some(value)`
    Some(Box<Expr>),
    /// `None`
    None,
    /// `Ok(value)`
    Ok(Box<Expr>),
    /// `Err(value)`
    Err(Box<Expr>),
    /// A list literal.
    List(Vec<Expr>),
    /// A map literal.
    Map(Vec<(Expr, Expr)>),
    /// A unary operator.
    Unary {
        /// The operator.
        op: UnaryOp,
        /// The operand.
        operand: Box<Expr>,
    },
    /// A binary operator; both operands have the same type, except for `&&` and `||`.
    Binary {
        /// The operator.
        op: BinaryOp,
        /// The left operand.
        lhs: Box<Expr>,
        /// The right operand.
        rhs: Box<Expr>,
    },
    /// A closure.
    Closure {
        /// Parameters.
        params: Vec<Param>,
        /// The body.
        body: Block,
    },
    /// `if`, with `else` or without.
    If {
        /// The condition.
        cond: Box<Expr>,
        /// The `then` branch.
        then: Block,
        /// The `else` branch.
        otherwise: Option<Block>,
    },
    /// `match`
    Match {
        /// The value matched.
        scrutinee: Box<Expr>,
        /// `match`, `match mut` or `match own`.
        mode: Mode,
        /// The arms.
        arms: Vec<Arm>,
    },
    /// A block.
    Block(Block),
    /// `return`
    Return(Option<Box<Expr>>),
    /// `break`
    Break,
    /// `continue`
    Continue,
    /// `value?`; for a `Result`, `coerce_error` is the interface the error is converted to.
    Try {
        /// The `Result` or `Option`.
        value: Box<Expr>,
        /// The interface type the error is coerced to, if any.
        coerce_error: Option<Type>,
    },
    /// A value of a concrete type used as a value of an interface type.
    Coerce {
        /// The value.
        value: Box<Expr>,
        /// The interface type.
        to: Type,
    },
    /// `value.copy()`
    Copy(Box<Expr>),
    /// `panic(message)` with the source location of the call.
    Panic {
        /// The message.
        message: Box<Expr>,
        /// `file:line:column`.
        location: String,
    },
    /// `assert(cond)` with the source text of the condition and the location of the call.
    Assert {
        /// The condition.
        cond: Box<Expr>,
        /// Its source text.
        text: String,
        /// `file:line:column`.
        location: String,
    },
}

/// An arm of a `match`.
#[derive(Clone, Debug)]
pub struct Arm {
    /// The pattern.
    pub pattern: Pattern,
    /// The guard.
    pub guard: Option<Expr>,
    /// The body.
    pub body: Block,
}

/// A pattern.
#[derive(Clone, Debug)]
pub enum Pattern {
    /// `_`
    Wild,
    /// A name that binds the value.
    Bind(LocalRef),
    /// An `Int` literal.
    Int(i64),
    /// A `String` literal.
    Str(String),
    /// A `Bool` literal.
    Bool(bool),
    /// A constant.
    Const(DefRef),
    /// A variant with patterns for its payload, in declaration order.
    Variant {
        /// The variant.
        def: DefRef,
        /// The payload patterns.
        fields: Vec<Pattern>,
    },
    /// `Some(p)`
    Some(Box<Pattern>),
    /// `None`
    None,
    /// `Ok(p)`
    Ok(Box<Pattern>),
    /// `Err(p)`
    Err(Box<Pattern>),
    /// A struct with patterns for its fields, in declaration order.
    Struct {
        /// The struct.
        def: DefRef,
        /// The field patterns.
        fields: Vec<Pattern>,
    },
    /// `p | q`
    Or(Vec<Pattern>),
}
