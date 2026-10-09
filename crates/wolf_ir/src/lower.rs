//! Lowering of checked syntax trees to the IR.
//!
//! The input is a program without errors: every name is resolved and every expression has a
//! type. Lowering reads the syntax tree together with the tables of name resolution and type
//! checking, and makes explicit what the checker decided: which function a call calls, where a
//! value is coerced to an interface, which blocks discard their final value.

use std::collections::BTreeMap;

use wolf_ast::{self as ast, ExprKind as AstExpr, ItemKind, Mode, PatKind, StmtKind};
use wolf_project::Kind;
use wolf_resolve::{DefId, DefKind as RDef, ModuleInput, Res, Resolved, Role};
use wolf_span::{FileId, SourceMap, Span};
use wolf_types::{ParamId, Shape, Ty, Typed};

use crate::*;

/// Facts about the project that the IR records.
pub struct ProjectMeta<'a> {
    /// The package name.
    pub name: &'a str,
    /// The package version.
    pub version: &'a str,
    /// `cli` or `lib`.
    pub kind: Kind,
}

/// A construct the IR cannot express yet. The driver reports it as an internal error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LowerError {
    /// What is not supported.
    pub message: String,
}

/// Lowers a checked program. `modules` are the modules given to name resolution and type
/// checking, std included.
pub fn lower(
    modules: &[ModuleInput<'_>],
    resolved: &Resolved,
    typed: &Typed,
    map: &SourceMap,
    meta: &ProjectMeta<'_>,
) -> Result<Program, LowerError> {
    let mut l = Lowerer::new(modules, resolved, typed, map);
    l.defs();
    let mut program = Program {
        name: meta.name.to_string(),
        version: meta.version.to_string(),
        kind: match meta.kind {
            Kind::Cli => ProgramKind::Cli,
            Kind::Lib => ProgramKind::Lib,
        },
        modules: Vec::new(),
        defs: BTreeMap::new(),
        locals: BTreeMap::new(),
    };
    for (index, input) in modules.iter().enumerate() {
        let module = l.module(index, input)?;
        program.modules.push(module);
    }
    program.defs = std::mem::take(&mut l.defs);
    for (i, local) in resolved.locals.iter().enumerate() {
        let id = LocalRef(u32::try_from(i).unwrap_or(u32::MAX));
        let ty = typed
            .local_types
            .get(&wolf_resolve::LocalId(id.0))
            .map(|t| l.ty(t))
            .unwrap_or(Type::Void);
        program.locals.insert(
            id,
            LocalInfo {
                name: local.name.clone(),
                ty,
            },
        );
    }
    Ok(program)
}

/// The std declarations that lowering treats specially.
#[derive(Default)]
struct Builtins {
    int: Option<DefId>,
    float: Option<DefId>,
    bool: Option<DefId>,
    string: Option<DefId>,
    void: Option<DefId>,
    never: Option<DefId>,
    list: Option<DefId>,
    map: Option<DefId>,
    option: Option<DefId>,
    result: Option<DefId>,
    some: Option<DefId>,
    none: Option<DefId>,
    ok: Option<DefId>,
    err: Option<DefId>,
    panic: Option<DefId>,
    assert: Option<DefId>,
}

struct Lowerer<'a> {
    inputs: &'a [ModuleInput<'a>],
    resolved: &'a Resolved,
    typed: &'a Typed,
    map: &'a SourceMap,
    b: Builtins,
    def_at: BTreeMap<Span, DefId>,
    defs: BTreeMap<DefRef, DefInfo>,
    /// The declaration of each function of [`wolf_types::Decls::methods`].
    method_defs: Vec<DefRef>,
    /// The method of a type by name.
    method_by_name: BTreeMap<(DefId, String), DefRef>,
    /// The method index by the span of its name.
    method_at: BTreeMap<Span, usize>,
    /// Type parameters of `impl` blocks renamed to those of their type.
    param_rename: BTreeMap<Span, String>,
}

/// Where code being lowered is.
#[derive(Clone, Copy)]
struct Ctx {
    file: FileId,
}

fn unsupported(message: impl Into<String>) -> LowerError {
    LowerError {
        message: message.into(),
    }
}

fn docs(docs: &[ast::Doc]) -> Vec<String> {
    docs.iter().map(|d| d.text.clone()).collect()
}

