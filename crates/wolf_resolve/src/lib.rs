//! Name resolution (spec chapter 6): modules, imports, visibility, scopes.
//!
//! Resolution runs in two passes over all modules of a project, together with the std modules:
//!
//! 1. **Collect**: the top-level names of every module, the members of types (enum variants and
//!    the functions of inherent `impl` blocks), then the imports and the import graph.
//! 2. **Resolve**: every name in types, expressions and patterns, with local scopes.
//!
//! Section order is layout, not visibility (chapter 2, rule 2.6), which is why the first pass
//! sees the whole project before anything is resolved. The result is a table from syntax nodes to
//! what they name, for the type checker (M4), plus the diagnostics of chapter 6.

mod body;
mod checks;
mod collect;
mod names;

use std::collections::BTreeMap;

use wolf_ast::{File, NodeId};
use wolf_diag::Diagnostic;
use wolf_project::{ConfigOverride, Kind};
use wolf_span::{FileId, Span};

/// What a module is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// The prelude module of std; never imported.
    Prelude,
    /// Another std module.
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

/// One parsed module given to the resolver.
pub struct ModuleInput<'a> {
    /// The module path derived from the file path.
    pub path: Vec<String>,
    /// What the module is for.
    pub role: Role,
    /// The source file.
    pub file: FileId,
    /// The syntax tree.
    pub ast: &'a File,
}

/// Facts about the project that resolution needs.
pub struct ProjectInfo<'a> {
    /// `cli` or `lib`.
    pub kind: Kind,
    /// Overrides from `[config.*]`, checked against the constants (E0613).
    pub config: &'a [ConfigOverride],
}

/// Identifies a module.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModuleId(pub u32);

/// Identifies a declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefId(pub u32);

/// Identifies a local variable, parameter or pattern binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalId(pub u32);

/// The kinds of declarations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefKind {
    /// `struct`
    Struct,
    /// `enum`
    Enum,
    /// `type A = B`
    Alias,
    /// `intrinsic type`
    IntrinsicType,
    /// `interface`
    Interface,
    /// A free function.
    Fn,
    /// A `config` constant.
    Config,
    /// A function from `extern`.
    ExternFn,
    /// An opaque type from `extern`.
    ExternType,
    /// A variant of an enum.
    Variant {
        /// The enum.
        parent: DefId,
        /// The payload form.
        payload: Payload,
    },
    /// A function in an inherent `impl` block (a method if it takes `self`).
    Member {
        /// The type the block is for.
        owner: DefId,
        /// Whether it takes `self`.
        method: bool,
    },
}

/// The payload form of a variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Payload {
    /// No payload.
    None,
    /// Positional payload.
    Tuple,
    /// Named fields.
    Struct,
}

impl DefKind {
    /// Whether the declaration names a type.
    pub fn is_type(self) -> bool {
        matches!(
            self,
            DefKind::Struct
                | DefKind::Enum
                | DefKind::Alias
                | DefKind::IntrinsicType
                | DefKind::Interface
                | DefKind::ExternType
        )
    }

    fn describe(self) -> &'static str {
        match self {
            DefKind::Struct => "struct",
            DefKind::Enum => "enum",
            DefKind::Alias => "type alias",
            DefKind::IntrinsicType => "built-in type",
            DefKind::Interface => "interface",
            DefKind::Fn => "function",
            DefKind::Config => "constant",
            DefKind::ExternFn => "extern function",
            DefKind::ExternType => "extern type",
            DefKind::Variant { .. } => "variant",
            DefKind::Member { method: true, .. } => "method",
            DefKind::Member { method: false, .. } => "associated function",
        }
    }
}

/// A declaration.
#[derive(Clone, Debug)]
pub struct Def {
    /// What it is.
    pub kind: DefKind,
    /// Its name.
    pub name: String,
    /// The module that declares it.
    pub module: ModuleId,
    /// `pub` (variants have the visibility of their enum).
    pub public: bool,
    /// The span of the name in the declaration.
    pub span: Span,
}

/// What a name in the source refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Res {
    /// A declaration.
    Def(DefId),
    /// A module (through an import).
    Module(ModuleId),
    /// A local variable, parameter or binding.
    Local(LocalId),
    /// A type parameter, identified by the span of its declaration.
    TypeParam(Span),
    /// `Self` inside an `impl` block or an interface.
    SelfType,
}

/// A local variable.
#[derive(Clone, Debug)]
pub struct Local {
    /// Its name.
    pub name: String,
    /// Where it is declared.
    pub span: Span,
}

/// Facts about a module after resolution.
#[derive(Clone, Debug)]
pub struct ModuleInfo {
    /// The module path.
    pub path: Vec<String>,
    /// What the module is for.
    pub role: Role,
    /// The source file.
    pub file: FileId,
    /// Top-level names declared in the module.
    pub items: BTreeMap<String, DefId>,
}

/// The result of name resolution.
#[derive(Debug, Default)]
pub struct Resolved {
    /// All modules, std included.
    pub modules: Vec<ModuleInfo>,
    /// All declarations.
    pub defs: Vec<Def>,
    /// All locals.
    pub locals: Vec<Local>,
    /// What each resolved node names: `Name` and `Field` expressions, types, patterns, struct
    /// literals and the shorthand fields of struct literals and patterns.
    pub res: BTreeMap<(FileId, NodeId), Res>,
    /// The local declared at each name span (`let`, `var`, parameters, loop variables, closure
    /// parameters, pattern bindings).
    pub local_decls: BTreeMap<Span, LocalId>,
    /// Members (variants and inherent functions) of each type.
    pub members: BTreeMap<DefId, BTreeMap<String, DefId>>,
    /// Diagnostics of chapter 6.
    pub diagnostics: Vec<Diagnostic>,
}

impl Resolved {
    /// The declaration with this id.
    pub fn def(&self, id: DefId) -> Option<&Def> {
        self.defs.get(id.0 as usize)
    }
}

/// Resolves a project. `modules` must contain the std modules (`wolf_std::MODULES`, parsed by
/// the caller, with roles [`Role::Prelude`] and [`Role::Std`]) and every module of the project.
pub fn resolve(modules: &[ModuleInput<'_>], project: &ProjectInfo<'_>) -> Resolved {
    let mut resolver = collect::Resolver::new(modules, project);
    resolver.collect();
    resolver.resolve_bodies();
    resolver.finish()
}

pub use names::{is_pascal_case, is_snake_case};
