//! The syntax tree of wolfpack (spec chapters 2 and 3).
//!
//! The tree mirrors the source closely: it keeps sections in written order, doc comments and
//! spans, and it may contain [`ExprKind::Error`] and similar nodes where the parser recovered from
//! an error. Meaning is assigned by later phases; for example `a.b.c` is a chain of field accesses
//! here, whether `a` turns out to be a module, a type or a variable.

mod dump;

use wolf_span::Span;

pub use dump::dump;

/// Identifies an expression, pattern or type node, for side tables in later phases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

/// A name with its position.
#[derive(Clone, Debug, PartialEq)]
pub struct Ident {
    /// The text of the name.
    pub name: String,
    /// Where it is.
    pub span: Span,
}

/// A dotted name such as `users.model` or `StorageError.Full`.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    /// The parts between the dots, at least one.
    pub segments: Vec<Ident>,
    /// The whole path.
    pub span: Span,
}

/// A doc comment.
#[derive(Clone, Debug, PartialEq)]
pub struct Doc {
    /// The text after the marker.
    pub text: String,
    /// `true` for `//!` and `/*! */`.
    pub inner: bool,
    /// Where it is.
    pub span: Span,
}

/// A whole source file.
#[derive(Clone, Debug, PartialEq)]
pub struct File {
    /// Inner doc comments before the `module` line: the module documentation.
    pub docs: Vec<Doc>,
    /// The `module` line, if present.
    pub module: Option<Path>,
    /// Sections in the order they are written.
    pub sections: Vec<Section>,
}

/// The nine parts of a file, in their required order (spec chapter 2, section 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SectionKind {
    /// `imports`
    Imports,
    /// `extern`
    Extern,
    /// `config`
    Config,
    /// `types`
    Types,
    /// `interfaces`
    Interfaces,
    /// `impl`
    Impl,
    /// `main`: the entry point (spec chapter 2, section 3.7)
    Main,
    /// `functions`
    Functions,
    /// `tests`
    Tests,
}

impl SectionKind {
    /// All sections in their required order.
    pub const ALL: [SectionKind; 9] = [
        SectionKind::Imports,
        SectionKind::Extern,
        SectionKind::Config,
        SectionKind::Types,
        SectionKind::Interfaces,
        SectionKind::Impl,
        SectionKind::Main,
        SectionKind::Functions,
        SectionKind::Tests,
    ];

    /// The name of the section as written in source.
    pub fn name(self) -> &'static str {
        match self {
            SectionKind::Imports => "imports",
            SectionKind::Extern => "extern",
            SectionKind::Config => "config",
            SectionKind::Types => "types",
            SectionKind::Interfaces => "interfaces",
            SectionKind::Impl => "impl",
            SectionKind::Main => "main",
            SectionKind::Functions => "functions",
            SectionKind::Tests => "tests",
        }
    }

    /// The section with this name.
    pub fn from_name(name: &str) -> Option<SectionKind> {
        SectionKind::ALL
            .into_iter()
            .find(|kind| kind.name() == name)
    }
}

/// A section and its entries.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    /// Which section.
    pub kind: SectionKind,
    /// The section name in the header.
    pub name_span: Span,
    /// From the name to the closing `}`.
    pub span: Span,
    /// The entries. They may include entries that belong to another section; the parser reports
    /// those (E0205) and keeps them.
    pub items: Vec<Item>,
}

/// A declaration inside a section.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Outer doc comments before it.
    pub docs: Vec<Doc>,
    /// What it is.
    pub kind: ItemKind,
    /// The whole declaration.
    pub span: Span,
}

/// The kinds of declarations.
#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind {
    /// An entry of `imports`.
    Import(Import),
    /// A target block of `extern`: `go { ... }`.
    ExternTarget(ExternTarget),
    /// A constant of `config`.
    Config(ConfigEntry),
    /// `struct`
    Struct(StructDecl),
    /// `enum`
    Enum(EnumDecl),
    /// `type Name = Type`
    Alias(AliasDecl),
    /// `intrinsic type Name<T>`: a built-in type declared in std (chapter 7, section 1).
    IntrinsicType(IntrinsicTypeDecl),
    /// `interface`
    Interface(InterfaceDecl),
    /// A block of `impl`.
    Impl(ImplBlock),
    /// `fn`
    Fn(FnDecl),
    /// `test "name" { ... }`
    Test(TestDecl),
}