impl<'a> Lowerer<'a> {
    fn new(
        inputs: &'a [ModuleInput<'a>],
        resolved: &'a Resolved,
        typed: &'a Typed,
        map: &'a SourceMap,
    ) -> Lowerer<'a> {
        let get = |module: &str, name: &str| {
            resolved
                .modules
                .iter()
                .find(|m| m.path.join(".") == module)
                .and_then(|m| m.items.get(name).copied())
        };
        let member = |owner: Option<DefId>, name: &str| {
            owner.and_then(|o| resolved.members.get(&o).and_then(|m| m.get(name).copied()))
        };
        let option = get("std.prelude", "Option");
        let result = get("std.prelude", "Result");
        let b = Builtins {
            int: get("std.math", "Int"),
            float: get("std.math", "Float"),
            bool: get("std.prelude", "Bool"),
            string: get("std.string", "String"),
            void: get("std.prelude", "Void"),
            never: get("std.prelude", "Never"),
            list: get("std.list", "List"),
            map: get("std.map", "Map"),
            option,
            result,
            some: member(option, "Some"),
            none: member(option, "None"),
            ok: member(result, "Ok"),
            err: member(result, "Err"),
            panic: get("std.prelude", "panic"),
            assert: get("std.prelude", "assert"),
        };
        let def_at = resolved
            .defs
            .iter()
            .enumerate()
            .map(|(i, d)| (d.span, DefId(u32::try_from(i).unwrap_or(u32::MAX))))
            .collect();
        Lowerer {
            inputs,
            resolved,
            typed,
            map,
            b,
            def_at,
            defs: BTreeMap::new(),
            method_defs: Vec::new(),
            method_by_name: BTreeMap::new(),
            method_at: BTreeMap::new(),
            param_rename: BTreeMap::new(),
        }
    }

    fn dref(def: DefId) -> DefRef {
        DefRef(def.0)
    }

    fn res(&self, ctx: Ctx, id: ast::NodeId) -> Option<Res> {
        self.resolved.res.get(&(ctx.file, id)).copied()
    }

    fn expr_ty(&self, ctx: Ctx, id: ast::NodeId) -> Type {
        self.typed
            .expr_types
            .get(&(ctx.file, id))
            .map(|t| self.ty(t))
            .unwrap_or(Type::Void)
    }

    // ----- declarations -----------------------------------------------------------------------

    /// Builds the table of declarations, with ids for the functions of interface
    /// implementations, which have no declaration of their own in name resolution.
    fn defs(&mut self) {
        let mut intrinsic: BTreeMap<Span, String> = BTreeMap::new();
        for input in self.inputs {
            let prefix = input.path.join(".");
            for section in &input.ast.sections {
                for item in &section.items {
                    match &item.kind {
                        ItemKind::Fn(decl) if decl.intrinsic => {
                            intrinsic.insert(
                                decl.sig.name.span,
                                format!("{prefix}.{}", decl.sig.name.name),
                            );
                        }
                        ItemKind::IntrinsicType(decl) => {
                            intrinsic
                                .insert(decl.name.span, format!("{prefix}.{}", decl.name.name));
                        }
                        ItemKind::Impl(block) => {
                            for method in &block.methods {
                                if method.decl.intrinsic {
                                    intrinsic.insert(
                                        method.decl.sig.name.span,
                                        format!(
                                            "{prefix}.{}.{}",
                                            block.type_name.name, method.decl.sig.name.name
                                        ),
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        for (i, d) in self.resolved.defs.iter().enumerate() {
            let def = DefId(u32::try_from(i).unwrap_or(u32::MAX));
            let kind = match d.kind {
                RDef::Struct => DefKind::Struct,
                RDef::Enum => DefKind::Enum,
                RDef::Alias => continue,
                RDef::IntrinsicType => DefKind::BuiltinType,
                RDef::Interface => DefKind::Interface,
                RDef::Fn => DefKind::Fn,
                RDef::Config => DefKind::Const,
                RDef::ExternFn => DefKind::ExternFn,
                RDef::ExternType => DefKind::ExternType,
                RDef::Variant { parent, .. } => DefKind::Variant {
                    parent: Lowerer::dref(parent),
                },
                RDef::Member { owner, method } => DefKind::Method {
                    owner: Lowerer::dref(owner),
                    has_self: method,
                },
            };
            self.defs.insert(
                Lowerer::dref(def),
                DefInfo {
                    name: d.name.clone(),
                    module: d.module.0 as usize,
                    public: d.public,
                    kind,
                    intrinsic: intrinsic.get(&d.span).cloned(),
                },
            );
        }
        let mut next = u32::try_from(self.resolved.defs.len()).unwrap_or(u32::MAX);
        for (i, m) in self.typed.decls.methods.iter().enumerate() {
            self.method_at.insert(m.span, i);
            let def = match self.def_at.get(&m.span) {
                Some(def) if m.interface.is_none() => Lowerer::dref(*def),
                _ => {
                    let def = DefRef(next);
                    next = next.saturating_add(1);
                    self.defs.insert(
                        def,
                        DefInfo {
                            name: m.name.clone(),
                            module: m.module.0 as usize,
                            public: m.public,
                            kind: DefKind::Method {
                                owner: Lowerer::dref(m.owner),
                                has_self: m.sig.self_mode.is_some(),
                            },
                            intrinsic: intrinsic.get(&m.span).cloned(),
                        },
                    );
                    def
                }
            };
            self.method_defs.push(def);
            self.method_by_name.insert((m.owner, m.name.clone()), def);
            // Type parameters of the block take the names of the type's parameters.
            if let Some(generics) = self.typed.decls.type_generics.get(&m.owner) {
                for (span, g) in m.impl_generics.iter().zip(generics) {
                    if let Some(name) = self.typed.decls.param_names.get(&g.span) {
                        self.param_rename.insert(*span, name.clone());
                    }
                }
            }
        }
    }

    fn generics(&self, generics: &[wolf_types::Generic]) -> Vec<Generic> {
        generics
            .iter()
            .map(|g| Generic {
                name: self
                    .typed
                    .decls
                    .param_names
                    .get(&g.span)
                    .cloned()
                    .unwrap_or_default(),
                bounds: g.bounds.iter().map(|b| self.ty(b)).collect(),
            })
            .collect()
    }

    fn type_generics(&self, def: DefId) -> Vec<Generic> {
        self.typed
            .decls
            .type_generics
            .get(&def)
            .map(|g| self.generics(g))
            .unwrap_or_default()
    }

    /// The type of the checker as an IR type.
    fn ty(&self, t: &Ty) -> Type {
        match t {
            Ty::Named(def, args) => {
                let arg = |i: usize| Box::new(args.get(i).map_or(Type::Void, |a| self.ty(a)));
                let d = Some(*def);
                if d == self.b.int {
                    Type::Int
                } else if d == self.b.float {
                    Type::Float
                } else if d == self.b.bool {
                    Type::Bool
                } else if d == self.b.string {
                    Type::String
                } else if d == self.b.void {
                    Type::Void
                } else if d == self.b.never {
                    Type::Never
                } else if d == self.b.list {
                    Type::List(arg(0))
                } else if d == self.b.map {
                    Type::Map(arg(0), arg(1))
                } else if d == self.b.option {
                    Type::Option(arg(0))
                } else if d == self.b.result {
                    Type::Result(arg(0), arg(1))
                } else {
                    Type::Named(
                        Lowerer::dref(*def),
                        args.iter().map(|a| self.ty(a)).collect(),
                    )
                }
            }
            Ty::Fn(params, ret) => Type::Fn(
                params.iter().map(|(m, t)| (*m, self.ty(t))).collect(),
                Box::new(self.ty(ret)),
            ),
            Ty::Param(ParamId::Decl(span)) => Type::Param(
                self.param_rename
                    .get(span)
                    .or_else(|| self.typed.decls.param_names.get(span))
                    .cloned()
                    .unwrap_or_default(),
            ),
            Ty::Param(ParamId::SelfType) => Type::SelfType,
            Ty::Never => Type::Never,
            Ty::Var(_) | Ty::Error => Type::Void,
        }
    }

    fn module(&mut self, index: usize, input: &'a ModuleInput<'a>) -> Result<Module, LowerError> {
        let ctx = Ctx { file: input.file };
        let role = match input.role {
            Role::Prelude | Role::Std => ModuleRole::Std,
            Role::Entry => ModuleRole::Entry,
            Role::Src => ModuleRole::Src,
            Role::Test => ModuleRole::Test,
            Role::Example => ModuleRole::Example,
        };
        let file = self
            .map
            .get(input.file)
            .map(|f| f.name().to_string())
            .unwrap_or_default();
        // Functions of impl blocks, attached to their types.
        let mut methods: BTreeMap<DefId, Vec<Function>> = BTreeMap::new();
        let mut reexports = Vec::new();
        for (s, section) in input.ast.sections.iter().enumerate() {
            for (i, item) in section.items.iter().enumerate() {
                match &item.kind {
                    ItemKind::Impl(block) => {
                        let key = (wolf_resolve::ModuleId(index as u32), s, i);
                        let Some(owner) = self.resolved.impls.get(&key).copied() else {
                            continue;
                        };
                        for method in &block.methods {
                            if let Some(f) = self.method(ctx, &method.decl, &method.docs)? {
                                methods.entry(owner).or_default().push(f);
                            }
                        }
                    }
                    ItemKind::Import(import) if import.public => {
                        let path = import
                            .path
                            .segments
                            .iter()
                            .map(|s| s.name.as_str())
                            .collect::<Vec<_>>()
                            .join(".");
                        let target = self
                            .resolved
                            .modules
                            .iter()
                            .find(|m| m.path.join(".") == path);
                        if let (Some(target), Some(names)) = (target, &import.names) {
                            for n in names {
                                if let Some(def) = target.items.get(&n.name.name) {
                                    let name = n.alias.as_ref().unwrap_or(&n.name).name.clone();
                                    reexports.push(Reexport {
                                        name,
                                        def: Lowerer::dref(*def),
                                    });
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut items = Vec::new();
        for section in &input.ast.sections {
            for item in &section.items {
                self.item(ctx, item, &mut methods, &mut items)?;
            }
        }
        Ok(Module {
            path: input.path.clone(),
            file,
            role,
            docs: docs(&input.ast.docs),
            items,
            reexports,
        })
    }

    fn implements(&self, owner: DefId) -> Vec<Type> {
        let Some(list) = self.typed.decls.impls.get(&owner) else {
            return Vec::new();
        };
        list.iter()
            .map(|imp| {
                Type::Named(
                    Lowerer::dref(imp.interface),
                    imp.args.iter().map(|a| self.ty(a)).collect(),
                )
            })
            .collect()
    }

    fn item(
        &mut self,
        ctx: Ctx,
        item: &'a ast::Item,
        methods: &mut BTreeMap<DefId, Vec<Function>>,
        out: &mut Vec<Item>,
    ) -> Result<(), LowerError> {
        let def_of = |l: &Lowerer<'_>, name: &ast::Ident| l.def_at.get(&name.span).copied();
        match &item.kind {
            ItemKind::Struct(decl) => {
                let Some(def) = def_of(self, &decl.name) else {
                    return Ok(());
                };
                let fields = self
                    .typed
                    .decls
                    .fields
                    .get(&def)
                    .map(|fs| {
                        fs.iter()
                            .zip(&decl.fields)
                            .map(|(f, a)| Field {
                                name: f.name.clone(),
                                ty: self.ty(&f.ty),
                                public: f.public,
                                docs: docs(&a.docs),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                out.push(Item::Struct(Struct {
                    def: Lowerer::dref(def),
                    name: decl.name.name.clone(),
                    docs: docs(&item.docs),
                    public: decl.public,
                    generics: self.type_generics(def),
                    fields,
                    methods: methods.remove(&def).unwrap_or_default(),
                    implements: self.implements(def),
                }));
            }
            ItemKind::Enum(decl) => {
                let Some(def) = def_of(self, &decl.name) else {
                    return Ok(());
                };
                let mut variants = Vec::new();
                for v in &decl.variants {
                    let Some(vdef) = def_of(self, &v.name) else {
                        continue;
                    };
                    let payload = match self.typed.decls.variants.get(&vdef).map(|i| &i.shape) {
                        Some(Shape::Tuple(types)) => {
                            Payload::Tuple(types.iter().map(|t| self.ty(t)).collect())
                        }
                        Some(Shape::Struct(fields)) => Payload::Struct(
                            fields
                                .iter()
                                .map(|f| Field {
                                    name: f.name.clone(),
                                    ty: self.ty(&f.ty),
                                    public: true,
                                    docs: Vec::new(),
                                })
                                .collect(),
                        ),
                        _ => Payload::None,
                    };
                    variants.push(Variant {
                        def: Lowerer::dref(vdef),
                        name: v.name.name.clone(),
                        docs: docs(&v.docs),
                        payload,
                    });
                }
                out.push(Item::Enum(Enum {
                    def: Lowerer::dref(def),
                    name: decl.name.name.clone(),
                    docs: docs(&item.docs),
                    public: decl.public,
                    generics: self.type_generics(def),
                    variants,
                    methods: methods.remove(&def).unwrap_or_default(),
                    implements: self.implements(def),
                }));
            }
            ItemKind::IntrinsicType(decl) => {
                // Built-in types have no declaration in the output; their functions are intrinsic
                // except for the few with bodies in std.
                if let Some(def) = def_of(self, &decl.name) {
                    let _ = methods.remove(&def);
                }
            }
            ItemKind::Interface(decl) => {
                let Some(def) = def_of(self, &decl.name) else {
                    return Ok(());
                };
                let sigs = self.typed.decls.interface_methods.get(&def);
                let mut list = Vec::new();
                for m in &decl.methods {
                    let Some(sig) = sigs.and_then(|s| s.get(&m.sig.name.name)) else {
                        continue;
                    };
                    list.push(Function {
                        def: Lowerer::dref(def),
                        name: m.sig.name.name.clone(),
                        docs: docs(&m.docs),
                        public: decl.public,
                        generics: Vec::new(),
                        self_mode: sig.self_mode,
                        params: self.params(&m.sig, sig),
                        ret: self.ty(&sig.ret),
                        body: None,
                        interface: Some(Lowerer::dref(def)),
                    });
                }
                out.push(Item::Interface(Interface {
                    def: Lowerer::dref(def),
                    name: decl.name.name.clone(),
                    docs: docs(&item.docs),
                    public: decl.public,
                    generics: self.type_generics(def),
                    methods: list,
                }));
            }
            ItemKind::Fn(decl) => {
                let Some(def) = def_of(self, &decl.sig.name) else {
                    return Ok(());
                };
                let Some(sig) = self.typed.decls.fns.get(&def) else {
                    return Ok(());
                };
                let body = match &decl.body {
                    Some(b) => Some(self.block(ctx, b, matches!(self.ty(&sig.ret), Type::Void))?),
                    None => None,
                };
                out.push(Item::Fn(Function {
                    def: Lowerer::dref(def),
                    name: decl.sig.name.name.clone(),
                    docs: docs(&item.docs),
                    public: decl.public,
                    generics: self.generics(&sig.generics),
                    self_mode: None,
                    params: self.params(&decl.sig, sig),
                    ret: self.ty(&sig.ret),
                    body,
                    interface: None,
                }));
            }
            ItemKind::Config(entry) => {
                let Some(def) = def_of(self, &entry.name) else {
                    return Ok(());
                };
                let ty = self
                    .typed
                    .decls
                    .consts
                    .get(&def)
                    .map(|t| self.ty(t))
                    .unwrap_or(Type::Void);
                let value = self.typed.consts.get(&def).cloned().ok_or_else(|| {
                    unsupported(format!("constant `{}` has no value", entry.name.name))
                })?;
                out.push(Item::Const(Const {
                    def: Lowerer::dref(def),
                    name: entry.name.name.clone(),
                    docs: docs(&item.docs),
                    public: entry.public,
                    ty,
                    value,
                }));
            }
            ItemKind::ExternTarget(target) => {
                for decl in &target.decls {
                    let name = match &decl.kind {
                        ast::ExternDeclKind::Fn(sig) => &sig.name,
                        ast::ExternDeclKind::Type(name) => name,
                    };
                    // One declaration may be written for several targets: find it by name.
                    let def = self.resolved.defs.iter().position(|d| {
                        d.name == name.name
                            && matches!(d.kind, RDef::ExternFn | RDef::ExternType)
                            && self
                                .resolved
                                .modules
                                .get(d.module.0 as usize)
                                .map(|m| m.file)
                                == Some(ctx.file)
                    });
                    let Some(def) = def.map(|i| DefRef(u32::try_from(i).unwrap_or(u32::MAX)))
                    else {
                        continue;
                    };
                    let existing = out.iter_mut().find_map(|it| match it {
                        Item::ExternFn(f) if f.def == def => Some(&mut f.paths),
                        Item::ExternType(t) if t.def == def => Some(&mut t.paths),
                        _ => None,
                    });
                    if let Some(paths) = existing {
                        paths.insert(target.target.name.clone(), decl.from.value.clone());
                        continue;
                    }
                    let mut paths = BTreeMap::new();
                    paths.insert(target.target.name.clone(), decl.from.value.clone());
                    match &decl.kind {
                        ast::ExternDeclKind::Fn(_) => {
                            let Some(sig) = self.typed.decls.fns.get(&DefId(def.0)) else {
                                continue;
                            };
                            out.push(Item::ExternFn(ExternFn {
                                def,
                                name: name.name.clone(),
                                public: decl.public,
                                params: sig.params.iter().map(|(m, t)| (*m, self.ty(t))).collect(),
                                ret: self.ty(&sig.ret),
                                paths,
                            }));
                        }
                        ast::ExternDeclKind::Type(_) => out.push(Item::ExternType(ExternType {
                            def,
                            name: name.name.clone(),
                            paths,
                        })),
                    }
                }
            }
            ItemKind::Test(test) => {
                out.push(Item::Test(Test {
                    name: test.name.value.clone(),
                    body: self.block(ctx, &test.body, true)?,
                }));
            }
            ItemKind::Alias(_) | ItemKind::Impl(_) | ItemKind::Import(_) => {}
        }
        Ok(())
    }

    fn params(&self, ast_sig: &ast::FnSig, sig: &wolf_types::Sig) -> Vec<Param> {
        let mut out = Vec::new();
        let mut types = sig.params.iter();
        for p in &ast_sig.params {
            if let ast::ParamKind::Named { name, .. } = &p.kind {
                let ty = types.next().map(|(_, t)| self.ty(t)).unwrap_or(Type::Void);
                let local = self
                    .resolved
                    .local_decls
                    .get(&name.span)
                    .map(|l| LocalRef(l.0))
                    .unwrap_or(LocalRef(u32::MAX));
                out.push(Param {
                    local,
                    name: name.name.clone(),
                    mode: p.mode,
                    ty,
                });
            }
        }
        out
    }

    fn method(
        &mut self,
        ctx: Ctx,
        decl: &'a ast::FnDecl,
        method_docs: &[ast::Doc],
    ) -> Result<Option<Function>, LowerError> {
        let Some(index) = self.method_at.get(&decl.sig.name.span).copied() else {
            return Ok(None);
        };
        let (Some(m), Some(def)) = (
            self.typed.decls.methods.get(index),
            self.method_defs.get(index).copied(),
        ) else {
            return Ok(None);
        };
        let ret = self.ty(&m.sig.ret);
        let body = match &decl.body {
            Some(b) => Some(self.block(ctx, b, matches!(ret, Type::Void))?),
            None => None,
        };
        Ok(Some(Function {
            def,
            name: m.name.clone(),
            docs: docs(method_docs),
            public: m.public,
            generics: self.generics(&m.sig.generics),
            self_mode: m.sig.self_mode,
            params: self.params(&decl.sig, &m.sig),
            ret,
            body,
            interface: m.interface.map(Lowerer::dref),
        }))
    }

    // ----- statements -------------------------------------------------------------------------

    /// A block; with `discard`, its final expression is a statement (chapter 4, rule 6.4).
    fn block(
        &mut self,
        ctx: Ctx,
        block: &'a ast::Block,
        discard: bool,
    ) -> Result<Block, LowerError> {
        let mut out = Block::default();
        let n = block.stmts.len();
        for (i, stmt) in block.stmts.iter().enumerate() {
            if i + 1 == n && !discard {
                if let StmtKind::Expr(e) = &stmt.kind {
                    out.tail = Some(Box::new(self.expr(ctx, e)?));
                    continue;
                }
            }
            out.stmts.push(self.stmt(ctx, stmt)?);
        }
        Ok(out)
    }

    /// An expression as a block (match arms, `else if`).
    fn expr_block(
        &mut self,
        ctx: Ctx,
        e: &'a ast::Expr,
        discard: bool,
    ) -> Result<Block, LowerError> {
        if let AstExpr::Block(block) = &e.kind {
            return self.block(ctx, block, discard);
        }
        let value = self.expr(ctx, e)?;
        Ok(if discard {
            Block {
                stmts: vec![Stmt::Expr(value)],
                tail: None,
            }
        } else {
            Block {
                stmts: Vec::new(),
                tail: Some(Box::new(value)),
            }
        })
    }

    fn local(&self, name: &ast::Ident) -> LocalRef {
        self.resolved
            .local_decls
            .get(&name.span)
            .map(|l| LocalRef(l.0))
            .unwrap_or(LocalRef(u32::MAX))
    }

    fn stmt(&mut self, ctx: Ctx, stmt: &'a ast::Stmt) -> Result<Stmt, LowerError> {
        Ok(match &stmt.kind {
            StmtKind::Let {
                mutable,
                name,
                value,
                ..
            } => {
                let value = self.expr(ctx, value)?;
                match name {
                    Some(name) => Stmt::Let {
                        local: self.local(name),
                        mutable: *mutable,
                        value,
                    },
                    None => Stmt::Expr(value),
                }
            }
            StmtKind::Assign { target, op, value } => Stmt::Assign {
                target: self.expr(ctx, target)?,
                op: assign_op(*op),
                value: self.expr(ctx, value)?,
            },
            StmtKind::While { cond, body } => Stmt::While {
                cond: self.expr(ctx, cond)?,
                body: self.block(ctx, body, true)?,
            },
            StmtKind::For { var, iter, body } => match iter {
                ast::ForIter::Collection { mode, expr } => Stmt::ForList {
                    var: self.local(var),
                    mode: *mode,
                    list: self.expr(ctx, expr)?,
                    body: self.block(ctx, body, true)?,
                },
                ast::ForIter::Range {
                    start,
                    end,
                    inclusive,
                } => Stmt::ForRange {
                    var: self.local(var),
                    start: self.expr(ctx, start)?,
                    end: self.expr(ctx, end)?,
                    inclusive: *inclusive,
                    body: self.block(ctx, body, true)?,
                },
            },
            StmtKind::Expr(e) => Stmt::Expr(self.expr(ctx, e)?),
        })
    }

    // ----- expressions ------------------------------------------------------------------------

    /// An expression, with the interface coercion recorded for it.
    fn expr(&mut self, ctx: Ctx, e: &'a ast::Expr) -> Result<Expr, LowerError> {
        let value = self.expr_inner(ctx, e)?;
        Ok(self.coerced(ctx, e.id, value))
    }

    fn coerced(&self, ctx: Ctx, id: ast::NodeId, value: Expr) -> Expr {
        match self.typed.coercions.get(&(ctx.file, id)) {
            Some(to) => {
                let to = self.ty(to);
                Expr {
                    ty: to.clone(),
                    kind: ExprKind::Coerce {
                        value: Box::new(value),
                        to,
                    },
                }
            }
            None => value,
        }
    }

    fn boxed(&mut self, ctx: Ctx, e: &'a ast::Expr) -> Result<Box<Expr>, LowerError> {
        Ok(Box::new(self.expr(ctx, e)?))
    }

    fn expr_inner(&mut self, ctx: Ctx, e: &'a ast::Expr) -> Result<Expr, LowerError> {
        let ty = self.expr_ty(ctx, e.id);
        let kind = match &e.kind {
            AstExpr::Int(v) => ExprKind::Int(
                i64::try_from(*v).map_err(|_| unsupported("integer literal out of range"))?,
            ),
            AstExpr::Float(v) => ExprKind::Float(*v),
            AstExpr::Str(s) => ExprKind::Str(s.clone()),
            AstExpr::Bool(v) => ExprKind::Bool(*v),
            AstExpr::Unit => ExprKind::Unit,
            AstExpr::SelfValue => ExprKind::SelfValue,
            AstExpr::Paren(inner) => return self.expr(ctx, inner),
            AstExpr::Name(_) | AstExpr::Field { .. } | AstExpr::TypeArgs { .. }
                if self.path_res(ctx, e).is_some() =>
            {
                let res = self.path_res(ctx, e).ok_or_else(|| unsupported("path"))?;
                self.path(res)?
            }
            AstExpr::Name(name) => {
                return Err(unsupported(format!("unresolved name `{}`", name.name)));
            }
            AstExpr::SelfType => return Err(unsupported("`Self` as a value")),
            AstExpr::Field { base, name } => ExprKind::Field {
                base: self.boxed(ctx, base)?,
                name: name.name.clone(),
            },
            AstExpr::TypeArgs { base, .. } => return self.expr(ctx, base),
            AstExpr::List(items) => ExprKind::List(
                items
                    .iter()
                    .map(|i| self.expr(ctx, i))
                    .collect::<Result<_, _>>()?,
            ),
            AstExpr::Map(entries) => ExprKind::Map(
                entries
                    .iter()
                    .map(|(k, v)| Ok((self.expr(ctx, k)?, self.expr(ctx, v)?)))
                    .collect::<Result<_, LowerError>>()?,
            ),
            AstExpr::StructLit { fields, .. } => self.struct_lit(ctx, e, fields)?,
            AstExpr::Call { callee, args } => self.call(ctx, e, callee, args)?,
            AstExpr::Index { base, index } => ExprKind::Index {
                base: self.boxed(ctx, base)?,
                index: self.boxed(ctx, index)?,
            },
            AstExpr::Slice {
                base,
                start,
                end,
                inclusive,
            } => ExprKind::Slice {
                base: self.boxed(ctx, base)?,
                start: match start {
                    Some(s) => Some(self.boxed(ctx, s)?),
                    None => None,
                },
                end: match end {
                    Some(s) => Some(self.boxed(ctx, s)?),
                    None => None,
                },
                inclusive: *inclusive,
            },
            AstExpr::Try(inner) => ExprKind::Try {
                value: self.boxed(ctx, inner)?,
                coerce_error: self
                    .typed
                    .try_coercions
                    .get(&(ctx.file, e.id))
                    .map(|t| self.ty(t)),
            },
            AstExpr::Unary {
                op: ast::UnaryOp::Neg,
                operand,
            } if matches!(operand.kind, AstExpr::Int(_)) => {
                let AstExpr::Int(v) = operand.kind else {
                    return Err(unsupported("integer literal"));
                };
                // `-9223372036854775808` is the smallest `Int`.
                ExprKind::Int(
                    i64::try_from(-i128::from(v))
                        .map_err(|_| unsupported("integer literal out of range"))?,
                )
            }
            AstExpr::Unary { op, operand } => ExprKind::Unary {
                op: *op,
                operand: self.boxed(ctx, operand)?,
            },
            AstExpr::Binary { op, lhs, rhs } => ExprKind::Binary {
                op: *op,
                lhs: self.boxed(ctx, lhs)?,
                rhs: self.boxed(ctx, rhs)?,
            },
            AstExpr::Closure { params, body, .. } => {
                let ret_void = matches!(&ty, Type::Fn(_, r) if matches!(**r, Type::Void));
                let modes: Vec<Mode> = match &ty {
                    Type::Fn(ps, _) => ps.iter().map(|(m, _)| *m).collect(),
                    _ => Vec::new(),
                };
                let mut list = Vec::new();
                for (i, p) in params.iter().enumerate() {
                    let local = self.local(&p.name);
                    let pty = self
                        .typed
                        .local_types
                        .get(&wolf_resolve::LocalId(local.0))
                        .map(|t| self.ty(t))
                        .unwrap_or(Type::Void);
                    list.push(Param {
                        local,
                        name: p.name.name.clone(),
                        mode: modes.get(i).copied().unwrap_or(Mode::Read),
                        ty: pty,
                    });
                }
                ExprKind::Closure {
                    params: list,
                    body: self.expr_block(ctx, body, ret_void)?,
                }
            }
            AstExpr::Block(block) => {
                ExprKind::Block(self.block(ctx, block, matches!(ty, Type::Void))?)
            }
            AstExpr::If {
                cond,
                then,
                otherwise,
            } => {
                let discard = matches!(ty, Type::Void);
                ExprKind::If {
                    cond: self.boxed(ctx, cond)?,
                    then: self.block(ctx, then, discard)?,
                    otherwise: match otherwise {
                        Some(o) => Some(self.expr_block(ctx, o, discard)?),
                        None => None,
                    },
                }
            }
            AstExpr::Match {
                mode,
                scrutinee,
                arms,
            } => {
                let discard = matches!(ty, Type::Void);
                let mut list = Vec::new();
                for arm in arms {
                    list.push(Arm {
                        pattern: self.pattern(ctx, &arm.pattern)?,
                        guard: match &arm.guard {
                            Some(g) => Some(self.expr(ctx, g)?),
                            None => None,
                        },
                        body: self.expr_block(ctx, &arm.body, discard)?,
                    });
                }
                ExprKind::Match {
                    scrutinee: self.boxed(ctx, scrutinee)?,
                    mode: *mode,
                    arms: list,
                }
            }
            AstExpr::Return(value) => ExprKind::Return(match value {
                Some(v) => Some(self.boxed(ctx, v)?),
                None => None,
            }),
            AstExpr::Break => ExprKind::Break,
            AstExpr::Continue => ExprKind::Continue,
            AstExpr::Error => return Err(unsupported("an expression with an error")),
        };
        Ok(Expr { kind, ty })
    }

    /// What a name or a dotted path names, if it is a path rather than a field access.
    fn path_res(&self, ctx: Ctx, e: &ast::Expr) -> Option<Res> {
        match &e.kind {
            AstExpr::TypeArgs { base, .. } => self.path_res(ctx, base),
            _ => self.res(ctx, e.id),
        }
    }

    fn path(&self, res: Res) -> Result<ExprKind, LowerError> {
        Ok(match res {
            Res::Local(id) => ExprKind::Local(LocalRef(id.0)),
            Res::Def(def) => {
                let d = self
                    .resolved
                    .def(def)
                    .ok_or_else(|| unsupported("unknown declaration"))?;
                match d.kind {
                    RDef::Fn | RDef::ExternFn | RDef::Member { .. } => {
                        ExprKind::FnValue(Lowerer::dref(def))
                    }
                    RDef::Config => ExprKind::Const(Lowerer::dref(def)),
                    RDef::Variant { .. } => {
                        if Some(def) == self.b.none {
                            ExprKind::None
                        } else {
                            ExprKind::Variant {
                                def: Lowerer::dref(def),
                                values: Vec::new(),
                            }
                        }
                    }
                    _ => return Err(unsupported(format!("`{}` used as a value", d.name))),
                }
            }
            Res::Module(_) | Res::TypeParam(_) | Res::SelfType => {
                return Err(unsupported("a module or type used as a value"));
            }
        })
    }

    fn struct_lit(
        &mut self,
        ctx: Ctx,
        e: &'a ast::Expr,
        fields: &'a [ast::FieldInit],
    ) -> Result<ExprKind, LowerError> {
        let Some(Res::Def(def)) = self.res(ctx, e.id) else {
            return Err(unsupported("struct literal without a type"));
        };
        let names: Vec<String> = match self.resolved.def(def).map(|d| d.kind) {
            Some(RDef::Struct) => self
                .typed
                .decls
                .fields
                .get(&def)
                .map(|f| f.iter().map(|f| f.name.clone()).collect())
                .unwrap_or_default(),
            Some(RDef::Variant { .. }) => {
                match self.typed.decls.variants.get(&def).map(|v| &v.shape) {
                    Some(Shape::Struct(f)) => f.iter().map(|f| f.name.clone()).collect(),
                    _ => Vec::new(),
                }
            }
            _ => return Err(unsupported("struct literal of an unknown type")),
        };
        let mut values: BTreeMap<String, Expr> = BTreeMap::new();
        for f in fields {
            let value = match &f.value {
                Some(v) => self.expr(ctx, v)?,
                None => {
                    let res = self
                        .res(ctx, f.id)
                        .ok_or_else(|| unsupported("unresolved shorthand field"))?;
                    let ty = match res {
                        Res::Local(id) => self
                            .typed
                            .local_types
                            .get(&id)
                            .map(|t| self.ty(t))
                            .unwrap_or(Type::Void),
                        Res::Def(d) => self
                            .typed
                            .decls
                            .consts
                            .get(&d)
                            .map(|t| self.ty(t))
                            .unwrap_or(Type::Void),
                        _ => Type::Void,
                    };
                    let value = Expr {
                        kind: self.path(res)?,
                        ty,
                    };
                    self.coerced(ctx, f.id, value)
                }
            };
            values.insert(f.name.name.clone(), value);
        }
        let ordered: Vec<(String, Expr)> = names
            .into_iter()
            .filter_map(|n| values.remove(&n).map(|v| (n, v)))
            .collect();
        Ok(match self.resolved.def(def).map(|d| d.kind) {
            Some(RDef::Struct) => ExprKind::Struct {
                def: Lowerer::dref(def),
                fields: ordered,
            },
            _ => ExprKind::Variant {
                def: Lowerer::dref(def),
                values: ordered.into_iter().map(|(_, v)| v).collect(),
            },
        })
    }

    fn args(&mut self, ctx: Ctx, args: &'a [ast::Arg]) -> Result<Vec<Arg>, LowerError> {
        args.iter()
            .map(|a| {
                Ok(Arg {
                    mode: a.mode,
                    value: self.expr(ctx, &a.value)?,
                })
            })
            .collect()
    }

    fn location(&self, span: Span) -> String {
        match self.map.get(span.file) {
            Some(file) => {
                let lc = file.line_col(span.start);
                format!("{}:{}:{}", file.name(), lc.line, lc.column)
            }
            None => String::new(),
        }
    }

    fn source_text(&self, span: Span) -> String {
        self.map
            .get(span.file)
            .and_then(|f| f.text().get(span.range()))
            .unwrap_or_default()
            .to_string()
    }

    fn call(
        &mut self,
        ctx: Ctx,
        e: &'a ast::Expr,
        callee: &'a ast::Expr,
        args: &'a [ast::Arg],
    ) -> Result<ExprKind, LowerError> {
        // A method call: `recv.m(...)` where `recv` is a value.
        if let AstExpr::Field { base, name } = &callee.kind {
            if self.res(ctx, callee.id).is_none() {
                let recv = self.boxed(ctx, base)?;
                let lowered = self.args(ctx, args)?;
                return match self.typed.method_calls.get(&(ctx.file, e.id)) {
                    Some(call) => {
                        let owner_kind = self.resolved.def(call.owner).map(|d| d.kind);
                        if owner_kind == Some(RDef::Interface) {
                            Ok(ExprKind::Call {
                                callee: Callee::Interface {
                                    interface: Lowerer::dref(call.owner),
                                    name: call.name.clone(),
                                    recv,
                                },
                                args: lowered,
                            })
                        } else {
                            let method = self
                                .method_by_name
                                .get(&(call.owner, call.name.clone()))
                                .copied()
                                .ok_or_else(|| {
                                    unsupported(format!("method `{}` not found", call.name))
                                })?;
                            Ok(ExprKind::Call {
                                callee: Callee::Method { method, recv },
                                args: lowered,
                            })
                        }
                    }
                    None if name.name == "copy" && args.is_empty() => Ok(ExprKind::Copy(recv)),
                    None => Err(unsupported(format!(
                        "call of `.{}` through a type alias or an interface type",
                        name.name
                    ))),
                };
            }
        }
        if let Some(Res::Def(def)) = self.path_res(ctx, callee) {
            if Some(def) == self.b.panic {
                let message = args
                    .first()
                    .ok_or_else(|| unsupported("panic without a message"))?;
                return Ok(ExprKind::Panic {
                    message: self.boxed(ctx, &message.value)?,
                    location: self.location(e.span),
                });
            }
            if Some(def) == self.b.assert {
                let cond = args
                    .first()
                    .ok_or_else(|| unsupported("assert without a condition"))?;
                return Ok(ExprKind::Assert {
                    cond: self.boxed(ctx, &cond.value)?,
                    text: self.source_text(cond.value.span),
                });
            }
            match self.resolved.def(def).map(|d| d.kind) {
                Some(RDef::Variant { .. }) => {
                    let mut values: Vec<Expr> =
                        self.args(ctx, args)?.into_iter().map(|a| a.value).collect();
                    let d = Some(def);
                    let single = |values: &mut Vec<Expr>| -> Result<Box<Expr>, LowerError> {
                        values
                            .pop()
                            .map(Box::new)
                            .ok_or_else(|| unsupported("variant without a payload value"))
                    };
                    return Ok(if d == self.b.some {
                        ExprKind::Some(single(&mut values)?)
                    } else if d == self.b.ok {
                        ExprKind::Ok(single(&mut values)?)
                    } else if d == self.b.err {
                        ExprKind::Err(single(&mut values)?)
                    } else {
                        ExprKind::Variant {
                            def: Lowerer::dref(def),
                            values,
                        }
                    });
                }
                Some(RDef::Fn | RDef::ExternFn | RDef::Member { .. }) => {
                    return Ok(ExprKind::Call {
                        callee: Callee::Fn(Lowerer::dref(def)),
                        args: self.args(ctx, args)?,
                    });
                }
                _ => {}
            }
        }
        Ok(ExprKind::Call {
            callee: Callee::Value(self.boxed(ctx, callee)?),
            args: self.args(ctx, args)?,
        })
    }

    // ----- patterns ---------------------------------------------------------------------------

    fn pattern(&self, ctx: Ctx, pat: &ast::Pat) -> Result<Pattern, LowerError> {
        let res = self.res(ctx, pat.id);
        Ok(match &pat.kind {
            PatKind::Wild => Pattern::Wild,
            PatKind::Error => return Err(unsupported("a pattern with an error")),
            PatKind::Name(_) | PatKind::Path(_) => match res {
                Some(Res::Local(id)) => Pattern::Bind(LocalRef(id.0)),
                Some(Res::Def(def)) => match self.resolved.def(def).map(|d| d.kind) {
                    Some(RDef::Config) => Pattern::Const(Lowerer::dref(def)),
                    Some(RDef::Variant { .. }) if Some(def) == self.b.none => Pattern::None,
                    Some(RDef::Variant { .. }) => Pattern::Variant {
                        def: Lowerer::dref(def),
                        fields: Vec::new(),
                    },
                    _ => return Err(unsupported("pattern path")),
                },
                _ => return Err(unsupported("unresolved pattern")),
            },
            PatKind::Int { negative, value } => {
                let v = i128::from(*value);
                let v = if *negative { -v } else { v };
                Pattern::Int(i64::try_from(v).map_err(|_| unsupported("pattern out of range"))?)
            }
            PatKind::Str(s) => Pattern::Str(s.clone()),
            PatKind::Bool(v) => Pattern::Bool(*v),
            PatKind::Tuple { items, .. } => {
                let Some(Res::Def(def)) = res else {
                    return Err(unsupported("unresolved pattern"));
                };
                let mut fields: Vec<Pattern> = items
                    .iter()
                    .map(|i| self.pattern(ctx, i))
                    .collect::<Result<_, _>>()?;
                let d = Some(def);
                if d == self.b.some || d == self.b.ok || d == self.b.err {
                    let inner = Box::new(fields.pop().unwrap_or(Pattern::Wild));
                    if d == self.b.some {
                        Pattern::Some(inner)
                    } else if d == self.b.ok {
                        Pattern::Ok(inner)
                    } else {
                        Pattern::Err(inner)
                    }
                } else {
                    Pattern::Variant {
                        def: Lowerer::dref(def),
                        fields,
                    }
                }
            }
            PatKind::Struct { fields, .. } => {
                let Some(Res::Def(def)) = res else {
                    return Err(unsupported("unresolved pattern"));
                };
                let (names, is_struct): (Vec<String>, bool) =
                    match self.resolved.def(def).map(|d| d.kind) {
                        Some(RDef::Struct) => (
                            self.typed
                                .decls
                                .fields
                                .get(&def)
                                .map(|f| f.iter().map(|f| f.name.clone()).collect())
                                .unwrap_or_default(),
                            true,
                        ),
                        _ => (
                            match self.typed.decls.variants.get(&def).map(|v| &v.shape) {
                                Some(Shape::Struct(f)) => {
                                    f.iter().map(|f| f.name.clone()).collect()
                                }
                                _ => Vec::new(),
                            },
                            false,
                        ),
                    };
                let mut out = Vec::new();
                for n in names {
                    let p = match fields.iter().find(|f| f.name.name == n) {
                        Some(f) => match &f.pat {
                            Some(sub) => self.pattern(ctx, sub)?,
                            None => match self.res(ctx, f.id) {
                                Some(Res::Local(id)) => Pattern::Bind(LocalRef(id.0)),
                                _ => Pattern::Wild,
                            },
                        },
                        None => Pattern::Wild,
                    };
                    out.push(p);
                }
                if is_struct {
                    Pattern::Struct {
                        def: Lowerer::dref(def),
                        fields: out,
                    }
                } else {
                    Pattern::Variant {
                        def: Lowerer::dref(def),
                        fields: out,
                    }
                }
            }
            PatKind::Or(alts) => Pattern::Or(
                alts.iter()
                    .map(|a| self.pattern(ctx, a))
                    .collect::<Result<_, _>>()?,
            ),
        })
    }
}

fn assign_op(op: ast::AssignOp) -> Option<BinaryOp> {
    use ast::AssignOp as A;
    Some(match op {
        A::Assign => return None,
        A::Add => BinaryOp::Add,
        A::Sub => BinaryOp::Sub,
        A::Mul => BinaryOp::Mul,
        A::Div => BinaryOp::Div,
        A::Rem => BinaryOp::Rem,
        A::BitAnd => BinaryOp::BitAnd,
        A::BitOr => BinaryOp::BitOr,
        A::BitXor => BinaryOp::BitXor,
        A::Shl => BinaryOp::Shl,
        A::Shr => BinaryOp::Shr,
    })
}
