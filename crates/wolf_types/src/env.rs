//! Declarations: the built-in types, the types of declared items, and written types turned into
//! [`Ty`].

use std::collections::{BTreeMap, BTreeSet};

use wolf_ast::{self as ast, ItemKind, Mode, ParamKind, TyKind, VariantPayload};
use wolf_diag::{Diagnostic, codes};
use wolf_resolve::{DefId, DefKind, ModuleId, ModuleInput, ProjectInfo, Res, Resolved};
use wolf_span::{FileId, Span};

use crate::consts::{ConstInit, ConstState};
use crate::ty::{Infer, ParamId, Ty};
use crate::{ConstValue, Typed};

/// The declarations of std that the rules of chapter 4 refer to.
#[derive(Default)]
pub(crate) struct Builtins {
    pub(crate) int: Option<DefId>,
    pub(crate) float: Option<DefId>,
    pub(crate) bool: Option<DefId>,
    pub(crate) string: Option<DefId>,
    pub(crate) void: Option<DefId>,
    pub(crate) never: Option<DefId>,
    pub(crate) list: Option<DefId>,
    pub(crate) map: Option<DefId>,
    pub(crate) option: Option<DefId>,
    pub(crate) result: Option<DefId>,
    pub(crate) comparable: Option<DefId>,
    pub(crate) equatable: Option<DefId>,
    pub(crate) copyable: Option<DefId>,
    pub(crate) hashable: Option<DefId>,
    pub(crate) error: Option<DefId>,
    pub(crate) display: Option<DefId>,
}

impl Builtins {
    fn find(resolved: &Resolved) -> Builtins {
        let get = |module: &str, name: &str| {
            resolved
                .modules
                .iter()
                .find(|m| m.path.join(".") == module)
                .and_then(|m| m.items.get(name).copied())
        };
        Builtins {
            int: get("std.math", "Int"),
            float: get("std.math", "Float"),
            bool: get("std.prelude", "Bool"),
            string: get("std.string", "String"),
            void: get("std.prelude", "Void"),
            never: get("std.prelude", "Never"),
            list: get("std.list", "List"),
            map: get("std.map", "Map"),
            option: get("std.prelude", "Option"),
            result: get("std.prelude", "Result"),
            comparable: get("std.prelude", "Comparable"),
            equatable: get("std.prelude", "Equatable"),
            copyable: get("std.prelude", "Copyable"),
            hashable: get("std.prelude", "Hashable"),
            error: get("std.prelude", "Error"),
            display: get("std.prelude", "Display"),
        }
    }
}

/// A type parameter of a declaration.
#[derive(Clone, Debug)]
pub struct Generic {
    /// The span of its name, which identifies it.
    pub span: Span,
    /// Its bounds.
    pub bounds: Vec<Ty>,
}

/// A field of a struct or of a struct variant.
#[derive(Clone, Debug)]
pub struct FieldInfo {
    /// The field name.
    pub name: String,
    /// The span of the name.
    pub span: Span,
    /// The field type.
    pub ty: Ty,
    /// `pub`.
    pub public: bool,
}

/// The payload of a variant.
#[derive(Clone, Debug)]
pub enum Shape {
    /// No payload.
    None,
    /// Positional values.
    Tuple(Vec<Ty>),
    /// Named fields.
    Struct(Vec<FieldInfo>),
}

/// A variant of an enum.
#[derive(Clone, Debug)]
pub struct VariantInfo {
    /// The enum.
    pub parent: DefId,
    /// The payload.
    pub shape: Shape,
}

/// A function signature.
#[derive(Clone, Debug)]
pub struct Sig {
    /// The type parameters of the function itself.
    pub generics: Vec<Generic>,
    /// The mode of `self`, for methods.
    pub self_mode: Option<Mode>,
    /// Parameter modes and types, without `self`.
    pub params: Vec<(Mode, Ty)>,
    /// The result type.
    pub ret: Ty,
}

/// A function of an `impl` block.
#[derive(Clone, Debug)]
pub struct Method {
    /// The function name.
    pub name: String,
    /// The span of the name in its declaration.
    pub span: Span,
    /// The type of the `impl` block.
    pub owner: DefId,
    /// The module of the block.
    pub module: ModuleId,
    /// The type parameters of the `impl` block, in the order of the type's parameters.
    pub impl_generics: Vec<Span>,
    /// The bounds of the `impl` block's type parameters, in the same order.
    pub impl_bounds: Vec<Vec<Ty>>,
    /// The bounds that the method's `where` adds to them, in the same order (chapter 4, 8.6).
    pub where_bounds: Vec<Vec<Ty>>,
    /// The signature.
    pub sig: Sig,
    /// Visible outside its module.
    pub public: bool,
    /// The interface, for a function of an interface implementation.
    pub interface: Option<DefId>,
}

/// An implementation of an interface by a type.
#[derive(Clone, Debug)]
pub struct ImplOf {
    /// The interface.
    pub interface: DefId,
    /// The interface's type arguments, in terms of the impl block's type parameters.
    pub args: Vec<Ty>,
    /// The impl block's type parameters, in the order of the type's parameters.
    pub generics: Vec<Span>,
}

/// A body to check.
#[derive(Clone)]
pub(crate) enum BodyRef<'a> {
    Fn {
        module: usize,
        decl: &'a ast::FnDecl,
        /// The type of `Self` and `self`, in an `impl` block.
        self_ty: Option<Ty>,
        /// Index into [`Checker::methods`], for methods.
        method: Option<usize>,
    },
    Test {
        module: usize,
        test: &'a ast::TestDecl,
    },
    Config {
        module: usize,
        def: DefId,
        entry: &'a ast::ConfigEntry,
    },
}

enum AliasState {
    InProgress,
    Done(Ty),
}

/// Where a written type is: the file, and what `Self` means there.
pub(crate) struct Scope {
    pub(crate) file: FileId,
    pub(crate) self_ty: Option<Ty>,
}