impl ItemKind {
    /// The section this kind of declaration belongs in. Functions are also allowed in `tests`.
    pub fn home(&self) -> SectionKind {
        match self {
            ItemKind::Import(_) => SectionKind::Imports,
            ItemKind::ExternTarget(_) => SectionKind::Extern,
            ItemKind::Config(_) => SectionKind::Config,
            ItemKind::Struct(_)
            | ItemKind::Enum(_)
            | ItemKind::Alias(_)
            | ItemKind::IntrinsicType(_) => SectionKind::Types,
            ItemKind::Interface(_) => SectionKind::Interfaces,
            ItemKind::Impl(_) => SectionKind::Impl,
            ItemKind::Fn(_) => SectionKind::Functions,
            ItemKind::Test(_) => SectionKind::Tests,
        }
    }

    /// A short description for diagnostics: "`struct`", "import", ...
    pub fn describe(&self) -> &'static str {
        match self {
            ItemKind::Import(_) => "import",
            ItemKind::ExternTarget(_) => "extern block",
            ItemKind::Config(_) => "constant",
            ItemKind::Struct(_) => "`struct`",
            ItemKind::Enum(_) => "`enum`",
            ItemKind::Alias(_) => "type alias",
            ItemKind::IntrinsicType(_) => "`intrinsic type`",
            ItemKind::Interface(_) => "`interface`",
            ItemKind::Impl(_) => "`impl` block",
            ItemKind::Fn(_) => "`fn`",
            ItemKind::Test(_) => "test",
        }
    }
}

/// An import (spec chapter 6, section 2).
#[derive(Clone, Debug, PartialEq)]
pub struct Import {
    /// `pub` import (a re-export).
    pub public: bool,
    /// The module path.
    pub path: Path,
    /// `as name` after a whole-module import.
    pub alias: Option<Ident>,
    /// The `.{A, B as C}` group, if present.
    pub names: Option<Vec<ImportName>>,
}

/// One name in an import group.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportName {
    /// The imported name.
    pub name: Ident,
    /// `as` alias.
    pub alias: Option<Ident>,
}

/// A target block inside `extern`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExternTarget {
    /// `ts`, `go` or `rust` (anything else is reported).
    pub target: Ident,
    /// The declarations.
    pub decls: Vec<ExternDecl>,
}

/// A declaration inside an extern target block.
#[derive(Clone, Debug, PartialEq)]
pub struct ExternDecl {
    /// Doc comments.
    pub docs: Vec<Doc>,
    /// `pub`
    pub public: bool,
    /// Function or type.
    pub kind: ExternDeclKind,
    /// The target path after `from`.
    pub from: StrLit,
    /// The whole declaration.
    pub span: Span,
    /// For a function of a native module (spec chapter 7, section 8.7): the Rust types of its
    /// parameters and result as the reader found them. `None` for written declarations.
    pub native: Option<NativeSig>,
    /// For a record of a native module (spec chapter 8, section 8a): the name in the target code
    /// and the type the reader found of each field, in order. Empty for written declarations.
    pub native_fields: Vec<(String, NativeTy)>,
}

/// The Rust types of a function of a native module, which the generated call must match.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeSig {
    /// One per parameter.
    pub params: Vec<NativeTy>,
    /// The result; `Unit` when there is none.
    pub ret: NativeTy,
}

/// A Rust type of a native function, in the forms of spec chapter 7, section 8.7, rule 3.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeTy {
    /// An integer type, by name: `i32`, `usize`.
    Int(String),
    /// `f32` or `f64`.
    Float(String),
    /// `bool`
    Bool,
    /// `&str`
    Str,
    /// `String`
    String,
    /// `&String`
    RefString,
    /// `&[T]`
    Slice(Box<NativeTy>),
    /// `Vec<T>`
    Vec(Box<NativeTy>),
    /// `&Vec<T>`
    RefVec(Box<NativeTy>),
    /// `HashMap<K, V>` or `BTreeMap<K, V>`, by reference or by value.
    Map {
        /// `HashMap` or `BTreeMap`
        kind: String,
        /// `&`
        by_ref: bool,
        /// The key type.
        key: Box<NativeTy>,
        /// The value type.
        value: Box<NativeTy>,
    },
    /// `Option<T>`
    Option(Box<NativeTy>),
    /// `Result<T, E>`; the error converts with its `Display` text.
    Result(Box<NativeTy>),
    /// `()`, or no result.
    Unit,
    /// A type of the module, by value or by reference.
    Named {
        /// Its name.
        name: String,
        /// `&`
        by_ref: bool,
    },
    /// A callback parameter, passed as the backend passes every callback.
    Callback,
    /// A record of the module (spec chapter 7, section 8.7), by value or by reference.
    Record {
        /// Its name.
        name: String,
        /// `&`
        by_ref: bool,
    },
}

