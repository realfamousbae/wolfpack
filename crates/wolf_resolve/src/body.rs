//! Pass 2: names in types, expressions and patterns, with local scopes.

use std::collections::BTreeMap;

use wolf_ast::*;
use wolf_diag::{Diagnostic, codes};
use wolf_span::Span;

use crate::collect::{Binding, Resolver, join};
use crate::names::suggest;
use crate::{DefId, DefKind, Local, LocalId, ModuleId, Payload, Res};

/// Where the code being resolved is.
#[derive(Clone)]
struct Ctx {
    module: ModuleId,
    in_tests: bool,
    /// Type parameters in scope with their declaring spans, innermost last.
    generics: Vec<(String, Span)>,
    /// `Self`: `Some(None)` inside an interface, `Some(Some(owner))` inside an impl block.
    self_type: Option<Option<DefId>>,
    /// Whether `self` is a parameter of the current function.
    self_value: bool,
}

/// One block scope.
#[derive(Default)]
struct Scope {
    names: BTreeMap<String, LocalId>,
}

/// What the left part of a dotted expression names.
#[derive(Clone, Copy)]
enum Head {
    Module(ModuleId),
    Type(DefId),
    /// `Self` in an interface, or an unresolved type: members are not checked.
    UnknownType,
    Value,
}

/// How a local was introduced, for the E0618 rule and its hint.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `let`
    Let,
    /// `var`
    Var,
    /// A binding in a pattern.
    Pattern,
    /// Parameters and loop variables: exempt from E0618.
    Exempt,
}

pub(crate) struct Locals {
    scopes: Vec<Scope>,
    /// How each local was introduced, and whether it was used.
    kinds: Vec<Kind>,
    used: Vec<bool>,
    /// The labels of the enclosing loops of the current body, innermost last (chapter 3,
    /// section 4, rule 5).
    labels: Vec<Ident>,
    /// Inside the pattern of a destructuring `let`: a lone name is always a binding.
    let_pattern: bool,
}

