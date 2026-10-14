//! Type checking (spec chapter 4).
//!
//! The checker runs after name resolution and works on its table of what every name means. It
//! has three parts:
//!
//! 1. **Declarations** (`env`): the types of fields, variants, functions, methods and constants
//!    are built from the written types, with aliases expanded. Errors in declarations themselves
//!    (wrong number of type arguments, cycles of aliases, `impl` outside the module of its type,
//!    infinitely sized structs) are found here.
//! 2. **Bodies** (`body`, `pat`): every function body, test and constant initializer is checked
//!    on its own, with type inference local to the body (chapter 4, section 10).
//! 3. **Constants** (`consts`): `config` initializers are evaluated, with the overrides of
//!    `wolfpack.toml` applied.
//!
//! Inference uses an inference table: an unknown type is a variable, and the checker binds it when
//! it learns more (unification). An expression that already has an error gets [`Ty::Error`],
//! which agrees with every type, so one mistake is reported once.

mod body;
mod consts;
mod derive;
mod env;
mod externs;
mod pat;
mod ty;

use std::collections::BTreeMap;

use wolf_ast::NodeId;
use wolf_diag::Diagnostic;
use wolf_project::ConfigValue;
use wolf_resolve::{DefId, LocalId, ModuleInput, ProjectInfo, Resolved};
use wolf_span::FileId;

pub use env::{FieldInfo, Generic, ImplOf, Method, Shape, Sig, VariantInfo};
pub use ty::{ParamId, Ty, VarId};

/// The function a method call resolved to.
#[derive(Clone, Debug, PartialEq)]
pub struct MethodCall {
    /// The type whose function it is, or the interface for a call on an interface value.
    pub owner: DefId,
    /// The function name.
    pub name: String,
    /// The interface, if the function comes from an interface implementation or an interface.
    pub interface: Option<DefId>,
}

/// The value of a `config` constant.
#[derive(Clone, Debug, PartialEq)]
pub enum ConstValue {
    /// An `Int`.
    Int(i64),
    /// A `Float`.
    Float(f64),
    /// A `Bool`.
    Bool(bool),
    /// A `String`.
    Str(String),
}

impl From<ConfigValue> for ConstValue {
    fn from(value: ConfigValue) -> ConstValue {
        match value {
            ConfigValue::Int(v) => ConstValue::Int(v),
            ConfigValue::Float(v) => ConstValue::Float(v),
            ConfigValue::Bool(v) => ConstValue::Bool(v),
            ConfigValue::Str(v) => ConstValue::Str(v),
        }
    }
}

/// The types of all declarations, for the lowering to IR.
#[derive(Debug, Default)]
pub struct Decls {
    /// The type parameters of every type declaration.
    pub type_generics: BTreeMap<DefId, Vec<Generic>>,
    /// The fields of every struct.
    pub fields: BTreeMap<DefId, Vec<FieldInfo>>,
    /// Every enum variant.
    pub variants: BTreeMap<DefId, VariantInfo>,
    /// The variants of every enum, in declaration order.
    pub enum_variants: BTreeMap<DefId, Vec<DefId>>,
    /// Free functions, extern functions and inherent functions by declaration.
    pub fns: BTreeMap<DefId, Sig>,
    /// Every function of every `impl` block.
    pub methods: Vec<Method>,
    /// The method signatures of each interface.
    pub interface_methods: BTreeMap<DefId, BTreeMap<String, Sig>>,
    /// The interfaces implemented by each type.
    pub impls: BTreeMap<DefId, Vec<ImplOf>>,
    /// The type of each constant.
    pub consts: BTreeMap<DefId, Ty>,
    /// The names of type parameters by the span of their declaration.
    pub param_names: BTreeMap<wolf_span::Span, String>,
}

/// The result of type checking. Tables are keyed like the resolution table.
#[derive(Debug, Default)]
pub struct Typed {
    /// The type of each checked expression.
    pub expr_types: BTreeMap<(FileId, NodeId), Ty>,
    /// The type of each local variable, parameter and binding of a checked body.
    pub local_types: BTreeMap<LocalId, Ty>,
    /// The function each method call (the `Call` node) resolved to.
    pub method_calls: BTreeMap<(FileId, NodeId), MethodCall>,
    /// Expressions whose value is converted to an interface type, and that type: a concrete
    /// value used where an interface is expected (chapter 4, 8.5). Shorthand fields of struct
    /// literals are keyed by their field node.
    pub coercions: BTreeMap<(FileId, NodeId), Ty>,
    /// `?` expressions whose error is converted to an interface type, and that type.
    pub try_coercions: BTreeMap<(FileId, NodeId), Ty>,
    /// The values of `config` constants, with overrides applied.
    pub consts: BTreeMap<DefId, ConstValue>,
    /// The types of all declarations.
    pub decls: Decls,
    /// Diagnostics of chapter 4.
    pub diagnostics: Vec<Diagnostic>,
}

/// Checks the types of a resolved project. `modules` are the same modules, in the same order,
/// that were given to [`wolf_resolve::resolve`].
pub fn check(modules: &[ModuleInput<'_>], resolved: &Resolved, project: &ProjectInfo<'_>) -> Typed {
    let mut checker = env::Checker::new(modules, resolved, project);
    let bodies = checker.collect();
    checker.check_bodies(bodies);
    checker.eval_consts();
    checker.finish()
}