/// What an extern declaration declares.
#[derive(Clone, Debug, PartialEq)]
pub enum ExternDeclKind {
    /// `fn name(params) -> T`
    Fn(FnSig),
    /// `type Name`
    Type(Ident),
    /// `struct Name`: a struct of the module bound to a struct type of the target (0.2).
    Struct(Ident),
}

/// A string literal with its position.
#[derive(Clone, Debug, PartialEq)]
pub struct StrLit {
    /// The value with escapes resolved.
    pub value: String,
    /// Where it is.
    pub span: Span,
}

/// `name: Type = value` in `config`.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigEntry {
    /// `pub`
    pub public: bool,
    /// The name.
    pub name: Ident,
    /// The written type.
    pub ty: Ty,
    /// The initializer.
    pub value: Expr,
}

/// A type parameter with its bounds: `T: Comparable + Copyable`.
#[derive(Clone, Debug, PartialEq)]
pub struct GenericParam {
    /// The name.
    pub name: Ident,
    /// The interfaces after `:`.
    pub bounds: Vec<Ty>,
}

/// `struct Name<T> { fields }`
#[derive(Clone, Debug, PartialEq)]
pub struct StructDecl {
    /// `pub`
    pub public: bool,
    /// The name.
    pub name: Ident,
    /// Type parameters.
    pub generics: Vec<GenericParam>,
    /// The fields.
    pub fields: Vec<Field>,
}

/// A field of a struct or of a variant.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    /// Doc comments.
    pub docs: Vec<Doc>,
    /// `pub` (always `false` in variants).
    pub public: bool,
    /// The name.
    pub name: Ident,
    /// The type.
    pub ty: Ty,
    /// The whole field.
    pub span: Span,
}

/// `enum Name<T> { variants }`
#[derive(Clone, Debug, PartialEq)]
pub struct EnumDecl {
    /// `pub`
    pub public: bool,
    /// The name.
    pub name: Ident,
    /// Type parameters.
    pub generics: Vec<GenericParam>,
    /// The variants.
    pub variants: Vec<Variant>,
}

/// A variant of an enum.
#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    /// Doc comments.
    pub docs: Vec<Doc>,
    /// The name.
    pub name: Ident,
    /// The payload.
    pub payload: VariantPayload,
    /// The whole variant.
    pub span: Span,
}

/// The data a variant carries.
#[derive(Clone, Debug, PartialEq)]
pub enum VariantPayload {
    /// `Full`
    None,
    /// `Io(String)`
    Tuple(Vec<Ty>),
    /// `Moved { x: Int, y: Int }`
    Struct(Vec<Field>),
}

/// `type Name<T> = Type`
#[derive(Clone, Debug, PartialEq)]
pub struct AliasDecl {
    /// `pub`
    pub public: bool,
    /// The name.
    pub name: Ident,
    /// Type parameters.
    pub generics: Vec<GenericParam>,
    /// The aliased type.
    pub ty: Ty,
}

/// `intrinsic type Name<T>`
#[derive(Clone, Debug, PartialEq)]
pub struct IntrinsicTypeDecl {
    /// `pub`
    pub public: bool,
    /// The name.
    pub name: Ident,
    /// Type parameters.
    pub generics: Vec<GenericParam>,
}

/// `interface Name<T> { signatures }`
#[derive(Clone, Debug, PartialEq)]
pub struct InterfaceDecl {
    /// `pub`
    pub public: bool,
    /// The name.
    pub name: Ident,
    /// Type parameters.
    pub generics: Vec<GenericParam>,
    /// The method signatures.
    pub methods: Vec<MethodSig>,
}

/// A signature inside an interface.
#[derive(Clone, Debug, PartialEq)]
pub struct MethodSig {
    /// Doc comments.
    pub docs: Vec<Doc>,
    /// The signature.
    pub sig: FnSig,
}

/// A block in `impl`: `Stack<T: Comparable>: Container<T> { methods }`.
#[derive(Clone, Debug, PartialEq)]
pub struct ImplBlock {
    /// The implemented type.
    pub type_name: Ident,
    /// Type parameters declared in the head.
    pub generics: Vec<GenericParam>,
    /// The interface after `:`, if this is an interface implementation.
    pub interface: Option<Ty>,
    /// The methods, each with its doc comments.
    pub methods: Vec<Method>,
}