impl Resolver<'_> {
    pub(crate) fn resolve_bodies(&mut self) {
        let inputs = self.inputs;
        for (index, input) in inputs.iter().enumerate() {
            let module = ModuleId(index as u32);
            for (s, section) in input.ast.sections.iter().enumerate() {
                let in_tests = section.kind == SectionKind::Tests;
                for (i, item) in section.items.iter().enumerate() {
                    let ctx = Ctx {
                        module,
                        in_tests,
                        generics: Vec::new(),
                        self_type: None,
                        self_value: false,
                    };
                    let owner = self.impl_owners.get(&(index, s, i)).copied();
                    self.item(&ctx, item, owner);
                }
            }
            self.unused_imports(module);
        }
        crate::checks::run(self);
    }

    fn item(&mut self, ctx: &Ctx, item: &Item, impl_owner: Option<DefId>) {
        let mut locals = Locals {
            scopes: Vec::new(),
            kinds: Vec::new(),
            used: Vec::new(),
            labels: Vec::new(),
            let_pattern: false,
        };
        match &item.kind {
            ItemKind::Struct(decl) => {
                let ctx = self.with_generics(ctx, &decl.generics);
                for field in &decl.fields {
                    self.ty(&ctx, &field.ty);
                }
            }
            ItemKind::Enum(decl) => {
                let ctx = self.with_generics(ctx, &decl.generics);
                for variant in &decl.variants {
                    match &variant.payload {
                        VariantPayload::None => {}
                        VariantPayload::Tuple(types) => {
                            for t in types {
                                self.ty(&ctx, t);
                            }
                        }
                        VariantPayload::Struct(fields) => {
                            for f in fields {
                                self.ty(&ctx, &f.ty);
                            }
                        }
                    }
                }
            }
            ItemKind::Alias(decl) => {
                let ctx = self.with_generics(ctx, &decl.generics);
                self.ty(&ctx, &decl.ty);
            }
            ItemKind::IntrinsicType(decl) => {
                let _ = self.with_generics(ctx, &decl.generics);
            }
            ItemKind::Interface(decl) => {
                let mut ctx = self.with_generics(ctx, &decl.generics);
                ctx.self_type = Some(None);
                for method in &decl.methods {
                    self.signature(&ctx, &method.sig, &mut locals);
                    locals.scopes.clear();
                }
            }
            ItemKind::Impl(block) => {
                let mut ctx = self.with_generics(ctx, &block.generics);
                if impl_owner.is_none() {
                    self.unknown_type(&ctx, &block.type_name);
                }
                ctx.self_type = Some(impl_owner);
                if let Some(interface) = &block.interface {
                    self.ty(&ctx, interface);
                }
                for method in &block.methods {
                    self.function(&ctx, &method.decl, &mut locals);
                }
            }
            ItemKind::Fn(decl) => self.function(ctx, decl, &mut locals),
            ItemKind::Config(entry) => {
                self.ty(ctx, &entry.ty);
                self.expr(ctx, &entry.value, &mut locals);
            }
            ItemKind::ExternTarget(target) => {
                for decl in &target.decls {
                    if let ExternDeclKind::Fn(sig) = &decl.kind {
                        self.signature(ctx, sig, &mut locals);
                        locals.scopes.clear();
                    }
                }
            }
            ItemKind::Test(test) => {
                self.block(ctx, &test.body, &mut locals, false);
            }
            ItemKind::Import(_) => {}
        }
    }

    fn with_generics(&mut self, ctx: &Ctx, generics: &[GenericParam]) -> Ctx {
        let mut ctx = ctx.clone();
        let mut seen: BTreeMap<String, Span> = BTreeMap::new();
        for param in generics {
            self.style(&param.name, true, "type parameter");
            if let Some(first) = seen.get(&param.name.name) {
                self.report(
                    Diagnostic::new(
                        codes::E0605,
                        format!("type parameter `{}` is declared twice", param.name.name),
                    )
                    .primary(param.name.span, "second declaration")
                    .secondary(*first, "first declaration"),
                );
            }
            seen.insert(param.name.name.clone(), param.name.span);
            ctx.generics
                .push((param.name.name.clone(), param.name.span));
        }
        for param in generics {
            for bound in &param.bounds {
                self.ty(&ctx, bound);
            }
        }
        ctx
    }

    /// Parameters and result of a signature, with parameters declared in a new scope.
    fn signature(&mut self, ctx: &Ctx, sig: &FnSig, locals: &mut Locals) -> Ctx {
        let mut ctx = self.with_generics(ctx, &sig.generics);
        locals.scopes.push(Scope::default());
        for param in &sig.params {
            match &param.kind {
                ParamKind::SelfParam => ctx.self_value = true,
                ParamKind::Named { name, ty } => {
                    self.ty(&ctx, ty);
                    self.style(name, false, "parameter");
                    // Unused parameters are allowed (chapter 9, section 2).
                    self.declare(name, locals, Kind::Exempt);
                }
            }
        }
        for param in &sig.where_bounds {
            for bound in &param.bounds {
                self.ty(&ctx, bound);
            }
        }
        if let Some(ret) = &sig.ret {
            self.ty(&ctx, ret);
        }
        ctx
    }

    fn function(&mut self, ctx: &Ctx, decl: &FnDecl, locals: &mut Locals) {
        let ctx = self.signature(ctx, &decl.sig, locals);
        if let Some(body) = &decl.body {
            // The parameters and the top block of the body are one scope (chapter 6, 3.2).
            self.block(&ctx, body, locals, true);
        }
        self.pop_scope(locals);
    }

    // ----- locals ---------------------------------------------------------------------------

    fn declare(&mut self, name: &Ident, locals: &mut Locals, kind: Kind) -> LocalId {
        let id = LocalId(u32::try_from(self.locals.len()).unwrap_or(u32::MAX));
        self.locals.push(Local {
            name: name.name.clone(),
            span: name.span,
        });
        locals.kinds.push(kind);
        locals.used.push(false);
        if let Some(scope) = locals.scopes.last_mut() {
            if let Some(first) = scope.names.get(&name.name).copied() {
                let first_span = self.locals.get(first.0 as usize).map(|l| l.span);
                let mut d = Diagnostic::new(
                    codes::E0606,
                    format!("`{}` is declared twice in the same block", name.name),
                )
                .primary(name.span, "second declaration")
                .help("rename it; a name may be reused only in a nested block");
                if let Some(span) = first_span {
                    d = d.secondary(span, "first declaration");
                }
                self.report(d);
            }
            scope.names.insert(name.name.clone(), id);
        }
        id
    }

    fn local_index(&self, id: LocalId, locals: &Locals) -> Option<usize> {
        // Locals of the current item start after those of earlier items.
        let base = self.locals.len().checked_sub(locals.used.len())?;
        (id.0 as usize).checked_sub(base)
    }

    fn pop_scope(&mut self, locals: &mut Locals) {
        let Some(scope) = locals.scopes.pop() else {
            return;
        };
        let mut unused: Vec<(String, Span, Kind)> = Vec::new();
        for (name, id) in &scope.names {
            if let Some(i) = self.local_index(*id, locals) {
                let kind = locals.kinds.get(i).copied().unwrap_or(Kind::Exempt);
                let used = locals.used.get(i).copied().unwrap_or(true);
                if kind != Kind::Exempt && !used {
                    if let Some(local) = self.locals.get(id.0 as usize) {
                        unused.push((name.clone(), local.span, kind));
                    }
                }
            }
        }
        unused.sort_by_key(|(_, span, _)| *span);
        for (name, span, kind) in unused {
            let help = match kind {
                Kind::Pattern => "remove it, or replace the name with `_`",
                _ => "remove it, or write `let _ = ...` to compute the value without keeping it",
            };
            self.report(
                Diagnostic::new(codes::E0618, format!("unused variable `{name}`"))
                    .primary(span, "this variable is never used")
                    .help(help),
            );
        }
    }

    fn lookup_local(&self, name: &str, locals: &Locals) -> Option<LocalId> {
        locals
            .scopes
            .iter()
            .rev()
            .find_map(|scope| scope.names.get(name).copied())
    }

    fn mark_used(&self, id: LocalId, locals: &mut Locals) {
        if let Some(i) = self.local_index(id, locals) {
            if let Some(slot) = locals.used.get_mut(i) {
                *slot = true;
            }
        }
    }

    // ----- names ----------------------------------------------------------------------------

    /// Resolves a value name: locals, then module level. Reports E0609 when unknown.
    fn value_name(&mut self, ctx: &Ctx, name: &Ident, locals: &mut Locals) -> Option<Res> {
        if let Some(id) = self.lookup_local(&name.name, locals) {
            self.mark_used(id, locals);
            return Some(Res::Local(id));
        }
        match self.lookup_module_level(ctx.module, &name.name, ctx.in_tests, true) {
            Some(Binding::Def(def)) => Some(Res::Def(def)),
            Some(Binding::Module(m)) => Some(Res::Module(m)),
            None if self.is_poisoned(ctx.module, &name.name) => None,
            None => {
                let mut candidates: Vec<String> = locals
                    .scopes
                    .iter()
                    .flat_map(|s| s.names.keys().cloned())
                    .collect();
                candidates.extend(self.visible_names(ctx));
                let mut d = Diagnostic::new(codes::E0609, format!("unknown name `{}`", name.name))
                    .primary(name.span, "not found in this scope");
                if let Some(close) = suggest(&name.name, candidates.iter().map(String::as_str)) {
                    d = d.help(format!("did you mean `{close}`?"));
                } else if name
                    .name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_lowercase())
                {
                    d = d.help("if it comes from another module, import it in `imports`");
                }
                self.report(d);
                None
            }
        }
    }

    fn visible_names(&self, ctx: &Ctx) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if let Some(state) = self.module(ctx.module) {
            out.extend(state.items.keys().cloned());
            if ctx.in_tests {
                out.extend(state.test_items.keys().cloned());
            }
            out.extend(state.imports.keys().cloned());
        }
        out.extend(self.prelude.keys().cloned());
        out
    }

    fn unknown_type(&mut self, ctx: &Ctx, name: &Ident) {
        if self.is_poisoned(ctx.module, &name.name) {
            return;
        }
        let candidates: Vec<String> = self
            .visible_names(ctx)
            .into_iter()
            .filter(|n| n.starts_with(|c: char| c.is_ascii_uppercase()))
            .collect();
        let mut d = Diagnostic::new(codes::E0402, format!("unknown type `{}`", name.name))
            .primary(name.span, "not found in this scope");
        if let Some(close) = suggest(&name.name, candidates.iter().map(String::as_str)) {
            d = d.help(format!("did you mean `{close}`?"));
        }
        self.report(d);
    }

    /// A member of a module reached through `module.name`: must exist and be public.
    fn module_member(&mut self, ctx: &Ctx, module: ModuleId, name: &Ident) -> Option<Binding> {
        let state = self.module(module)?;
        let path = join(&state.path);
        if let Some(binding) = state.exports.get(&name.name).copied() {
            return Some(binding);
        }
        if module == ctx.module {
            return state.items.get(&name.name).map(|d| Binding::Def(*d));
        }
        if let Some(d) = self.unavailable(&path, name) {
            if let Some(d) = d {
                self.report(d);
            }
            return None;
        }
        let private = state.items.contains_key(&name.name);
        let mut d = if private {
            Diagnostic::new(
                codes::E0607,
                format!("`{}` is private in `{path}`", name.name),
            )
            .primary(name.span, "private declaration")
            .help(format!("mark it `pub` in `{path}`"))
        } else {
            Diagnostic::new(
                codes::E0609,
                format!("`{path}` has no member `{}`", name.name),
            )
            .primary(name.span, "not found in this module")
        };
        if !private {
            if let Some(close) = suggest(&name.name, state.exports.keys().map(String::as_str)) {
                d = d.help(format!("did you mean `{close}`?"));
            }
        }
        self.report(d);
        None
    }

    /// A member (variant or inherent function) of a type reached through `Type.name`.
    fn type_member(&mut self, ctx: &Ctx, owner: DefId, name: &Ident) -> Option<DefId> {
        let found = self
            .members
            .get(&owner)
            .and_then(|m| m.get(&name.name))
            .copied();
        let owner_def = self.def(owner).cloned()?;
        match found {
            Some(member) => {
                let visible = self
                    .def(member)
                    .is_some_and(|d| d.public || d.module == ctx.module);
                if !visible {
                    self.report(
                        Diagnostic::new(
                            codes::E0607,
                            format!("`{}.{}` is private", owner_def.name, name.name),
                        )
                        .primary(name.span, "private associated function"),
                    );
                }
                Some(member)
            }
            None => {
                // Aliases and interfaces have no members of their own here; the type checker
                // looks at them (M4).
                if matches!(
                    owner_def.kind,
                    DefKind::Alias | DefKind::Interface | DefKind::ExternType
                ) {
                    return None;
                }
                let what = if owner_def.kind == DefKind::Enum {
                    "variant or associated function"
                } else {
                    "associated function"
                };
                let mut d = Diagnostic::new(
                    codes::E0609,
                    format!("`{}` has no {what} `{}`", owner_def.name, name.name),
                )
                .primary(name.span, "not found");
                if let Some(members) = self.members.get(&owner) {
                    if let Some(close) = suggest(&name.name, members.keys().map(String::as_str)) {
                        d = d.help(format!("did you mean `{close}`?"));
                    }
                }
                self.report(d);
                None
            }
        }
    }

    fn record(&mut self, file_node: (wolf_span::FileId, NodeId), span: wolf_span::Span, res: Res) {
        self.res.insert(file_node, res);
        self.spans.insert(file_node, span);
    }

    fn file(&self, ctx: &Ctx) -> wolf_span::FileId {
        self.module(ctx.module)
            .map(|m| m.file)
            .unwrap_or(wolf_span::FileId::ZERO)
    }

    // ----- types ----------------------------------------------------------------------------

    fn ty(&mut self, ctx: &Ctx, ty: &Ty) {
        let file = self.file(ctx);
        match &ty.kind {
            TyKind::Path { path, args } => {
                for arg in args {
                    self.ty(ctx, arg);
                }
                if let Some(res) = self.type_path(ctx, path) {
                    self.record((file, ty.id), ty.span, res);
                }
            }
            TyKind::Fn { params, ret } => {
                for (_, param) in params {
                    self.ty(ctx, param);
                }
                if let Some(ret) = ret {
                    self.ty(ctx, ret);
                }
            }
            TyKind::SelfType => {
                if ctx.self_type.is_none() {
                    self.report(
                        Diagnostic::new(codes::E0609, "`Self` outside of `impl` and `interface`")
                            .primary(
                                ty.span,
                                "`Self` names the type of an impl block or an interface",
                            ),
                    );
                } else {
                    self.record((file, ty.id), ty.span, Res::SelfType);
                }
            }
            TyKind::Error => {}
        }
    }

    fn type_path(&mut self, ctx: &Ctx, path: &Path) -> Option<Res> {
        let (first, rest) = path.segments.split_first()?;
        if rest.is_empty() {
            if let Some((_, span)) = ctx
                .generics
                .iter()
                .rev()
                .find(|(name, _)| *name == first.name)
            {
                return Some(Res::TypeParam(*span));
            }
        }
        let mut binding =
            match self.lookup_module_level(ctx.module, &first.name, ctx.in_tests, true) {
                Some(binding) => binding,
                None => {
                    self.unknown_type(ctx, first);
                    return None;
                }
            };
        for segment in rest {
            match binding {
                Binding::Module(m) => binding = self.module_member(ctx, m, segment)?,
                Binding::Def(_) => {
                    self.report(
                        Diagnostic::new(
                            codes::E0402,
                            format!("`{}` is not a type", join(&names(path))),
                        )
                        .primary(path.span, "a type has no members that are types"),
                    );
                    return None;
                }
            }
        }
        match binding {
            Binding::Def(def) if self.def(def).is_some_and(|d| d.kind.is_type()) => {
                Some(Res::Def(def))
            }
            Binding::Def(def) => {
                let what = self.def(def).map(|d| d.kind.describe()).unwrap_or("value");
                self.report(
                    Diagnostic::new(
                        codes::E0402,
                        format!("`{}` is a {what}, not a type", join(&names(path))),
                    )
                    .primary(path.span, "expected a type"),
                );
                None
            }
            Binding::Module(_) => {
                self.report(
                    Diagnostic::new(
                        codes::E0402,
                        format!("`{}` is a module, not a type", join(&names(path))),
                    )
                    .primary(path.span, "expected a type"),
                );
                None
            }
        }
    }

    // ----- statements -------------------------------------------------------------------------

    /// A block. With `shared`, the block's names go into the current scope (the top block of a
    /// function, loop, arm or closure).
    fn block(&mut self, ctx: &Ctx, block: &Block, locals: &mut Locals, shared: bool) {
        if !shared {
            locals.scopes.push(Scope::default());
        }
        for stmt in &block.stmts {
            self.stmt(ctx, stmt, locals);
        }
        if !shared {
            self.pop_scope(locals);
        }
    }

    fn stmt(&mut self, ctx: &Ctx, stmt: &Stmt, locals: &mut Locals) {
        match &stmt.kind {
            StmtKind::Let {
                name,
                ty,
                value,
                mutable,
            } => {
                if let Some(ty) = ty {
                    self.ty(ctx, ty);
                }
                // The initializer is resolved before the name exists: `let x = x + 1`.
                self.expr(ctx, value, locals);
                if let Some(name) = name {
                    self.style(name, false, "variable");
                    let kind = if *mutable { Kind::Var } else { Kind::Let };
                    self.declare(name, locals, kind);
                }
            }
            StmtKind::LetPattern { pattern, value, .. } => {
                self.expr(ctx, value, locals);
                locals.let_pattern = true;
                self.pattern(ctx, pattern, locals);
                locals.let_pattern = false;
            }
            StmtKind::Assign { target, value, .. } => {
                self.expr(ctx, value, locals);
                self.expr(ctx, target, locals);
            }
            StmtKind::While { label, cond, body } => {
                self.expr(ctx, cond, locals);
                self.push_label(label, locals);
                self.block(ctx, body, locals, false);
                self.pop_label(label, locals);
            }
            StmtKind::WhileLet {
                label,
                pattern,
                value,
                body,
                ..
            } => {
                self.expr(ctx, value, locals);
                locals.scopes.push(Scope::default());
                self.pattern(ctx, pattern, locals);
                self.push_label(label, locals);
                self.block(ctx, body, locals, true);
                self.pop_label(label, locals);
                self.pop_scope(locals);
            }
            StmtKind::For {
                label,
                var,
                iter,
                body,
            } => {
                match iter {
                    ForIter::Collection { expr, .. } => self.expr(ctx, expr, locals),
                    ForIter::Range { start, end, .. } => {
                        self.expr(ctx, start, locals);
                        self.expr(ctx, end, locals);
                    }
                }
                locals.scopes.push(Scope::default());
                self.style(var, false, "loop variable");
                // Loop variables are exempt from E0618 (chapter 6, 3.2 rule 6).
                self.declare(var, locals, Kind::Exempt);
                self.push_label(label, locals);
                self.block(ctx, body, locals, true);
                self.pop_label(label, locals);
                self.pop_scope(locals);
            }
            StmtKind::Expr(expr) => self.expr(ctx, expr, locals),
        }
    }

    /// Enters a loop with `label`; a label of an enclosing loop cannot be reused (E0438).
    fn push_label(&mut self, label: &Option<Ident>, locals: &mut Locals) {
        let Some(label) = label else {
            return;
        };
        self.style(label, false, "label");
        if let Some(outer) = locals.labels.iter().find(|l| l.name == label.name) {
            let outer = outer.span;
            self.report(
                Diagnostic::new(
                    codes::E0438,
                    format!(
                        "the label `{}` is already used by an enclosing loop",
                        label.name
                    ),
                )
                .primary(label.span, "this label repeats an outer one")
                .secondary(outer, "the enclosing loop with this label")
                .help("give the inner loop another label"),
            );
        }
        locals.labels.push(label.clone());
    }

    fn pop_label(&mut self, label: &Option<Ident>, locals: &mut Locals) {
        if label.is_some() {
            locals.labels.pop();
        }
    }

    /// Checks the label of a `break` or a `continue` (E0436, E0437).
    fn jump_label(&mut self, label: &Ident, locals: &Locals) {
        if !locals.labels.iter().any(|l| l.name == label.name) {
            self.report(
                Diagnostic::new(codes::E0436, format!("unknown loop label `{}`", label.name))
                    .primary(label.span, "no enclosing loop has this label")
                    .help("labels are written before a loop: `outer: for x in xs { ... }`"),
            );
        } else if let Some(id) = self.lookup_local(&label.name, locals) {
            let mut d = Diagnostic::new(
                codes::E0437,
                format!("`{}` is both a loop label and a variable", label.name),
            )
            .primary(label.span, "a label or a value?")
            .help("rename the label or the variable");
            if let Some(local) = self.locals.get(id.0 as usize) {
                d = d.secondary(local.span, "the variable");
            }
            self.report(d);
        }
    }

    // ----- expressions ----------------------------------------------------------------------

    fn expr(&mut self, ctx: &Ctx, expr: &Expr, locals: &mut Locals) {
        let file = self.file(ctx);
        match &expr.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Unit
            | ExprKind::Error => {}
            ExprKind::FStr(parts) => {
                for part in parts {
                    if let FStrPart::Expr(e) = part {
                        self.expr(ctx, e, locals);
                    }
                }
            }
            ExprKind::Continue { label } => {
                if let Some(label) = label {
                    self.jump_label(label, locals);
                }
            }
            ExprKind::Break { label, value } => {
                if let Some(label) = label {
                    self.jump_label(label, locals);
                }
                if let Some(value) = value {
                    // `break name` with an unknown name: a label was most likely meant.
                    let unknown = match &value.kind {
                        ExprKind::Name(name) => {
                            self.lookup_local(&name.name, locals).is_none()
                                && self
                                    .lookup_module_level(
                                        ctx.module,
                                        &name.name,
                                        ctx.in_tests,
                                        false,
                                    )
                                    .is_none()
                                && !self.is_poisoned(ctx.module, &name.name)
                        }
                        _ => false,
                    };
                    match &value.kind {
                        ExprKind::Name(name) if unknown => self.jump_label(name, locals),
                        _ => self.expr(ctx, value, locals),
                    }
                }
            }
            ExprKind::Loop { label, body } => {
                self.push_label(label, locals);
                self.block(ctx, body, locals, false);
                self.pop_label(label, locals);
            }
            ExprKind::IfLet {
                pattern,
                value,
                then,
                otherwise,
                ..
            } => {
                self.expr(ctx, value, locals);
                locals.scopes.push(Scope::default());
                self.pattern(ctx, pattern, locals);
                self.block(ctx, then, locals, true);
                self.pop_scope(locals);
                if let Some(other) = otherwise {
                    self.expr(ctx, other, locals);
                }
            }
            ExprKind::Name(name) => {
                if let Some(res) = self.value_name(ctx, name, locals) {
                    self.record((file, expr.id), expr.span, res);
                }
            }
            ExprKind::SelfValue => {
                if !ctx.self_value {
                    self.report(
                        Diagnostic::new(codes::E0609, "`self` outside of a method").primary(
                            expr.span,
                            "only methods with a `self` parameter have `self`",
                        ),
                    );
                }
            }
            ExprKind::SelfType | ExprKind::Field { .. } | ExprKind::TypeArgs { .. } => {
                let _ = self.head(ctx, expr, locals);
            }
            ExprKind::Paren(inner) | ExprKind::Try(inner) => self.expr(ctx, inner, locals),
            ExprKind::List(items) => {
                for item in items {
                    self.expr(ctx, item, locals);
                }
            }
            ExprKind::Map(entries) => {
                for (k, v) in entries {
                    self.expr(ctx, k, locals);
                    self.expr(ctx, v, locals);
                }
            }
            ExprKind::StructLit {
                path,
                type_args,
                fields,
                base,
            } => {
                if let Some(base) = base {
                    self.expr(ctx, base, locals);
                }
                for arg in type_args {
                    self.ty(ctx, arg);
                }
                if let Some(res) = self.struct_path(ctx, path, locals) {
                    self.record((file, expr.id), expr.span, res);
                }
                for field in fields {
                    match &field.value {
                        Some(value) => self.expr(ctx, value, locals),
                        // `User { id }` uses the variable `id`.
                        None => {
                            if let Some(res) = self.value_name(ctx, &field.name, locals) {
                                self.record((file, field.id), field.name.span, res);
                            }
                        }
                    }
                }
            }
            ExprKind::Call { callee, args } => {
                self.expr(ctx, callee, locals);
                for arg in args {
                    self.expr(ctx, &arg.value, locals);
                }
            }
            ExprKind::Index { base, index } => {
                self.expr(ctx, base, locals);
                self.expr(ctx, index, locals);
            }
            ExprKind::Slice {
                base, start, end, ..
            } => {
                self.expr(ctx, base, locals);
                for bound in [start, end].into_iter().flatten() {
                    self.expr(ctx, bound, locals);
                }
            }
            ExprKind::Unary { operand, .. } => self.expr(ctx, operand, locals),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(ctx, lhs, locals);
                self.expr(ctx, rhs, locals);
            }
            ExprKind::Closure { params, ret, body } => {
                // A closure body is not inside the loops around the closure.
                let outer_labels = std::mem::take(&mut locals.labels);
                locals.scopes.push(Scope::default());
                for param in params {
                    if let Some(ty) = &param.ty {
                        self.ty(ctx, ty);
                    }
                    self.style(&param.name, false, "parameter");
                    self.declare(&param.name, locals, Kind::Exempt);
                }
                if let Some(ret) = ret {
                    self.ty(ctx, ret);
                }
                match &body.kind {
                    ExprKind::Block(block) => self.block(ctx, block, locals, true),
                    _ => self.expr(ctx, body, locals),
                }
                self.pop_scope(locals);
                locals.labels = outer_labels;
            }
            ExprKind::Block(block) => self.block(ctx, block, locals, false),
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.expr(ctx, cond, locals);
                self.block(ctx, then, locals, false);
                if let Some(other) = otherwise {
                    self.expr(ctx, other, locals);
                }
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                self.expr(ctx, scrutinee, locals);
                for arm in arms {
                    locals.scopes.push(Scope::default());
                    self.pattern(ctx, &arm.pattern, locals);
                    if let Some(guard) = &arm.guard {
                        self.expr(ctx, guard, locals);
                    }
                    match &arm.body.kind {
                        ExprKind::Block(block) => self.block(ctx, block, locals, true),
                        _ => self.expr(ctx, &arm.body, locals),
                    }
                    self.pop_scope(locals);
                }
            }
            ExprKind::Return(value) => {
                if let Some(value) = value {
                    self.expr(ctx, value, locals);
                }
            }
        }
    }

    /// Resolves the left part of a dotted expression and records what it names.
    fn head(&mut self, ctx: &Ctx, expr: &Expr, locals: &mut Locals) -> Head {
        let file = self.file(ctx);
        match &expr.kind {
            ExprKind::Name(name) => match self.value_name(ctx, name, locals) {
                Some(res) => {
                    self.record((file, expr.id), expr.span, res);
                    match res {
                        Res::Module(m) => Head::Module(m),
                        Res::Def(def) if self.def(def).is_some_and(|d| d.kind.is_type()) => {
                            Head::Type(def)
                        }
                        _ => Head::Value,
                    }
                }
                None => Head::Value,
            },
            ExprKind::SelfType => match ctx.self_type {
                Some(Some(owner)) => {
                    self.record((file, expr.id), expr.span, Res::SelfType);
                    Head::Type(owner)
                }
                Some(None) => Head::UnknownType,
                None => {
                    self.report(
                        Diagnostic::new(codes::E0609, "`Self` outside of `impl` and `interface`")
                            .primary(expr.span, "`Self` names the type of an impl block"),
                    );
                    Head::Value
                }
            },
            ExprKind::Field { base, name } => match self.head(ctx, base, locals) {
                Head::Module(m) => match self.module_member(ctx, m, name) {
                    Some(Binding::Module(inner)) => {
                        self.record((file, expr.id), expr.span, Res::Module(inner));
                        Head::Module(inner)
                    }
                    Some(Binding::Def(def)) => {
                        self.record((file, expr.id), expr.span, Res::Def(def));
                        if self.def(def).is_some_and(|d| d.kind.is_type()) {
                            Head::Type(def)
                        } else {
                            Head::Value
                        }
                    }
                    None => Head::Value,
                },
                Head::Type(owner) => {
                    if let Some(member) = self.type_member(ctx, owner, name) {
                        self.record((file, expr.id), expr.span, Res::Def(member));
                    }
                    Head::Value
                }
                Head::UnknownType | Head::Value => Head::Value,
            },
            ExprKind::TypeArgs { base, args } => {
                for arg in args {
                    self.ty(ctx, arg);
                }
                self.head(ctx, base, locals)
            }
            _ => {
                self.expr(ctx, expr, locals);
                Head::Value
            }
        }
    }

    /// The path of a struct literal or of a pattern: a struct, a variant or `Self`.
    fn struct_path(&mut self, ctx: &Ctx, path: &Path, locals: &mut Locals) -> Option<Res> {
        let (first, rest) = path.segments.split_first()?;
        let mut current = if first.name == "Self" {
            match ctx.self_type {
                Some(Some(owner)) => Res::Def(owner),
                Some(None) => return None,
                None => {
                    self.report(
                        Diagnostic::new(codes::E0609, "`Self` outside of `impl` and `interface`")
                            .primary(first.span, "`Self` names the type of an impl block"),
                    );
                    return None;
                }
            }
        } else {
            match self.lookup_module_level(ctx.module, &first.name, ctx.in_tests, true) {
                Some(Binding::Def(def)) => Res::Def(def),
                Some(Binding::Module(m)) => Res::Module(m),
                None => {
                    let _ = self.value_name(ctx, first, locals);
                    return None;
                }
            }
        };
        for segment in rest {
            current = match current {
                Res::Module(m) => match self.module_member(ctx, m, segment)? {
                    Binding::Def(def) => Res::Def(def),
                    Binding::Module(inner) => Res::Module(inner),
                },
                Res::Def(def) if self.def(def).is_some_and(|d| d.kind.is_type()) => {
                    Res::Def(self.type_member(ctx, def, segment)?)
                }
                _ => return None,
            };
        }
        Some(current)
    }

    // ----- patterns -------------------------------------------------------------------------

    fn pattern(&mut self, ctx: &Ctx, pat: &Pat, locals: &mut Locals) {
        let file = self.file(ctx);
        match &pat.kind {
            PatKind::Wild
            | PatKind::Int { .. }
            | PatKind::Str(_)
            | PatKind::Bool(_)
            | PatKind::Error => {}
            PatKind::Name(name) => {
                // A lone name is a binding, unless it is a payload-less variant or a constant
                // (chapter 3, section 7, rule 3). In a destructuring `let` it is always a binding.
                let existing =
                    self.lookup_module_level(ctx.module, &name.name, ctx.in_tests, false);
                let constant = !locals.let_pattern
                    && match existing {
                        Some(Binding::Def(def)) => self.def(def).is_some_and(|d| {
                            matches!(
                                d.kind,
                                DefKind::Config
                                    | DefKind::Variant {
                                        payload: Payload::None,
                                        ..
                                    }
                            )
                        }),
                        _ => false,
                    };
                if constant {
                    if let Some(Binding::Def(def)) =
                        self.lookup_module_level(ctx.module, &name.name, ctx.in_tests, true)
                    {
                        self.record((file, pat.id), pat.span, Res::Def(def));
                    }
                } else if locals
                    .scopes
                    .last()
                    .is_some_and(|s| s.names.contains_key(&name.name))
                {
                    // The same name in two alternatives of `A(x) | B(x)` is one binding.
                    if let Some(id) = self.lookup_local(&name.name, locals) {
                        self.record((file, pat.id), pat.span, Res::Local(id));
                    }
                } else {
                    self.style(name, false, "binding");
                    let id = self.declare(name, locals, Kind::Pattern);
                    self.record((file, pat.id), pat.span, Res::Local(id));
                }
            }
            PatKind::Path(path) => {
                if let Some(res) = self.struct_path(ctx, path, locals) {
                    self.record((file, pat.id), pat.span, res);
                }
            }
            PatKind::Tuple { path, items } => {
                if let Some(res) = self.struct_path(ctx, path, locals) {
                    self.record((file, pat.id), pat.span, res);
                }
                for item in items {
                    self.pattern(ctx, item, locals);
                }
            }
            PatKind::Struct { path, fields, .. } => {
                if let Some(res) = self.struct_path(ctx, path, locals) {
                    self.record((file, pat.id), pat.span, res);
                }
                for field in fields {
                    match &field.pat {
                        Some(sub) => self.pattern(ctx, sub, locals),
                        None => {
                            self.style(&field.name, false, "binding");
                            let id = self.declare(&field.name, locals, Kind::Pattern);
                            self.record((file, field.id), field.name.span, Res::Local(id));
                        }
                    }
                }
            }
            PatKind::Or(alternatives) => {
                for alternative in alternatives {
                    self.pattern(ctx, alternative, locals);
                }
            }
        }
    }

    fn unused_imports(&mut self, module: ModuleId) {
        let Some(state) = self.module(module) else {
            return;
        };
        let mut unused: Vec<(String, Span)> = state
            .imports
            .iter()
            .filter(|(_, import)| !import.used)
            .map(|(name, import)| (name.clone(), import.span))
            .collect();
        unused.sort_by_key(|(_, span)| *span);
        for (name, span) in unused {
            self.report(
                Diagnostic::new(codes::E0615, format!("unused import `{name}`"))
                    .primary(span, "imported but never used")
                    .help("remove it from `imports`"),
            );
        }
    }
}

fn names(path: &Path) -> Vec<String> {
    path.segments.iter().map(|s| s.name.clone()).collect()
}