pub(crate) struct Checker<'a> {
    pub(crate) inputs: &'a [ModuleInput<'a>],
    pub(crate) resolved: &'a Resolved,
    pub(crate) project: &'a ProjectInfo<'a>,
    pub(crate) b: Builtins,
    /// Every declaration by the span of its name.
    pub(crate) def_at: BTreeMap<Span, DefId>,
    /// The type parameters of every type declaration.
    pub(crate) type_generics: BTreeMap<DefId, Vec<Generic>>,
    alias_decls: BTreeMap<DefId, (usize, &'a ast::AliasDecl)>,
    alias_state: BTreeMap<DefId, AliasState>,
    alias_stack: Vec<DefId>,
    /// The fields of every struct.
    pub(crate) fields: BTreeMap<DefId, Vec<FieldInfo>>,
    /// The structs bound to a struct type of the target in the extern block being checked
    /// (chapter 7, section 8.4a).
    pub(crate) extern_records: BTreeSet<DefId>,
    /// The fields of records already reported as not crossing (E0702), once for all blocks.
    pub(crate) reported_fields: BTreeSet<(DefId, String)>,
    pub(crate) variants: BTreeMap<DefId, VariantInfo>,
    /// The variants of every enum, in declaration order.
    pub(crate) enum_variants: BTreeMap<DefId, Vec<DefId>>,
    /// Free functions, extern functions and the functions of inherent `impl` blocks.
    pub(crate) fns: BTreeMap<DefId, Sig>,
    pub(crate) methods: Vec<Method>,
    /// The functions of each type, inherent and from interface implementations, by name.
    pub(crate) method_table: BTreeMap<DefId, BTreeMap<String, usize>>,
    /// The [`Method`] of each inherent function declaration.
    pub(crate) member_method: BTreeMap<DefId, usize>,
    /// The method signatures of each interface.
    pub(crate) interface_methods: BTreeMap<DefId, BTreeMap<String, Sig>>,
    /// The interfaces implemented by each type.
    pub(crate) impls: BTreeMap<DefId, Vec<ImplOf>>,
    /// The type of each constant.
    pub(crate) consts: BTreeMap<DefId, Ty>,
    /// The names of type parameters, for messages.
    pub(crate) param_names: BTreeMap<Span, String>,
    pub(crate) infer: Infer,
    pub(crate) out: Typed,
    /// Constant initializers that type-checked, for evaluation.
    pub(crate) const_inits: BTreeMap<DefId, ConstInit<'a>>,
    pub(crate) const_state: BTreeMap<DefId, ConstState>,
    /// Constants that cannot be evaluated because of an earlier error.
    pub(crate) const_failed: BTreeSet<DefId>,
    /// The written type of each constant.
    pub(crate) const_written: BTreeMap<DefId, (ast::Ty, FileId)>,
    /// Values from `[config]` in `wolfpack.toml`.
    pub(crate) overrides: BTreeMap<DefId, ConstValue>,
    /// Bounds of type arguments in written types, waiting for [`Checker::check_pending_bounds`].
    pub(crate) pending_bounds: Vec<(Ty, DefId, Span)>,
    /// The bounds of every type parameter, by the span of its name.
    pub(crate) param_bounds: BTreeMap<Span, Vec<Ty>>,
    /// Set while a bound or the interface of an implementation is lowered: there an interface
    /// is not used as a value type.
    in_bound: bool,
    /// Interfaces used as value types, checked for object safety after collection.
    pending_object: Vec<(DefId, Span)>,
}