/// A method inside an impl block.
#[derive(Clone, Debug, PartialEq)]
pub struct Method {
    /// Doc comments.
    pub docs: Vec<Doc>,
    /// The function.
    pub decl: FnDecl,
    /// The whole method.
    pub span: Span,
}

/// A function or method declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct FnDecl {
    /// `pub`
    pub public: bool,
    /// `intrinsic`
    pub intrinsic: bool,
    /// The signature.
    pub sig: FnSig,
    /// The body; absent for `intrinsic` functions and on some errors.
    pub body: Option<Block>,
}

/// Name, type parameters, parameters and result.
#[derive(Clone, Debug, PartialEq)]
pub struct FnSig {
    /// The name.
    pub name: Ident,
    /// Type parameters.
    pub generics: Vec<GenericParam>,
    /// Parameters, `self` included.
    pub params: Vec<Param>,
    /// The written result type; `None` means `Void`.
    pub ret: Option<Ty>,
    /// From `fn` to the end of the result type.
    pub span: Span,
}

/// How a value is passed (spec chapter 5, section 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// No marker: read access.
    Read,
    /// `mut`
    Mut,
    /// `own`
    Own,
}

/// A parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    /// The mode.
    pub mode: Mode,
    /// `self` or a named parameter.
    pub kind: ParamKind,
    /// The whole parameter.
    pub span: Span,
}

/// The two kinds of parameters.
#[derive(Clone, Debug, PartialEq)]
pub enum ParamKind {
    /// `self`
    SelfParam,
    /// `name: Type`
    Named {
        /// The name.
        name: Ident,
        /// The type.
        ty: Ty,
    },
}

/// `test "name" { body }`
#[derive(Clone, Debug, PartialEq)]
pub struct TestDecl {
    /// The test name.
    pub name: StrLit,
    /// The body.
    pub body: Block,
}

/// A written type.
#[derive(Clone, Debug, PartialEq)]
pub struct Ty {
    /// Node id.
    pub id: NodeId,
    /// What it is.
    pub kind: TyKind,
    /// Where it is.
    pub span: Span,
}

/// The forms of a written type.
#[derive(Clone, Debug, PartialEq)]
pub enum TyKind {
    /// `Int`, `List<User>`, `model.User`.
    Path {
        /// The name.
        path: Path,
        /// Type arguments.
        args: Vec<Ty>,
    },
    /// `fn(Int, mut List<Int>) -> Bool`
    Fn {
        /// Parameter modes and types.
        params: Vec<(Mode, Ty)>,
        /// The result; `None` means `Void`.
        ret: Option<Box<Ty>>,
    },
    /// `Self`
    SelfType,
    /// A type that could not be parsed.
    Error,
}

/// A block of statements.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// The statements. A final expression statement is the value of the block.
    pub stmts: Vec<Stmt>,
    /// From `{` to `}`.
    pub span: Span,
}

/// A statement.
#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    /// What it is.
    pub kind: StmtKind,
    /// Where it is.
    pub span: Span,
}

/// The forms of statements.
#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    /// `let x: T = e`, `var x = e`, `let _ = e`.
    Let {
        /// `var`
        mutable: bool,
        /// The name; `None` for `let _`.
        name: Option<Ident>,
        /// The written type.
        ty: Option<Ty>,
        /// The initializer.
        value: Expr,
    },
    /// `let Point { x, y } = e`, `var Config { name, .. } = e`: a destructuring `let` (0.2).
    LetPattern {
        /// `var`
        mutable: bool,
        /// The pattern; the types check that it always matches.
        pattern: Pat,
        /// The initializer.
        value: Expr,
    },
    /// `place op= value`
    Assign {
        /// The place.
        target: Expr,
        /// `=`, `+=`, ...
        op: AssignOp,
        /// The value.
        value: Expr,
    },
    /// `label: while cond { body }`
    While {
        /// The label of the loop.
        label: Option<Ident>,
        /// The condition.
        cond: Expr,
        /// The body.
        body: Block,
    },
    /// `label: while let pattern = mode? value { body }` (0.2).
    WhileLet {
        /// The label of the loop.
        label: Option<Ident>,
        /// The pattern.
        pattern: Pat,
        /// `mut`, `own` or read.
        mode: Mode,
        /// The value, evaluated before every iteration.
        value: Expr,
        /// The body.
        body: Block,
    },
    /// `label: for x in mode? collection { body }` or `for i in a..b { body }`.
    For {
        /// The label of the loop.
        label: Option<Ident>,
        /// The loop variable.
        var: Ident,
        /// What is iterated.
        iter: ForIter,
        /// The body.
        body: Block,
    },
    /// An expression used as a statement.
    Expr(Expr),
}

