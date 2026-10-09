//! Declarations: the built-in types, the types of declared items, and written types turned into
//! [`Ty`].

use std::collections::{BTreeMap, BTreeSet};

use wolf_ast::{self as ast, ExternDeclKind, ItemKind, Mode, ParamKind, TyKind, VariantPayload};
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
        }
    }
}

/// A type parameter of a declaration.
#[derive(Clone, Debug)]
pub(crate) struct Generic {
    pub(crate) span: Span,
    pub(crate) bounds: Vec<Ty>,
}

/// A field of a struct or of a struct variant.
#[derive(Clone, Debug)]
pub(crate) struct FieldInfo {
    pub(crate) name: String,
    pub(crate) span: Span,
    pub(crate) ty: Ty,
    pub(crate) public: bool,
}

/// The payload of a variant.
#[derive(Clone, Debug)]
pub(crate) enum Shape {
    None,
    Tuple(Vec<Ty>),
    Struct(Vec<FieldInfo>),
}

/// A variant of an enum.
#[derive(Clone, Debug)]
pub(crate) struct VariantInfo {
    pub(crate) parent: DefId,
    pub(crate) shape: Shape,
}

/// A function signature.
#[derive(Clone, Debug)]
pub(crate) struct Sig {
    pub(crate) generics: Vec<Generic>,
    /// The mode of `self`, for methods.
    pub(crate) self_mode: Option<Mode>,
    pub(crate) params: Vec<(Mode, Ty)>,
    pub(crate) ret: Ty,
}

/// A function of an `impl` block.
#[derive(Clone, Debug)]
pub(crate) struct Method {
    pub(crate) name: String,
    pub(crate) span: Span,
    pub(crate) owner: DefId,
    pub(crate) module: ModuleId,
    /// The type parameters of the `impl` block, in the order of the type's parameters.
    pub(crate) impl_generics: Vec<Span>,
    /// The bounds of the `impl` block's type parameters, in the same order.
    pub(crate) impl_bounds: Vec<Vec<Ty>>,
    pub(crate) sig: Sig,
    pub(crate) public: bool,
    /// The interface, for a function of an interface implementation.
    pub(crate) interface: Option<DefId>,
}

/// An implementation of an interface by a type.
#[derive(Clone, Debug)]
pub(crate) struct ImplOf {
    pub(crate) interface: DefId,
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
        }
    }

    pub(crate) fn finish(self) -> Typed {
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
        for (ty, interface, span) in std::mem::take(&mut self.pending_bounds) {
            if ty.has_error() || has_param(&ty) {
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

    fn expand_alias(&mut self, def: DefId, args: Vec<Ty>) -> Ty {
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
                Generic {
                    span: g.name.span,
                    bounds: g.bounds.iter().map(|b| self.lower(b, scope)).collect(),
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
                        ItemKind::ExternTarget(target) => {
                            for decl in &target.decls {
                                if let ExternDeclKind::Fn(sig) = &decl.kind {
                                    if let Some(def) = self.item_def(&sig.name) {
                                        let sig = self.lower_sig(sig, &scope);
                                        self.fns.entry(def).or_insert(sig);
                                    }
                                }
                            }
                        }
                        ItemKind::Test(test) => bodies.push(BodyRef::Test { module: m, test }),
                        ItemKind::Impl(block) => impl_blocks.push((m, s, i, block)),
                        ItemKind::Alias(_) | ItemKind::IntrinsicType(_) | ItemKind::Import(_) => {}
                    }
                }
            }
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
        let generics = self.lower_generics(&block.generics, &scope0);
        let impl_generics: Vec<Span> = generics.iter().map(|g| g.span).collect();
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
        if let Some(iface) = &block.interface {
            match self.lower(iface, &scope) {
                Ty::Named(def, _) if self.def_kind(def) == Some(DefKind::Interface) => {
                    interface = Some(def);
                    public_methods = self.resolved.def(def).map(|d| d.public);
                    self.impls
                        .entry(owner)
                        .or_default()
                        .push(ImplOf { interface: def });
                }
                // Not an interface: reported with interface implementations (M5).
                _ => return,
            }
        }
        for method in &block.methods {
            let sig = self.lower_sig(&method.decl.sig, &scope);
            let name = method.decl.sig.name.clone();
            let index = self.methods.len();
            self.methods.push(Method {
                name: name.name.clone(),
                span: name.span,
                owner,
                module,
                impl_generics: impl_generics.clone(),
                impl_bounds: generics.iter().map(|g| g.bounds.clone()).collect(),
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

/// Whether a type mentions a type parameter.
fn has_param(ty: &Ty) -> bool {
    match ty {
        Ty::Param(_) => true,
        Ty::Named(_, args) => args.iter().any(has_param),
        Ty::Fn(params, ret) => params.iter().any(|(_, t)| has_param(t)) || has_param(ret),
        Ty::Var(_) | Ty::Never | Ty::Error => false,
    }
}