impl<'a> Checker<'a> {
    pub(crate) fn new(
        inputs: &'a [ModuleInput<'a>],
        resolved: &'a Resolved,
        project: &'a ProjectInfo<'a>,
    ) -> Checker<'a> {
        let def_at = resolved
            .defs
            .iter()
            .enumerate()
            .map(|(i, d)| (d.span, DefId(u32::try_from(i).unwrap_or(u32::MAX))))
            .collect();
        Checker {
            inputs,
            resolved,
            project,
            b: Builtins::find(resolved),
            def_at,
            type_generics: BTreeMap::new(),
            alias_decls: BTreeMap::new(),
            alias_state: BTreeMap::new(),
            alias_stack: Vec::new(),
            fields: BTreeMap::new(),
            extern_records: BTreeSet::new(),
            reported_fields: BTreeSet::new(),
            variants: BTreeMap::new(),
            enum_variants: BTreeMap::new(),
            fns: BTreeMap::new(),
            methods: Vec::new(),
            method_table: BTreeMap::new(),
            member_method: BTreeMap::new(),
            interface_methods: BTreeMap::new(),
            impls: BTreeMap::new(),
            consts: BTreeMap::new(),
            param_names: BTreeMap::new(),
            infer: Infer::default(),
            out: Typed::default(),
            const_inits: BTreeMap::new(),
            const_state: BTreeMap::new(),
            const_failed: BTreeSet::new(),
            const_written: BTreeMap::new(),
            overrides: BTreeMap::new(),
            pending_bounds: Vec::new(),
            param_bounds: BTreeMap::new(),
            in_bound: false,
            pending_object: Vec::new(),
        }
    }

    pub(crate) fn finish(mut self) -> Typed {
        self.out.decls = crate::Decls {
            type_generics: self.type_generics,
            fields: self.fields,
            variants: self.variants,
            enum_variants: self.enum_variants,
            fns: self.fns,
            methods: self.methods,
            interface_methods: self.interface_methods,
            impls: self.impls,
            consts: self.consts,
            param_names: self.param_names,
        };
        self.out
    }

    pub(crate) fn report(&mut self, diagnostic: Diagnostic) {
        self.out.diagnostics.push(diagnostic);
    }

    pub(crate) fn file_of(&self, module: usize) -> FileId {
        self.inputs
            .get(module)
            .map(|m| m.file)
            .unwrap_or(FileId::ZERO)
    }

    // ----- simple types -----------------------------------------------------------------------

    pub(crate) fn plain(def: Option<DefId>) -> Ty {
        def.map_or(Ty::Error, |d| Ty::Named(d, Vec::new()))
    }

    pub(crate) fn int(&self) -> Ty {
        Checker::plain(self.b.int)
    }

    pub(crate) fn float(&self) -> Ty {
        Checker::plain(self.b.float)
    }

    pub(crate) fn bool(&self) -> Ty {
        Checker::plain(self.b.bool)
    }

    pub(crate) fn string(&self) -> Ty {
        Checker::plain(self.b.string)
    }

    pub(crate) fn void(&self) -> Ty {
        Checker::plain(self.b.void)
    }

    pub(crate) fn list_of(&self, item: Ty) -> Ty {
        self.b.list.map_or(Ty::Error, |d| Ty::Named(d, vec![item]))
    }

    /// Whether the type is the named type `def` (after following variables).
    pub(crate) fn is(&self, ty: &Ty, def: Option<DefId>) -> bool {
        match self.infer.shallow(ty) {
            Ty::Named(d, _) => Some(d) == def,
            _ => false,
        }
    }

    /// The type arguments if the type is the named type `def`.
    pub(crate) fn args_of(&self, ty: &Ty, def: Option<DefId>) -> Option<Vec<Ty>> {
        match self.infer.shallow(ty) {
            Ty::Named(d, args) if Some(d) == def => Some(args),
            _ => None,
        }
    }

    pub(crate) fn def_name(&self, def: DefId) -> String {
        self.resolved
            .def(def)
            .map(|d| d.name.clone())
            .unwrap_or_default()
    }

    /// E0439: a struct literal or a struct pattern whose name is not a struct, an alias of one or
    /// a variant with named fields. A name that did not resolve was reported already.
    pub(crate) fn not_a_struct(&mut self, res: Option<Res>, path: &ast::Path, pattern: bool) {
        let name = path
            .segments
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(".");
        let kind = match res {
            None => return,
            Some(Res::Def(def)) => self.def_kind(def),
            Some(_) => None,
        };
        let what = match kind {
            Some(DefKind::Enum) => "an enum, not a struct",
            Some(DefKind::Interface) => "an interface, not a struct",
            Some(DefKind::Fn | DefKind::ExternFn | DefKind::Member { .. }) => {
                "a function, not a struct"
            }
            Some(DefKind::Config) => "a constant, not a struct",
            Some(DefKind::Alias) => "an alias of a type that is not a struct",
            Some(DefKind::IntrinsicType | DefKind::ExternType) => "a type without fields",
            _ => "not a struct",
        };
        let form = if pattern { "pattern" } else { "literal" };
        let message = format!("`{name}` is {what}");
        let mut d = Diagnostic::new(codes::E0439, message).primary(
            path.span,
            format!("a struct {form} names a struct or a variant with named fields"),
        );
        if kind == Some(DefKind::Enum) {
            d = d.help(format!(
                "name one of its variants: `{name}.Variant {{ ... }}`"
            ));
        }
        self.report(d);
    }

    pub(crate) fn def_kind(&self, def: DefId) -> Option<DefKind> {
        self.resolved.def(def).map(|d| d.kind)
    }

    /// A type as written in source, for messages.
    pub(crate) fn show(&self, ty: &Ty) -> String {
        match self.infer.resolve(ty) {
            Ty::Named(def, args) => {
                let name = self.def_name(def);
                if args.is_empty() {
                    name
                } else {
                    let args: Vec<String> = args.iter().map(|a| self.show(a)).collect();
                    format!("{name}<{}>", args.join(", "))
                }
            }
            Ty::Fn(params, ret) => {
                let params: Vec<String> = params
                    .iter()
                    .map(|(mode, t)| format!("{}{}", mode_prefix(*mode), self.show(t)))
                    .collect();
                if self.is(&ret, self.b.void) {
                    format!("fn({})", params.join(", "))
                } else {
                    format!("fn({}) -> {}", params.join(", "), self.show(&ret))
                }
            }
            Ty::Param(ParamId::Decl(span)) => self
                .param_names
                .get(&span)
                .cloned()
                .unwrap_or_else(|| "T".to_string()),
            Ty::Param(ParamId::SelfType) => "Self".to_string(),
            Ty::Var(_) | Ty::Error => "_".to_string(),
            Ty::Never => "Never".to_string(),
        }
    }

    // ----- written types ----------------------------------------------------------------------

    /// Turns a written type into a [`Ty`], expanding aliases.
    pub(crate) fn lower(&mut self, ty: &ast::Ty, scope: &Scope) -> Ty {
        match &ty.kind {
            TyKind::Error => Ty::Error,
            TyKind::SelfType => scope.self_ty.clone().unwrap_or(Ty::Error),
            TyKind::Fn { params, ret } => {
                let params = params
                    .iter()
                    .map(|(mode, t)| (*mode, self.lower(t, scope)))
                    .collect();
                let ret = match ret {
                    Some(ret) => self.lower(ret, scope),
                    None => self.void(),
                };
                Ty::Fn(params, Box::new(ret))
            }
            TyKind::Path { path, args } => {
                let lowered: Vec<Ty> = args.iter().map(|a| self.lower(a, scope)).collect();
                let name = path
                    .segments
                    .last()
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                match self.resolved.res.get(&(scope.file, ty.id)).copied() {
                    Some(Res::TypeParam(span)) => {
                        if !lowered.is_empty() {
                            self.report(
                                Diagnostic::new(
                                    codes::E0403,
                                    format!("type parameter `{name}` takes no type arguments"),
                                )
                                .primary(ty.span, "remove the type arguments"),
                            );
                        }
                        Ty::Param(ParamId::Decl(span))
                    }
                    Some(Res::SelfType) => scope.self_ty.clone().unwrap_or(Ty::Error),
                    Some(Res::Def(def)) => self.named(def, lowered, ty.span),
                    _ => Ty::Error,
                }
            }
        }
    }

    /// A named type with its arguments, checking their number.
    fn named(&mut self, def: DefId, args: Vec<Ty>, span: Span) -> Ty {
        let Some(kind) = self.def_kind(def) else {
            return Ty::Error;
        };
        if !kind.is_type() {
            return Ty::Error;
        }
        if Some(def) == self.b.never {
            return Ty::Never;
        }
        let expected = self.type_generics.get(&def).map_or(0, Vec::len);
        if args.len() != expected {
            let name = self.def_name(def);
            let d = if expected == 0 {
                Diagnostic::new(
                    codes::E0403,
                    format!(
                        "`{name}` takes no type arguments, but {} given",
                        given(args.len())
                    ),
                )
                .primary(span, "remove the type arguments")
            } else if args.is_empty() {
                Diagnostic::new(
                    codes::E0403,
                    format!("`{name}` needs {}", count(expected, "type argument")),
                )
                .primary(
                    span,
                    format!("missing {}", count(expected, "type argument")),
                )
            } else {
                Diagnostic::new(
                    codes::E0403,
                    format!(
                        "`{name}` takes {}, but {} given",
                        count(expected, "type argument"),
                        given(args.len())
                    ),
                )
                .primary(
                    span,
                    format!("expected {}", count(expected, "type argument")),
                )
            };
            self.report(d);
            return Ty::Error;
        }
        if kind == DefKind::Alias {
            return self.expand_alias(def, args);
        }
        if kind == DefKind::Interface && !self.in_bound {
            self.pending_object.push((def, span));
        }
        // Bounds of the type's parameters (`Map<K: Hashable, V>`), checked once all types are
        // known. Types that mention type parameters are checked with generics (M5).
        let bounds: Vec<Vec<Ty>> = self
            .type_generics
            .get(&def)
            .map(|g| g.iter().map(|g| g.bounds.clone()).collect())
            .unwrap_or_default();
        for (arg, bounds) in args.iter().zip(bounds) {
            for bound in bounds {
                if let Ty::Named(interface, _) = bound {
                    self.pending_bounds.push((arg.clone(), interface, span));
                }
            }
        }
        Ty::Named(def, args)
    }

    /// Checks the bounds recorded while written types were lowered.
    pub(crate) fn check_pending_bounds(&mut self) {
        self.check_object_safety();
        for (ty, interface, span) in std::mem::take(&mut self.pending_bounds) {
            if ty.has_error() || has_self(&ty) {
                continue;
            }
            let why = if Some(interface) == self.b.hashable {
                "a map key".to_string()
            } else {
                "required by a bound of this type".to_string()
            };
            self.require(&ty, interface, span, &why);
        }
    }

    pub(crate) fn expand_alias(&mut self, def: DefId, args: Vec<Ty>) -> Ty {
        let body = match self.alias_state.get(&def) {
            Some(AliasState::Done(t)) => t.clone(),
            Some(AliasState::InProgress) => {
                self.alias_cycle(def);
                return Ty::Error;
            }
            None => self.lower_alias(def),
        };
        let map: BTreeMap<ParamId, Ty> = self
            .type_generics
            .get(&def)
            .map(|g| g.iter().map(|g| ParamId::Decl(g.span)).zip(args).collect())
            .unwrap_or_default();
        body.subst(&map)
    }

    /// The bounds of a method's `where`, one list for each type parameter of its `impl` block
    /// (chapter 4, section 8.6). A `where` anywhere else, or one that names something other
    /// than a parameter of the block, is E0442.
    pub(crate) fn lower_where(
        &mut self,
        sig: &ast::FnSig,
        block_generics: &[ast::GenericParam],
        in_interface_impl: bool,
        scope: &Scope,
    ) -> Vec<Vec<Ty>> {
        let mut out: Vec<Vec<Ty>> = vec![Vec::new(); block_generics.len()];
        for param in &sig.where_bounds {
            let name = &param.name.name;
            let index = block_generics.iter().position(|g| g.name.name == *name);
            let why = if in_interface_impl {
                Some(
                    "an interface implementation adds no bounds; the interface decides".to_string(),
                )
            } else if sig.generics.iter().any(|g| g.name.name == *name) {
                Some(format!(
                    "`{name}` is a parameter of the method: write `<{name}: ...>`"
                ))
            } else if index.is_none() {
                Some(format!(
                    "`{name}` is not a type parameter of the `impl` block"
                ))
            } else {
                None
            };
            if let Some(why) = why {
                self.report(
                    Diagnostic::new(codes::E0442, "`where` that is not allowed here")
                        .primary(param.name.span, why),
                );
                continue;
            }
            let outer = std::mem::replace(&mut self.in_bound, true);
            let bounds: Vec<Ty> = param.bounds.iter().map(|b| self.lower(b, scope)).collect();
            self.in_bound = outer;
            if let Some(slot) = index.and_then(|i| out.get_mut(i)) {
                slot.extend(bounds);
            }
        }
        out
    }

    /// E0442 for a `where` on a function that is not a method of an inherent block.
    pub(crate) fn no_where(&mut self, sig: &ast::FnSig, what: &str) {
        if let Some(first) = sig.where_bounds.first() {
            self.report(
                Diagnostic::new(codes::E0442, "`where` that is not allowed here")
                    .primary(first.name.span, format!("{what} has no `where`"))
                    .help("put the bounds in `<T: ...>` after the name"),
            );
        }
    }

    /// The struct type an alias stands for, with the written type arguments of the alias or new
    /// inference variables; `None` when the alias stands for something else.
    pub(crate) fn struct_of_alias(&mut self, def: DefId, written: Option<Vec<Ty>>) -> Option<Ty> {
        let count = self.type_generics.get(&def).map_or(0, Vec::len);
        let args = match written {
            Some(args) if args.len() == count => args,
            _ => (0..count).map(|_| self.infer.fresh()).collect(),
        };
        match self.expand_alias(def, args) {
            t @ Ty::Named(target, _) if self.def_kind(target) == Some(DefKind::Struct) => Some(t),
            _ => None,
        }
    }

    /// The type an alias without type parameters stands for.
    pub(crate) fn alias_type(&self, def: DefId) -> Option<Ty> {
        if self.type_generics.get(&def).is_some_and(|g| !g.is_empty()) {
            return None;
        }
        match self.alias_state.get(&def) {
            Some(AliasState::Done(t)) => Some(t.clone()),
            _ => None,
        }
    }

    fn lower_alias(&mut self, def: DefId) -> Ty {
        let Some((module, decl)) = self.alias_decls.get(&def).copied() else {
            return Ty::Error;
        };
        self.alias_state.insert(def, AliasState::InProgress);
        self.alias_stack.push(def);
        let scope = Scope {
            file: self.file_of(module),
            self_ty: None,
        };
        let ty = self.lower(&decl.ty, &scope);
        self.alias_stack.pop();
        match self.alias_state.get(&def) {
            Some(AliasState::Done(done)) => done.clone(),
            _ => {
                self.alias_state.insert(def, AliasState::Done(ty.clone()));
                ty
            }
        }
    }

    /// Reports a cycle of aliases that ends at `def` (E0419).
    fn alias_cycle(&mut self, def: DefId) {
        let Some(start) = self.alias_stack.iter().position(|d| *d == def) else {
            return;
        };
        let cycle: Vec<DefId> = self.alias_stack.iter().skip(start).copied().collect();
        for member in &cycle {
            self.alias_state
                .insert(*member, AliasState::Done(Ty::Error));
        }
        let mut names: Vec<String> = cycle
            .iter()
            .map(|d| format!("`{}`", self.def_name(*d)))
            .collect();
        names.push(format!("`{}`", self.def_name(def)));
        let span = self.resolved.def(def).map(|d| d.span);
        let mut d = Diagnostic::new(
            codes::E0419,
            format!("cycle in type aliases: {}", names.join(" -> ")),
        )
        .note("an alias is another name for a type, so it cannot contain itself; use a struct or an enum");
        if let Some(span) = span {
            d = d.primary(span, "this alias refers to itself");
        }
        self.report(d);
    }

    fn lower_generics(&mut self, generics: &[ast::GenericParam], scope: &Scope) -> Vec<Generic> {
        generics
            .iter()
            .map(|g| {
                self.param_names.insert(g.name.span, g.name.name.clone());
                let outer = std::mem::replace(&mut self.in_bound, true);
                let bounds: Vec<Ty> = g.bounds.iter().map(|b| self.lower(b, scope)).collect();
                self.in_bound = outer;
                self.param_bounds.insert(g.name.span, bounds.clone());
                Generic {
                    span: g.name.span,
                    bounds,
                }
            })
            .collect()
    }

    pub(crate) fn lower_sig(&mut self, sig: &ast::FnSig, scope: &Scope) -> Sig {
        let generics = self.lower_generics(&sig.generics, scope);
        let mut self_mode = None;
        let mut params = Vec::new();
        for param in &sig.params {
            match &param.kind {
                ParamKind::SelfParam => self_mode = Some(param.mode),
                ParamKind::Named { ty, .. } => params.push((param.mode, self.lower(ty, scope))),
            }
        }
        let ret = match &sig.ret {
            Some(ret) => self.lower(ret, scope),
            None => self.void(),
        };
        Sig {
            generics,
            self_mode,
            params,
            ret,
        }
    }

    // ----- collection -------------------------------------------------------------------------

    /// Builds the types of all declarations and returns the bodies to check.
    pub(crate) fn collect(&mut self) -> Vec<BodyRef<'a>> {
        self.collect_generics();
        self.collect_aliases();
        let bodies = self.collect_items();
        self.infinite_structs();
        self.check_pending_bounds();
        bodies
    }

    fn item_def(&self, name: &ast::Ident) -> Option<DefId> {
        self.def_at.get(&name.span).copied()
    }

    /// Type parameters of type declarations: names first, so that bounds and fields can refer to
    /// any type.
    fn collect_generics(&mut self) {
        let inputs = self.inputs;
        let mut pending: Vec<(DefId, usize, &'a [ast::GenericParam])> = Vec::new();
        for (m, input) in inputs.iter().enumerate() {
            for section in &input.ast.sections {
                for item in &section.items {
                    let (name, generics) = match &item.kind {
                        ItemKind::Struct(d) => (&d.name, d.generics.as_slice()),
                        ItemKind::Enum(d) => (&d.name, d.generics.as_slice()),
                        ItemKind::Alias(d) => (&d.name, d.generics.as_slice()),
                        ItemKind::IntrinsicType(d) => (&d.name, d.generics.as_slice()),
                        ItemKind::Interface(d) => (&d.name, d.generics.as_slice()),
                        _ => continue,
                    };
                    let Some(def) = self.item_def(name) else {
                        continue;
                    };
                    if let ItemKind::Alias(decl) = &item.kind {
                        self.alias_decls.insert(def, (m, decl));
                    }
                    let list = generics
                        .iter()
                        .map(|g| {
                            self.param_names.insert(g.name.span, g.name.name.clone());
                            Generic {
                                span: g.name.span,
                                bounds: Vec::new(),
                            }
                        })
                        .collect();
                    self.type_generics.insert(def, list);
                    pending.push((def, m, generics));
                }
            }
        }
        for (def, m, generics) in pending {
            let scope = Scope {
                file: self.file_of(m),
                self_ty: None,
            };
            let lowered = self.lower_generics(generics, &scope);
            self.type_generics.insert(def, lowered);
        }
    }

    fn collect_aliases(&mut self) {
        let defs: Vec<DefId> = self.alias_decls.keys().copied().collect();
        for def in defs {
            if !self.alias_state.contains_key(&def) {
                let _ = self.lower_alias(def);
            }
        }
    }

    fn collect_items(&mut self) -> Vec<BodyRef<'a>> {
        let inputs = self.inputs;
        let mut bodies = Vec::new();
        // Impl blocks are collected after all other items, so their signatures may name any
        // function type; the order is still deterministic.
        let mut impl_blocks = Vec::new();
        let mut extern_blocks = Vec::new();
        let mut first_extern = BTreeMap::new();
        for (m, input) in inputs.iter().enumerate() {
            let scope = Scope {
                file: input.file,
                self_ty: None,
            };
            for (s, section) in input.ast.sections.iter().enumerate() {
                for (i, item) in section.items.iter().enumerate() {
                    match &item.kind {
                        ItemKind::Struct(decl) => {
                            let Some(def) = self.item_def(&decl.name) else {
                                continue;
                            };
                            let fields = self.lower_fields(&decl.fields, &scope);
                            self.fields.insert(def, fields);
                        }
                        ItemKind::Enum(decl) => {
                            let Some(def) = self.item_def(&decl.name) else {
                                continue;
                            };
                            let mut list = Vec::new();
                            for variant in &decl.variants {
                                let Some(v) = self.item_def(&variant.name) else {
                                    continue;
                                };
                                let shape = match &variant.payload {
                                    VariantPayload::None => Shape::None,
                                    VariantPayload::Tuple(types) => Shape::Tuple(
                                        types.iter().map(|t| self.lower(t, &scope)).collect(),
                                    ),
                                    VariantPayload::Struct(fields) => {
                                        Shape::Struct(self.lower_fields(fields, &scope))
                                    }
                                };
                                self.variants.insert(v, VariantInfo { parent: def, shape });
                                list.push(v);
                            }
                            self.enum_variants.insert(def, list);
                        }
                        ItemKind::Interface(decl) => {
                            let Some(def) = self.item_def(&decl.name) else {
                                continue;
                            };
                            let iscope = Scope {
                                file: input.file,
                                self_ty: Some(Ty::Param(ParamId::SelfType)),
                            };
                            let mut sigs = BTreeMap::new();
                            for method in &decl.methods {
                                let sig = self.lower_sig(&method.sig, &iscope);
                                self.no_where(&method.sig, "a method of an interface");
                                self.interface_member(&method.sig, &sig);
                                sigs.entry(method.sig.name.name.clone()).or_insert(sig);
                            }
                            self.interface_methods.insert(def, sigs);
                        }
                        ItemKind::Config(entry) => {
                            let Some(def) = self.item_def(&entry.name) else {
                                continue;
                            };
                            let ty = self.lower(&entry.ty, &scope);
                            self.consts.insert(def, ty);
                            bodies.push(BodyRef::Config {
                                module: m,
                                def,
                                entry,
                            });
                        }
                        ItemKind::Fn(decl) => {
                            if let Some(def) = self.item_def(&decl.sig.name) {
                                let sig = self.lower_sig(&decl.sig, &scope);
                                self.no_where(&decl.sig, "a free function");
                                self.fns.insert(def, sig);
                            }
                            if decl.body.is_some() {
                                bodies.push(BodyRef::Fn {
                                    module: m,
                                    decl,
                                    self_ty: None,
                                    method: None,
                                });
                            }
                        }
                        // After every struct: a record binding reads the fields of its struct
                        // (chapter 7, section 8.4a).
                        ItemKind::ExternTarget(target) => {
                            extern_blocks.push((m, target, input.file))
                        }
                        ItemKind::Test(test) => bodies.push(BodyRef::Test { module: m, test }),
                        ItemKind::Impl(block) => impl_blocks.push((m, s, i, block)),
                        ItemKind::Alias(_) | ItemKind::IntrinsicType(_) | ItemKind::Import(_) => {}
                    }
                }
            }
        }
        for (m, target, file) in extern_blocks {
            let scope = Scope {
                file,
                self_ty: None,
            };
            self.extern_block(m, target, &scope, &mut first_extern);
        }
        for (m, s, i, block) in impl_blocks {
            self.impl_block(m, s, i, block, &mut bodies);
        }
        bodies
    }

    fn lower_fields(&mut self, fields: &[ast::Field], scope: &Scope) -> Vec<FieldInfo> {
        fields
            .iter()
            .map(|f| FieldInfo {
                name: f.name.name.clone(),
                span: f.name.span,
                ty: self.lower(&f.ty, scope),
                public: f.public,
            })
            .collect()
    }

    fn impl_block(
        &mut self,
        m: usize,
        s: usize,
        i: usize,
        block: &'a ast::ImplBlock,
        bodies: &mut Vec<BodyRef<'a>>,
    ) {
        let module = ModuleId(u32::try_from(m).unwrap_or(u32::MAX));
        let Some(owner) = self.resolved.impls.get(&(module, s, i)).copied() else {
            return;
        };
        let Some(owner_def) = self.resolved.def(owner).cloned() else {
            return;
        };
        // Coherence (chapter 4, rule 8.3.2): every impl is in the module of its type.
        if owner_def.module != module {
            let what = if block.interface.is_some() {
                "an interface implementation"
            } else {
                "an `impl` block"
            };
            self.report(
                Diagnostic::new(
                    codes::E0414,
                    format!("{what} for `{}` outside the module that declares it", owner_def.name),
                )
                .primary(block.type_name.span, "this type is declared in another module")
                .note("every `impl` block must be in the module that declares its type, so that it maps onto Go methods"),
            );
            return;
        }
        if !matches!(
            owner_def.kind,
            DefKind::Struct | DefKind::Enum | DefKind::IntrinsicType | DefKind::ExternType
        ) {
            return;
        }
        // Only a native module gives an opaque type methods (chapter 7, sections 8.3 and 8.7).
        let native = self
            .resolved
            .modules
            .get(m)
            .is_some_and(|x| x.role == wolf_resolve::Role::Native);
        if owner_def.kind == DefKind::ExternType && !native {
            self.report(
                Diagnostic::new(
                    codes::E0707,
                    format!(
                        "`{}` is an extern type and cannot have an `impl` block",
                        owner_def.name
                    ),
                )
                .primary(
                    block.type_name.span,
                    "an extern type has no methods of its own",
                )
                .help("declare an extern function that takes the value as its first parameter"),
            );
            return;
        }
        let file = self.file_of(m);
        let expected = self.type_generics.get(&owner).map_or(0, Vec::len);
        if block.generics.len() != expected {
            self.report(
                Diagnostic::new(
                    codes::E0403,
                    format!(
                        "`{}` takes {}, but the `impl` block declares {}",
                        owner_def.name,
                        count(expected, "type parameter"),
                        block.generics.len()
                    ),
                )
                .primary(
                    block.type_name.span,
                    format!("expected {}", count(expected, "type parameter")),
                ),
            );
            return;
        }
        let scope0 = Scope {
            file,
            self_ty: None,
        };
        let mut generics = self.lower_generics(&block.generics, &scope0);
        let impl_generics: Vec<Span> = generics.iter().map(|g| g.span).collect();
        // The bounds of the type's declaration apply without being repeated (chapter 3, 5.6).
        let declared = self.type_generics.get(&owner).cloned().unwrap_or_default();
        let to_impl: BTreeMap<ParamId, Ty> = declared
            .iter()
            .zip(&impl_generics)
            .map(|(d, s)| (ParamId::Decl(d.span), Ty::Param(ParamId::Decl(*s))))
            .collect();
        for (g, d) in generics.iter_mut().zip(&declared) {
            for bound in &d.bounds {
                let bound = bound.subst(&to_impl);
                if !g.bounds.contains(&bound) {
                    g.bounds.push(bound);
                }
            }
            self.param_bounds.insert(g.span, g.bounds.clone());
        }
        let self_ty = Ty::Named(
            owner,
            impl_generics
                .iter()
                .map(|s| Ty::Param(ParamId::Decl(*s)))
                .collect(),
        );
        let scope = Scope {
            file,
            self_ty: Some(self_ty.clone()),
        };
        let mut interface = None;
        let mut public_methods = None;
        let mut interface_args = Vec::new();
        if let Some(iface) = &block.interface {
            let outer = std::mem::replace(&mut self.in_bound, true);
            let lowered = self.lower(iface, &scope);
            self.in_bound = outer;
            match lowered {
                Ty::Named(def, args) if self.def_kind(def) == Some(DefKind::Interface) => {
                    let name = self.def_name(def);
                    let derived =
                        [self.b.equatable, self.b.copyable, self.b.hashable].contains(&Some(def));
                    if derived {
                        self.report(
                            Diagnostic::new(
                                codes::E0431,
                                format!("`{name}` cannot be implemented by hand"),
                            )
                            .primary(iface.span, "a built-in interface")
                            .note("`Equatable` and `Copyable` are derived from the fields of a type, and `Hashable` is a fixed set of types"),
                        );
                        return;
                    }
                    let twice = self
                        .impls
                        .get(&owner)
                        .is_some_and(|list| list.iter().any(|i| i.interface == def));
                    if twice {
                        self.report(
                            Diagnostic::new(
                                codes::E0431,
                                format!("`{}` implements `{name}` twice", owner_def.name),
                            )
                            .primary(iface.span, "second implementation")
                            .note("a type implements an interface at most once"),
                        );
                        return;
                    }
                    interface = Some(def);
                    interface_args = args.clone();
                    public_methods = self.resolved.def(def).map(|d| d.public);
                    self.impls.entry(owner).or_default().push(ImplOf {
                        interface: def,
                        args,
                        generics: impl_generics.clone(),
                    });
                }
                Ty::Error => return,
                other => {
                    let shown = self.show(&other);
                    self.report(
                        Diagnostic::new(codes::E0431, format!("`{shown}` is not an interface"))
                            .primary(iface.span, "expected an interface")
                            .help(format!(
                                "methods of `{}` itself go into a block without `:`",
                                owner_def.name
                            )),
                    );
                    return;
                }
            }
        }
        let mut implemented: Vec<(String, Span, Sig)> = Vec::new();
        for method in &block.methods {
            let sig = self.lower_sig(&method.decl.sig, &scope);
            let has_self = sig.self_mode.is_some();
            let name = method.decl.sig.name.clone();
            let where_bounds = self.lower_where(
                &method.decl.sig,
                &block.generics,
                interface.is_some(),
                &scope,
            );
            implemented.push((name.name.clone(), name.span, sig.clone()));
            let index = self.methods.len();
            self.methods.push(Method {
                name: name.name.clone(),
                span: name.span,
                owner,
                module,
                impl_generics: impl_generics.clone(),
                impl_bounds: generics.iter().map(|g| g.bounds.clone()).collect(),
                where_bounds,
                sig,
                public: public_methods.unwrap_or(method.decl.public),
                interface,
            });
            let table = self.method_table.entry(owner).or_default();
            match table.get(&name.name).copied() {
                Some(first) => {
                    let first_span = self.methods.get(first).map(|f| f.span);
                    let mut d = Diagnostic::new(
                        codes::E0415,
                        format!(
                            "`{}` has two functions named `{}`",
                            owner_def.name, name.name
                        ),
                    )
                    .primary(name.span, "second definition")
                    .note("a type has at most one function with a given name, across its inherent block and all its interface implementations");
                    if let Some(span) = first_span {
                        d = d.secondary(span, "first definition");
                    }
                    self.report(d);
                }
                None => {
                    table.insert(name.name.clone(), index);
                }
            }
            // TypeScript classes and Go structs keep fields and methods in one namespace
            // (spec chapter 4, section 8.3, rule 6).
            if has_self {
                let field = self
                    .fields
                    .get(&owner)
                    .and_then(|fields| fields.iter().find(|f| f.name == name.name))
                    .map(|f| f.span);
                if let Some(field_span) = field {
                    self.report(
                        Diagnostic::new(
                            codes::E0432,
                            format!(
                                "`{}` has a field and a method named `{}`",
                                owner_def.name, name.name
                            ),
                        )
                        .primary(name.span, "method")
                        .secondary(field_span, "field")
                        .help("rename the method or the field")
                        .note("fields and methods of a struct share one namespace in TypeScript and Go"),
                    );
                }
            }
            if interface.is_none() {
                if let Some(def) = self.item_def(&name) {
                    self.member_method.insert(def, index);
                }
            }
            if method.decl.body.is_some() {
                bodies.push(BodyRef::Fn {
                    module: m,
                    decl: &method.decl,
                    self_ty: Some(self_ty.clone()),
                    method: Some(index),
                });
            }
        }
        if let Some(iface) = interface {
            self.conformance(iface, &interface_args, &self_ty, block, &implemented);
        }
    }

    /// An implementation defines exactly the methods of its interface, with the same signatures
    /// after `Self` is replaced (chapter 4, rule 8.3.1; E0413).
    fn conformance(
        &mut self,
        iface: DefId,
        args: &[Ty],
        self_ty: &Ty,
        block: &ast::ImplBlock,
        implemented: &[(String, Span, Sig)],
    ) {
        let Some(required) = self.interface_methods.get(&iface).cloned() else {
            return;
        };
        let mut map: BTreeMap<ParamId, Ty> = BTreeMap::new();
        map.insert(ParamId::SelfType, self_ty.clone());
        if let Some(g) = self.type_generics.get(&iface) {
            for (g, a) in g.iter().zip(args) {
                map.insert(ParamId::Decl(g.span), a.clone());
            }
        }
        let iname = self.def_name(iface);
        let owner = block.type_name.name.clone();
        let missing: Vec<String> = required
            .keys()
            .filter(|n| !implemented.iter().any(|(name, _, _)| name == *n))
            .map(|n| format!("`{n}`"))
            .collect();
        if !missing.is_empty() {
            let word = if missing.len() == 1 {
                "method"
            } else {
                "methods"
            };
            self.report(
                Diagnostic::new(
                    codes::E0413,
                    format!("`{owner}` does not implement every method of `{iname}`"),
                )
                .primary(
                    block.type_name.span,
                    format!("missing {word} {}", missing.join(", ")),
                ),
            );
        }
        for (name, span, sig) in implemented {
            let Some(req) = required.get(name) else {
                self.report(
                    Diagnostic::new(
                        codes::E0413,
                        format!("`{name}` is not a method of `{iname}`"),
                    )
                    .primary(*span, "not in the interface")
                    .help(format!(
                        "move it into a block `{owner} {{ ... }}` without an interface"
                    )),
                );
                continue;
            };
            let expected = Sig {
                generics: Vec::new(),
                self_mode: req.self_mode,
                params: req
                    .params
                    .iter()
                    .map(|(m, t)| (*m, t.subst(&map)))
                    .collect(),
                ret: req.ret.subst(&map),
            };
            let same = sig.generics.is_empty()
                && sig.self_mode == expected.self_mode
                && sig.params == expected.params
                && sig.ret == expected.ret;
            if !same && !sig.params.iter().any(|(_, t)| t.has_error()) && !sig.ret.has_error() {
                self.report(
                    Diagnostic::new(
                        codes::E0413,
                        format!("`{name}` does not match its signature in `{iname}`"),
                    )
                    .primary(*span, "different signature")
                    .note(format!("expected `{}`", self.show_sig(name, &expected)))
                    .note(format!("found `{}`", self.show_sig(name, sig))),
                );
            }
        }
    }

    fn show_sig(&self, name: &str, sig: &Sig) -> String {
        let mut parts = Vec::new();
        if let Some(mode) = sig.self_mode {
            parts.push(format!("{}self", mode_prefix(mode)));
        }
        for (mode, t) in &sig.params {
            parts.push(format!("{}{}", mode_prefix(*mode), self.show(t)));
        }
        let generics = if sig.generics.is_empty() {
            String::new()
        } else {
            let names: Vec<String> = sig
                .generics
                .iter()
                .map(|g| self.param_names.get(&g.span).cloned().unwrap_or_default())
                .collect();
            format!("<{}>", names.join(", "))
        };
        let ret = if self.is(&sig.ret, self.b.void) {
            String::new()
        } else {
            format!(" -> {}", self.show(&sig.ret))
        };
        format!("fn {name}{generics}({}){ret}", parts.join(", "))
    }

    /// Members of an interface take `self` and have no type parameters (chapter 4, 8.2; E0417).
    fn interface_member(&mut self, ast_sig: &ast::FnSig, sig: &Sig) {
        if let Some(first) = ast_sig.generics.first() {
            self.report(
                Diagnostic::new(
                    codes::E0417,
                    format!("interface method `{}` cannot have type parameters", ast_sig.name.name),
                )
                .primary(first.name.span, "type parameter of an interface method")
                .note("Go interfaces and Rust trait objects cannot have generic methods; put the type parameter on the interface or use an inherent `impl`"),
            );
        }
        if sig.self_mode.is_none() {
            self.report(
                Diagnostic::new(
                    codes::E0417,
                    format!("interface member `{}` has no `self`", ast_sig.name.name),
                )
                .primary(ast_sig.name.span, "every interface member is a method")
                .help("add a `self` parameter, or make it a free function"),
            );
        }
    }

    /// The first method of an interface that mentions `Self` outside its receiver.
    fn not_object_safe(&self, iface: DefId) -> Option<String> {
        self.interface_methods
            .get(&iface)?
            .iter()
            .find_map(|(name, sig)| {
                let mentions = sig.params.iter().any(|(_, t)| has_self(t)) || has_self(&sig.ret);
                mentions.then(|| name.clone())
            })
    }

    /// Interfaces used as value types must be object-safe (chapter 4, 8.5; E0416).
    fn check_object_safety(&mut self) {
        let mut seen = BTreeSet::new();
        for (iface, span) in std::mem::take(&mut self.pending_object) {
            if !seen.insert(span) {
                continue;
            }
            if let Some(method) = self.not_object_safe(iface) {
                let name = self.def_name(iface);
                self.report(
                    Diagnostic::new(codes::E0416, format!("`{name}` cannot be used as a type"))
                        .primary(span, "this interface is not object-safe")
                        .note(format!("method `{method}` mentions `Self` outside its receiver, so a value of type `{name}` could not call it"))
                        .help(format!("use it as a bound instead: `<T: {name}>`")),
                );
            }
        }
    }

    /// A struct that contains itself directly, possibly through other structs, has no finite
    /// values (E0418).
    fn infinite_structs(&mut self) {
        let structs: Vec<DefId> = self.fields.keys().copied().collect();
        let mut reported: BTreeSet<DefId> = BTreeSet::new();
        for start in structs {
            if reported.contains(&start) {
                continue;
            }
            let mut path = Vec::new();
            let mut visited = BTreeSet::new();
            if self.reaches(start, start, &BTreeMap::new(), &mut path, &mut visited) {
                let Some((first_field, _)) = path.first().cloned() else {
                    continue;
                };
                let route: Vec<String> = path.iter().map(|(_, s)| s.clone()).collect();
                let name = self.def_name(start);
                self.report(
                    Diagnostic::new(codes::E0418, format!("struct `{name}` contains itself"))
                        .primary(first_field, "this field makes the struct infinitely large")
                        .note(format!(
                            "`{name}` contains {}",
                            route.join(", which contains ")
                        ))
                        .help("store it in an `Option<...>`, a `List<...>` or an enum variant"),
                );
                for def in self.structs_on(&path) {
                    reported.insert(def);
                }
                reported.insert(start);
            }
        }
    }

    fn structs_on(&self, path: &[(Span, String)]) -> Vec<DefId> {
        path.iter()
            .filter_map(|(span, _)| {
                self.fields
                    .iter()
                    .find(|(_, fs)| fs.iter().any(|f| f.span == *span))
                    .map(|(d, _)| *d)
            })
            .collect()
    }

    /// Whether the fields of `current` (with `map` applied) reach the struct `target` directly.
    fn reaches(
        &self,
        current: DefId,
        target: DefId,
        map: &BTreeMap<ParamId, Ty>,
        path: &mut Vec<(Span, String)>,
        visited: &mut BTreeSet<DefId>,
    ) -> bool {
        if !visited.insert(current) {
            return false;
        }
        let Some(fields) = self.fields.get(&current) else {
            return false;
        };
        for field in fields {
            let Ty::Named(def, args) = field.ty.subst(map) else {
                continue;
            };
            if !self.fields.contains_key(&def) {
                continue;
            }
            path.push((
                field.span,
                format!(
                    "field `{}` of type `{}`",
                    field.name,
                    self.show(&Ty::Named(def, args.clone()))
                ),
            ));
            if def == target {
                return true;
            }
            let inner: BTreeMap<ParamId, Ty> = self
                .type_generics
                .get(&def)
                .map(|g| g.iter().map(|g| ParamId::Decl(g.span)).zip(args).collect())
                .unwrap_or_default();
            if self.reaches(def, target, &inner, path, visited) {
                return true;
            }
            path.pop();
        }
        false
    }
}

pub(crate) fn mode_prefix(mode: Mode) -> &'static str {
    match mode {
        Mode::Read => "",
        Mode::Mut => "mut ",
        Mode::Own => "own ",
    }
}

/// `1 type argument`, `2 type arguments`.
pub(crate) fn count(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

/// `1 was`, `2 were`.
pub(crate) fn given(n: usize) -> String {
    if n == 1 {
        "1 was".to_string()
    } else {
        format!("{n} were")
    }
}

/// Whether a type mentions `Self` of an interface.
pub(crate) fn has_self(ty: &Ty) -> bool {
    match ty {
        Ty::Param(p) => *p == ParamId::SelfType,
        Ty::Named(_, args) => args.iter().any(has_self),
        Ty::Fn(params, ret) => params.iter().any(|(_, t)| has_self(t)) || has_self(ret),
        Ty::Var(_) | Ty::Never | Ty::Error => false,
    }
}