/// What a `for` loop walks over.
#[derive(Clone, Debug, PartialEq)]
pub enum ForIter {
    /// A collection with an optional mode.
    Collection {
        /// `mut`, `own` or read.
        mode: Mode,
        /// The collection.
        expr: Expr,
    },
    /// `start..end` or `start..=end`.
    Range {
        /// The first value.
        start: Expr,
        /// The bound.
        end: Expr,
        /// `..=`
        inclusive: bool,
    },
}

/// Assignment operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum AssignOp {
    Assign,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

/// An expression.
#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    /// Node id.
    pub id: NodeId,
    /// What it is.
    pub kind: ExprKind,
    /// Where it is.
    pub span: Span,
}

/// The forms of expressions.
#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    /// An integer literal.
    Int(u64),
    /// A float literal.
    Float(f64),
    /// A string literal.
    Str(String),
    /// An interpolated string `f"..."` (0.2).
    FStr(Vec<FStrPart>),
    /// `true` or `false`.
    Bool(bool),
    /// `()`
    Unit,
    /// A name.
    Name(Ident),
    /// `self`
    SelfValue,
    /// `Self`
    SelfType,
    /// `(e)`
    Paren(Box<Expr>),
    /// `[a, b]`
    List(Vec<Expr>),
    /// `["a": 1]`, `[:]`
    Map(Vec<(Expr, Expr)>),
    /// `Path.<T> { field: value, shorthand, ..base }`
    StructLit {
        /// The type name.
        path: Path,
        /// Explicit type arguments `.<T>`.
        type_args: Vec<Ty>,
        /// The fields.
        fields: Vec<FieldInit>,
        /// The base of the update syntax `..base` (0.2).
        base: Option<Box<Expr>>,
    },
    /// `base.name`
    Field {
        /// The left side.
        base: Box<Expr>,
        /// The name after the dot.
        name: Ident,
    },
    /// `base.<T, U>`
    TypeArgs {
        /// The left side.
        base: Box<Expr>,
        /// The type arguments.
        args: Vec<Ty>,
    },
    /// `callee(args)`
    Call {
        /// What is called.
        callee: Box<Expr>,
        /// The arguments.
        args: Vec<Arg>,
    },
    /// `base[index]`
    Index {
        /// The collection.
        base: Box<Expr>,
        /// The index.
        index: Box<Expr>,
    },
    /// `base[a..b]` and the other slice forms.
    Slice {
        /// The collection.
        base: Box<Expr>,
        /// The start, if written.
        start: Option<Box<Expr>>,
        /// The end, if written.
        end: Option<Box<Expr>>,
        /// `..=`
        inclusive: bool,
    },
    /// `e?`
    Try(Box<Expr>),
    /// `-e`, `!e`, `~e`
    Unary {
        /// The operator.
        op: UnaryOp,
        /// The operand.
        operand: Box<Expr>,
    },
    /// `a op b`
    Binary {
        /// The operator.
        op: BinaryOp,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
    },
    /// `|x, y: Int| body` or `|x| -> T { body }`.
    Closure {
        /// The parameters.
        params: Vec<ClosureParam>,
        /// The written result type.
        ret: Option<Ty>,
        /// The body.
        body: Box<Expr>,
    },
    /// `{ ... }` as the body of a closure or a match arm.
    Block(Block),
    /// `if cond { } else ...`
    If {
        /// The condition.
        cond: Box<Expr>,
        /// The first branch.
        then: Block,
        /// `else` followed by a block or another `if`.
        otherwise: Option<Box<Expr>>,
    },
    /// `if let pattern = mode? value { } else ...` (0.2).
    IfLet {
        /// The pattern.
        pattern: Pat,
        /// The mode marker of the value.
        mode: Mode,
        /// The matched value.
        value: Box<Expr>,
        /// The branch taken when the value matches.
        then: Block,
        /// `else` followed by a block or another `if`.
        otherwise: Option<Box<Expr>>,
    },
    /// `label: loop { body }` (0.2).
    Loop {
        /// The label of the loop.
        label: Option<Ident>,
        /// The body.
        body: Block,
    },
    /// `match mode? scrutinee { arms }`
    Match {
        /// The mode marker.
        mode: Mode,
        /// The matched value.
        scrutinee: Box<Expr>,
        /// The arms.
        arms: Vec<Arm>,
    },
    /// `return e?`
    Return(Option<Box<Expr>>),
    /// `break`, `break label`, `break value`. The parser takes a name after `break` as a label
    /// when an enclosing loop has that label.
    Break {
        /// The label of the loop that is left.
        label: Option<Ident>,
        /// The value of the `loop` that is left.
        value: Option<Box<Expr>>,
    },
    /// `continue`, `continue label`.
    Continue {
        /// The label of the loop that is continued.
        label: Option<Ident>,
    },
    /// Something that could not be parsed; a diagnostic was reported.
    Error,
}

