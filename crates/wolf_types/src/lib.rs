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
mod supers;
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

use std::collections::BTreeSet;

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
    /// The methods of each interface that have a default (chapter 4, section 8.2, rule 4).
    pub interface_defaults: BTreeMap<DefId, BTreeSet<String>>,
    /// The super-interfaces of each interface, in terms of its type parameters (chapter 4,
    /// section 8.2, rule 6).
    pub interface_supers: BTreeMap<DefId, Vec<Ty>>,
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
    /// Calls of a default by the interface's name, `Named.greeting(x)` (the `Call` node): the
    /// interface and the method.
    pub default_calls: BTreeMap<(FileId, NodeId), (DefId, String)>,
    /// Expressions whose value is converted to an interface type, and that type: a concrete
    /// value used where an interface is expected (chapter 4, 8.5). Shorthand fields of struct
    /// literals are keyed by their field node.
    pub coercions: BTreeMap<(FileId, NodeId), Ty>,
    /// `?` expressions whose error is converted to an interface type, and that type.
    pub try_coercions: BTreeMap<(FileId, NodeId), Ty>,
    /// Struct literals and struct patterns written with an alias (`P { x: 1 }` for
    /// `type P = Point`), and the struct the alias stands for (chapter 4, section 7.3).
    pub alias_structs: BTreeMap<(FileId, NodeId), DefId>,
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

/// A type as it is written in source (`List<Int>`, `fn(Int) -> String`), for tools such as the
/// language server.
pub fn show(ty: &Ty, resolved: &Resolved, typed: &Typed) -> String {
    let name = |def: &DefId| {
        resolved
            .def(*def)
            .map(|d| d.name.clone())
            .unwrap_or_default()
    };
    let list = |types: &[Ty]| {
        types
            .iter()
            .map(|t| show(t, resolved, typed))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match ty {
        Ty::Named(def, args) if args.is_empty() => name(def),
        Ty::Named(def, args) => format!("{}<{}>", name(def), list(args)),
        Ty::Fn(params, ret) => {
            let params: Vec<String> = params
                .iter()
                .map(|(mode, t)| format!("{}{}", env::mode_prefix(*mode), show(t, resolved, typed)))
                .collect();
            match &**ret {
                Ty::Named(d, _) if name(d) == "Void" => format!("fn({})", params.join(", ")),
                ret => format!(
                    "fn({}) -> {}",
                    params.join(", "),
                    show(ret, resolved, typed)
                ),
            }
        }
        Ty::Param(ParamId::Decl(span)) => typed
            .decls
            .param_names
            .get(span)
            .cloned()
            .unwrap_or_else(|| "T".to_string()),
        Ty::Param(ParamId::SelfType) => "Self".to_string(),
        Ty::Var(_) | Ty::Error => "_".to_string(),
        Ty::Never => "Never".to_string(),
    }
}

/// The signature of a function as it is written in source: `fn area(s: Shape) -> Float`, with
/// the names of its parameters when they are given.
pub fn show_fn(
    name: &str,
    sig: &Sig,
    params: &[String],
    resolved: &Resolved,
    typed: &Typed,
) -> String {
    let mut parts = Vec::new();
    if let Some(mode) = sig.self_mode {
        parts.push(format!("{}self", env::mode_prefix(mode)));
    }
    for (i, (mode, t)) in sig.params.iter().enumerate() {
        let ty = show(t, resolved, typed);
        match params.get(i) {
            Some(p) => parts.push(format!("{}{p}: {ty}", env::mode_prefix(*mode))),
            None => parts.push(format!("{}{ty}", env::mode_prefix(*mode))),
        }
    }
    let generics = if sig.generics.is_empty() {
        String::new()
    } else {
        let names: Vec<String> = sig
            .generics
            .iter()
            .map(|g| {
                typed
                    .decls
                    .param_names
                    .get(&g.span)
                    .cloned()
                    .unwrap_or_default()
            })
            .collect();
        format!("<{}>", names.join(", "))
    };
    let ret = match &sig.ret {
        Ty::Named(d, _) if resolved.def(*d).is_some_and(|d| d.name == "Void") => String::new(),
        ret => format!(" -> {}", show(ret, resolved, typed)),
    };
    format!("fn {name}{generics}({}){ret}", parts.join(", "))
}