/// A part of an interpolated string.
#[derive(Clone, Debug, PartialEq)]
pub enum FStrPart {
    /// Text.
    Text(String),
    /// An interpolated expression.
    Expr(Expr),
}

/// A field in a struct literal.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldInit {
    /// Node id, for what a shorthand field names.
    pub id: NodeId,
    /// The field name.
    pub name: Ident,
    /// The value; `None` for the shorthand `User { id }`.
    pub value: Option<Expr>,
}

/// An argument of a call.
#[derive(Clone, Debug, PartialEq)]
pub struct Arg {
    /// `mut`, `own` or none.
    pub mode: Mode,
    /// The value.
    pub value: Expr,
}

/// A closure parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct ClosureParam {
    /// The name.
    pub name: Ident,
    /// The written type.
    pub ty: Option<Ty>,
}

/// An arm of a `match`.
#[derive(Clone, Debug, PartialEq)]
pub struct Arm {
    /// The pattern.
    pub pattern: Pat,
    /// `if` guard.
    pub guard: Option<Expr>,
    /// The result.
    pub body: Expr,
    /// The whole arm.
    pub span: Span,
}

/// Unary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    /// `-`
    Neg,
    /// `!`
    Not,
    /// `~`
    BitNot,
}

/// Binary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum BinaryOp {
    Mul,
    Div,
    Rem,
    Add,
    Sub,
    Shl,
    Shr,
    BitAnd,
    BitXor,
    BitOr,
    Eq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinaryOp {
    /// The operator as written.
    pub fn symbol(self) -> &'static str {
        match self {
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Rem => "%",
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Shl => "<<",
            BinaryOp::Shr => ">>",
            BinaryOp::BitAnd => "&",
            BinaryOp::BitXor => "^",
            BinaryOp::BitOr => "|",
            BinaryOp::Eq => "==",
            BinaryOp::NotEq => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::And => "&&",
            BinaryOp::Or => "||",
        }
    }

    /// Whether this is one of the non-associative comparison operators.
    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::Le
                | BinaryOp::Gt
                | BinaryOp::Ge
        )
    }
}

/// A pattern in a `match` arm.
#[derive(Clone, Debug, PartialEq)]
pub struct Pat {
    /// Node id.
    pub id: NodeId,
    /// What it is.
    pub kind: PatKind,
    /// Where it is.
    pub span: Span,
}

/// The forms of patterns (spec chapter 3, section 7).
#[derive(Clone, Debug, PartialEq)]
pub enum PatKind {
    /// `_`
    Wild,
    /// A lone name: a binding, or a payload-less variant or constant (decided later).
    Name(Ident),
    /// An integer, possibly negative.
    Int {
        /// `-`
        negative: bool,
        /// The magnitude.
        value: u64,
    },
    /// A string.
    Str(String),
    /// `true` or `false`.
    Bool(bool),
    /// A dotted path without payload: `StorageError.Full`.
    Path(Path),
    /// `Some(x)`, `Shape.Circle(r)`.
    Tuple {
        /// The variant.
        path: Path,
        /// The payload patterns.
        items: Vec<Pat>,
    },
    /// `Point { x, y: 0, .. }`.
    Struct {
        /// The struct or variant.
        path: Path,
        /// The field patterns.
        fields: Vec<FieldPat>,
        /// `..` at the end.
        rest: bool,
    },
    /// `A | B`
    Or(Vec<Pat>),
    /// A pattern that could not be parsed.
    Error,
}

/// A field in a struct pattern.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldPat {
    /// Node id, for the binding of a shorthand field.
    pub id: NodeId,
    /// The field name.
    pub name: Ident,
    /// The sub-pattern; `None` for the shorthand `{ x }`.
    pub pat: Option<Pat>,
}
