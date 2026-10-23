//! Pass 1: top-level names, type members, imports and the import graph.

use std::collections::{BTreeMap, BTreeSet};

use wolf_ast::*;
use wolf_diag::{Diagnostic, codes};
use wolf_project::Kind;

use crate::UNREADABLE;
use wolf_span::{FileId, Span};

use crate::names::{is_pascal_case, is_snake_case, suggest, to_pascal, to_snake};
use crate::{
    Def, DefId, DefKind, Local, LocalId, ModuleId, ModuleInfo, ModuleInput, Payload, ProjectInfo,
    Res, Resolved, Role,
};

/// What a module-level name refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Binding {
    Def(DefId),
    Module(ModuleId),
}

/// A name bound by an import.
pub(crate) struct ImportBinding {
    pub(crate) binding: Binding,
    /// The bound name (alias or imported name) in the import.
    pub(crate) span: Span,
    pub(crate) used: bool,
}

pub(crate) struct ModuleState {
    pub(crate) path: Vec<String>,
    pub(crate) role: Role,
    pub(crate) file: FileId,
    /// Top-level declarations, except helpers in `tests`.
    pub(crate) items: BTreeMap<String, DefId>,
    /// Functions declared in `tests`: visible only to tests of the module.
    pub(crate) test_items: BTreeMap<String, DefId>,
    pub(crate) imports: BTreeMap<String, ImportBinding>,
    /// What other modules can import: `pub` declarations, and `pub` imports where allowed.
    pub(crate) exports: BTreeMap<String, Binding>,
    /// Imports of other project modules, for cycle detection: target and import span.
    pub(crate) edges: Vec<(ModuleId, Span)>,
}

pub(crate) struct Resolver<'a> {
    pub(crate) inputs: &'a [ModuleInput<'a>],
    pub(crate) project: &'a ProjectInfo<'a>,
    pub(crate) modules: Vec<ModuleState>,
    pub(crate) defs: Vec<Def>,
    pub(crate) locals: Vec<Local>,
    pub(crate) res: BTreeMap<(FileId, NodeId), Res>,
    pub(crate) spans: BTreeMap<(FileId, NodeId), Span>,
    pub(crate) members: BTreeMap<DefId, BTreeMap<String, DefId>>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) by_path: BTreeMap<String, ModuleId>,
    /// Names visible everywhere without an import.
    pub(crate) prelude: BTreeMap<String, Binding>,
    /// The prelude as std itself sees it, including hidden names such as `Never`.
    pub(crate) prelude_for_std: BTreeMap<String, Binding>,
    /// Names of failed imports per module: uses of them are not reported again.
    pub(crate) poisoned: BTreeMap<ModuleId, BTreeSet<String>>,
    /// Inherent impl blocks resolved to their type: module, item index, owner.
    pub(crate) impl_owners: BTreeMap<(usize, usize, usize), DefId>,
}

/// A suggested alias in the same naming style: `FsErrorOther`, `size_other`.
fn other_name(name: &str) -> String {
    if name.starts_with(|c: char| c.is_ascii_uppercase()) {
        format!("{name}Other")
    } else {
        format!("{}_other", to_snake(name))
    }
}

pub(crate) fn join(path: &[String]) -> String {
    path.join(".")
}

fn path_names(path: &Path) -> Vec<String> {
    path.segments.iter().map(|s| s.name.clone()).collect()
}

impl<'a> Resolver<'a> {
    pub(crate) fn new(inputs: &'a [ModuleInput<'a>], project: &'a ProjectInfo<'a>) -> Resolver<'a> {
        Resolver {
            inputs,
            project,
            modules: Vec::new(),
            defs: Vec::new(),
            locals: Vec::new(),
            res: BTreeMap::new(),
            spans: BTreeMap::new(),
            members: BTreeMap::new(),
            diagnostics: Vec::new(),
            by_path: BTreeMap::new(),
            prelude: BTreeMap::new(),
            prelude_for_std: BTreeMap::new(),
            poisoned: BTreeMap::new(),
            impl_owners: BTreeMap::new(),
        }
    }

    pub(crate) fn finish(self) -> Resolved {
        let local_decls = self
            .locals
            .iter()
            .enumerate()
            .map(|(i, l)| (l.span, LocalId(u32::try_from(i).unwrap_or(u32::MAX))))
            .collect();
        Resolved {
            modules: self
                .modules
                .into_iter()
                .map(|m| ModuleInfo {
                    path: m.path,
                    role: m.role,
                    file: m.file,
                    items: m.items,
                })
                .collect(),
            defs: self.defs,
            locals: self.locals,
            local_decls,
            res: self.res,
            spans: self.spans,
            members: self.members,
            impls: self
                .impl_owners
                .iter()
                .map(|(&(m, s, i), &def)| ((ModuleId(m as u32), s, i), def))
                .collect(),
            diagnostics: self.diagnostics,
        }
    }

    pub(crate) fn report(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub(crate) fn def(&self, id: DefId) -> Option<&Def> {
        self.defs.get(id.0 as usize)
    }

    pub(crate) fn module(&self, id: ModuleId) -> Option<&ModuleState> {
        self.modules.get(id.0 as usize)
    }

    pub(crate) fn is_std(&self, module: ModuleId) -> bool {
        self.module(module)
            .is_some_and(|m| matches!(m.role, Role::Std | Role::Prelude))
    }

    /// The module of a library project whose `pub` imports are re-exports: `lib`.
    fn may_reexport(&self, module: ModuleId) -> bool {
        self.module(module).is_some_and(|m| match m.role {
            Role::Prelude | Role::Std => true,
            Role::Entry => self.project.kind == Kind::Lib && m.path == ["lib"],
            _ => false,
        })
    }

    // ----- pass 1 -------------------------------------------------------------------------

    pub(crate) fn collect(&mut self) {
        let inputs = self.inputs;
        for (index, input) in inputs.iter().enumerate() {
            let id = ModuleId(u32::try_from(index).unwrap_or(u32::MAX));
            self.by_path.insert(join(&input.path), id);
            self.modules.push(ModuleState {
                path: input.path.clone(),
                role: input.role,
                file: input.file,
                items: BTreeMap::new(),
                test_items: BTreeMap::new(),
                imports: BTreeMap::new(),
                exports: BTreeMap::new(),
                edges: Vec::new(),
            });
        }
        for (index, input) in inputs.iter().enumerate() {
            self.declare_items(ModuleId(index as u32), input);
        }
        // Exports from declarations; re-exports are added while imports are resolved.
        for module in &mut self.modules {
            for (name, def) in &module.items {
                if self.defs.get(def.0 as usize).is_some_and(|d| d.public) {
                    module.exports.insert(name.clone(), Binding::Def(*def));
                }
            }
        }
        // Imports in role order: std first, then `lib`, so re-exports exist before their users.
        let mut order: Vec<usize> = (0..inputs.len()).collect();
        order.sort_by_key(|&i| (inputs.get(i).map(|m| m.role), i));
        for i in order.iter().copied() {
            if let Some(input) = inputs.get(i) {
                self.imports(ModuleId(i as u32), input);
            }
        }
        self.build_prelude();
        for i in order {
            if let Some(input) = inputs.get(i) {
                self.check_prelude_redeclarations(ModuleId(i as u32));
                self.inherent_impls(i, input);
            }
        }
        self.cycles();
    }

    fn new_def(&mut self, kind: DefKind, name: &Ident, module: ModuleId, public: bool) -> DefId {
        let id = DefId(u32::try_from(self.defs.len()).unwrap_or(u32::MAX));
        self.defs.push(Def {
            kind,
            name: name.name.clone(),
            module,
            public,
            span: name.span,
        });
        id
    }

    /// Checks the naming style of a declared name (E0601).
    pub(crate) fn style(&mut self, name: &Ident, pascal: bool, what: &str) {
        if self.inputs_role_is_std(name.span.file) {
            return;
        }
        let ok = if pascal {
            is_pascal_case(&name.name)
        } else {
            is_snake_case(&name.name)
        };
        if ok {
            return;
        }
        let (style, fixed) = if pascal {
            ("PascalCase", to_pascal(&name.name))
        } else {
            ("snake_case", to_snake(&name.name))
        };
        let mut d = Diagnostic::new(
            codes::E0601,
            format!("{what} `{}` should be in `{style}`", name.name),
        )
        .primary(name.span, format!("not `{style}`"));
        if !fixed.is_empty() && fixed != name.name {
            d = d.help(format!("rename it to `{fixed}`"));
        }
        self.report(d);
    }

    fn inputs_role_is_std(&self, file: FileId) -> bool {
        self.inputs
            .iter()
            .any(|m| m.file == file && matches!(m.role, Role::Std | Role::Prelude))
    }

    /// Adds a top-level name, reporting a duplicate (E0605).
    fn add_item(&mut self, module: ModuleId, name: &Ident, def: DefId, tests: bool) {
        let Some(state) = self.modules.get_mut(module.0 as usize) else {
            return;
        };
        let table = if tests {
            &mut state.test_items
        } else {
            &mut state.items
        };
        if let Some(first) = table.get(&name.name).copied() {
            let first_span = self.defs.get(first.0 as usize).map(|d| d.span);
            let mut d = Diagnostic::new(codes::E0605, format!("`{}` is declared twice", name.name))
                .primary(name.span, "second declaration");
            if let Some(span) = first_span {
                d = d.secondary(span, "first declaration");
            }
            self.report(d.help("types and values share one namespace; rename one of them"));
            return;
        }
        table.insert(name.name.clone(), def);
    }

    fn declare_items(&mut self, module: ModuleId, input: &ModuleInput<'_>) {
        for section in &input.ast.sections {
            let tests = section.kind == SectionKind::Tests;
            for item in &section.items {
                match &item.kind {
                    ItemKind::Struct(decl) => {
                        self.style(&decl.name, true, "struct");
                        let def = self.new_def(DefKind::Struct, &decl.name, module, decl.public);
                        self.add_item(module, &decl.name, def, false);
                        let mut seen = BTreeSet::new();
                        for field in &decl.fields {
                            self.style(&field.name, false, "field");
                            if !seen.insert(field.name.name.clone()) {
                                self.report(
                                    Diagnostic::new(
                                        codes::E0605,
                                        format!("field `{}` is declared twice", field.name.name),
                                    )
                                    .primary(field.name.span, "second field with this name"),
                                );
                            }
                        }
                    }
                    ItemKind::Enum(decl) => {
                        self.style(&decl.name, true, "enum");
                        let def = self.new_def(DefKind::Enum, &decl.name, module, decl.public);
                        self.add_item(module, &decl.name, def, false);
                        for variant in &decl.variants {
                            self.style(&variant.name, true, "variant");
                            let payload = match variant.payload {
                                VariantPayload::None => Payload::None,
                                VariantPayload::Tuple(_) => Payload::Tuple,
                                VariantPayload::Struct(_) => Payload::Struct,
                            };
                            if let VariantPayload::Struct(fields) = &variant.payload {
                                for field in fields {
                                    self.style(&field.name, false, "field");
                                }
                            }
                            let v = self.new_def(
                                DefKind::Variant {
                                    parent: def,
                                    payload,
                                },
                                &variant.name,
                                module,
                                decl.public,
                            );
                            let entry = self.members.entry(def).or_default();
                            if entry.contains_key(&variant.name.name) {
                                self.report(
                                    Diagnostic::new(
                                        codes::E0605,
                                        format!(
                                            "variant `{}` is declared twice",
                                            variant.name.name
                                        ),
                                    )
                                    .primary(variant.name.span, "second variant with this name"),
                                );
                            } else {
                                entry.insert(variant.name.name.clone(), v);
                            }
                        }
                    }
                    ItemKind::Alias(decl) => {
                        self.style(&decl.name, true, "type alias");
                        let def = self.new_def(DefKind::Alias, &decl.name, module, decl.public);
                        self.add_item(module, &decl.name, def, false);
                    }
                    ItemKind::IntrinsicType(decl) => {
                        let def =
                            self.new_def(DefKind::IntrinsicType, &decl.name, module, decl.public);
                        self.add_item(module, &decl.name, def, false);
                    }
                    ItemKind::Interface(decl) => {
                        self.style(&decl.name, true, "interface");
                        let def = self.new_def(DefKind::Interface, &decl.name, module, decl.public);
                        self.add_item(module, &decl.name, def, false);
                        for method in &decl.methods {
                            self.style(&method.sig.name, false, "method");
                        }
                    }
                    ItemKind::Fn(decl) => {
                        self.style(&decl.sig.name, false, "function");
                        let def = self.new_def(
                            DefKind::Fn,
                            &decl.sig.name,
                            module,
                            decl.public && !tests,
                        );
                        self.add_item(module, &decl.sig.name, def, tests);
                    }
                    ItemKind::Config(entry) => {
                        self.style(&entry.name, false, "constant");
                        let def = self.new_def(DefKind::Config, &entry.name, module, entry.public);
                        self.add_item(module, &entry.name, def, false);
                    }
                    ItemKind::ExternTarget(target) => {
                        for decl in &target.decls {
                            let (name, kind) = match &decl.kind {
                                ExternDeclKind::Fn(sig) => (&sig.name, DefKind::ExternFn),
                                ExternDeclKind::Type(name) => (name, DefKind::ExternType),
                                // A record binding declares no name (chapter 7, section 8.4a).
                                ExternDeclKind::Struct(_) => continue,
                            };
                            self.style(name, kind == DefKind::ExternType, "extern declaration");
                            // The same name in several target blocks is one declaration
                            // (chapter 7, section 8.1); their signatures are compared in M11.
                            let existing = self
                                .modules
                                .get(module.0 as usize)
                                .and_then(|m| m.items.get(&name.name).copied());
                            if let Some(existing) = existing {
                                if self.def(existing).is_some_and(|d| d.kind == kind) {
                                    continue;
                                }
                            }
                            let def = self.new_def(kind, name, module, decl.public);
                            self.add_item(module, name, def, false);
                        }
                    }
                    ItemKind::Impl(_) | ItemKind::Test(_) | ItemKind::Import(_) => {}
                }
            }
        }
    }

    // ----- imports --------------------------------------------------------------------------

    fn imports(&mut self, module: ModuleId, input: &ModuleInput<'_>) {
        for section in &input.ast.sections {
            for item in &section.items {
                if let ItemKind::Import(import) = &item.kind {
                    self.import(module, input.role, import);
                }
            }
        }
    }

    /// Remembers the names an import would have bound, after it failed.
    fn poison(&mut self, module: ModuleId, import: &Import) {
        let names: Vec<String> = match &import.names {
            Some(group) => group
                .iter()
                .map(|n| n.alias.as_ref().unwrap_or(&n.name).name.clone())
                .collect(),
            None => import
                .alias
                .as_ref()
                .or(import.path.segments.last())
                .map(|n| vec![n.name.clone()])
                .unwrap_or_default(),
        };
        self.poisoned.entry(module).or_default().extend(names);
    }

    pub(crate) fn is_poisoned(&self, module: ModuleId, name: &str) -> bool {
        self.poisoned
            .get(&module)
            .is_some_and(|set| set.contains(name))
    }

    fn import(&mut self, module: ModuleId, role: Role, import: &Import) {
        let before = self.diagnostics.len();
        self.import_inner(module, role, import);
        if self.diagnostics.len() > before {
            self.poison(module, import);
        }
    }

    /// E0713 for a name of a native module that the compiler could not map (spec chapter 7,
    /// section 8.7).
    /// `Some(None)` when the native file could not be read (E0714 is reported already): its names
    /// are then not reported again.
    pub(crate) fn unavailable(
        &self,
        module_path: &str,
        name: &Ident,
    ) -> Option<Option<Diagnostic>> {
        let names = self.project.native.get(module_path)?;
        if names.contains_key(UNREADABLE) {
            return Some(None);
        }
        let reason = names.get(&name.name)?;
        Some(Some(
            Diagnostic::new(
                codes::E0713,
                format!(
                    "`{}` of the native module `{module_path}` is not available",
                    name.name
                ),
            )
            .primary(name.span, "cannot be used from wolfpack")
            .note(format!("`{}`: {reason}", name.name)),
        ))
    }

    fn import_inner(&mut self, module: ModuleId, role: Role, import: &Import) {
        if import.public && !self.may_reexport(module) {
            self.report(
                Diagnostic::new(codes::E0620, "`pub` import outside `src/lib.wolf`")
                    .primary(
                        import.path.span,
                        "re-exports are allowed only in `src/lib.wolf`",
                    )
                    .help("remove `pub`"),
            );
        }
        let target_path = join(&path_names(&import.path));
        let Some(target) = self.by_path.get(&target_path).copied() else {
            let candidates: Vec<String> = self
                .modules
                .iter()
                .filter(|m| !matches!(m.role, Role::Prelude | Role::Test | Role::Example))
                .map(|m| join(&m.path))
                .collect();
            let mut d = Diagnostic::new(codes::E0602, format!("unknown module `{target_path}`"))
                .primary(import.path.span, "no module with this path");
            if let Some(close) = suggest(&target_path, candidates.iter().map(String::as_str)) {
                d = d.help(format!("did you mean `{close}`?"));
            }
            self.report(d);
            return;
        };
        let target_role = self.module(target).map(|m| m.role).unwrap_or(Role::Src);
        if import.public && target_role == Role::Native {
            self.report(
                Diagnostic::new(
                    codes::E0712,
                    format!("the native module `{target_path}` cannot be re-exported"),
                )
                .primary(import.path.span, "a native module")
                .help(
                    "remove `pub`, or write a wolfpack function in `lib` that calls the native one",
                ),
            );
            return;
        }
        let target_is_lib = self
            .module(target)
            .is_some_and(|m| m.role == Role::Entry && m.path == ["lib"]);
        if target == module {
            self.report(
                Diagnostic::new(codes::E0616, "a module cannot import itself")
                    .primary(import.path.span, "this is the current module"),
            );
            return;
        }
        match target_role {
            Role::Prelude => {
                self.report(
                    Diagnostic::new(
                        codes::E0602,
                        format!("module `{target_path}` cannot be imported"),
                    )
                    .primary(
                        import.path.span,
                        "its names are available everywhere already",
                    ),
                );
                return;
            }
            Role::Test | Role::Example => {
                self.report(
                    Diagnostic::new(
                        codes::E0602,
                        format!("module `{target_path}` cannot be imported"),
                    )
                    .primary(
                        import.path.span,
                        "modules of `tests/` and `examples/` cannot be imported",
                    ),
                );
                return;
            }
            Role::Entry if matches!(role, Role::Src | Role::Entry) => {
                self.report(
                    Diagnostic::new(
                        codes::E0616,
                        format!("import of the entry module `{target_path}`"),
                    )
                    .primary(
                        import.path.span,
                        "modules in `src/` cannot import the entry module",
                    ),
                );
                return;
            }
            _ => {}
        }
        if matches!(role, Role::Test | Role::Example)
            && self.project.kind == Kind::Lib
            && !target_is_lib
            && !matches!(target_role, Role::Std)
        {
            self.report(
                Diagnostic::new(
                    codes::E0607,
                    format!("module `{target_path}` is internal to the library"),
                )
                .primary(
                    import.path.span,
                    "tests and examples of a library see only its API",
                )
                .help("import the names from `lib`"),
            );
            return;
        }
        if matches!(role, Role::Src | Role::Entry) && matches!(target_role, Role::Src | Role::Entry)
        {
            if let Some(state) = self.modules.get_mut(module.0 as usize) {
                state.edges.push((target, import.path.span));
            }
        }
        match &import.names {
            None => {
                let name = match &import.alias {
                    Some(alias) => alias.clone(),
                    None => match import.path.segments.last() {
                        Some(last) => last.clone(),
                        None => return,
                    },
                };
                if import.alias.is_some() {
                    self.style(&name, false, "module alias");
                }
                self.bind_import(module, &name, Binding::Module(target), import.public);
            }
            Some(names) => {
                for imported in names {
                    let found = self
                        .module(target)
                        .and_then(|m| m.exports.get(&imported.name.name).copied());
                    let Some(binding) = found else {
                        if let Some(d) = self.unavailable(&target_path, &imported.name) {
                            if let Some(d) = d {
                                self.report(d);
                            }
                            continue;
                        }
                        let private = self
                            .module(target)
                            .is_some_and(|m| m.items.contains_key(&imported.name.name));
                        let message = if private {
                            format!("`{}` is not public in `{target_path}`", imported.name.name)
                        } else {
                            format!("`{}` does not exist in `{target_path}`", imported.name.name)
                        };
                        let mut d = Diagnostic::new(codes::E0603, message).primary(
                            imported.name.span,
                            if private {
                                "private declaration"
                            } else {
                                "unknown name"
                            },
                        );
                        if private {
                            d = d.help(format!("mark it `pub` in `{target_path}`"));
                        } else if let Some(state) = self.module(target) {
                            if let Some(close) = suggest(
                                &imported.name.name,
                                state.exports.keys().map(String::as_str),
                            ) {
                                d = d.help(format!("did you mean `{close}`?"));
                            }
                        }
                        self.report(d);
                        continue;
                    };
                    let bound = imported
                        .alias
                        .clone()
                        .unwrap_or_else(|| imported.name.clone());
                    if let Some(alias) = &imported.alias {
                        let pascal = match binding {
                            Binding::Def(def) => self.def(def).is_some_and(|d| {
                                d.kind.is_type() || matches!(d.kind, DefKind::Variant { .. })
                            }),
                            Binding::Module(_) => false,
                        };
                        self.style(alias, pascal, "alias");
                    }
                    self.bind_import(module, &bound, binding, import.public);
                }
            }
        }
    }

    fn bind_import(&mut self, module: ModuleId, name: &Ident, binding: Binding, public: bool) {
        let Some(state) = self.modules.get(module.0 as usize) else {
            return;
        };
        let clash = state
            .items
            .get(&name.name)
            .and_then(|def| self.defs.get(def.0 as usize))
            .map(|d| d.span)
            .or_else(|| state.imports.get(&name.name).map(|i| i.span));
        if let Some(first) = clash {
            self.report(
                Diagnostic::new(
                    codes::E0605,
                    format!("`{}` is defined twice in this module", name.name),
                )
                .primary(name.span, "imported here")
                .secondary(first, "also defined here")
                .help(format!(
                    "import it under another name: `as {}`",
                    other_name(&name.name)
                )),
            );
            return;
        }
        let may_reexport = self.may_reexport(module);
        let Some(state) = self.modules.get_mut(module.0 as usize) else {
            return;
        };
        state.imports.insert(
            name.name.clone(),
            ImportBinding {
                binding,
                span: name.span,
                // A re-export is used by the modules that import it.
                used: public && may_reexport,
            },
        );
        if public && may_reexport {
            state.exports.insert(name.name.clone(), binding);
        }
    }

    fn build_prelude(&mut self) {
        let Some(prelude) = self.by_path.get(wolf_std::PRELUDE).copied() else {
            return;
        };
        let exports: Vec<(String, Binding)> = self
            .module(prelude)
            .map(|state| state.exports.iter().map(|(n, b)| (n.clone(), *b)).collect())
            .unwrap_or_default();
        for (name, binding) in exports {
            self.prelude_for_std.insert(name.clone(), binding);
            if !wolf_std::HIDDEN_PRELUDE_NAMES.contains(&name.as_str()) {
                self.prelude.insert(name, binding);
            }
        }
        // Variants of the prelude enums `Option` and `Result` are written without qualification.
        let enums: Vec<DefId> = ["Option", "Result"]
            .iter()
            .filter_map(|n| match self.prelude.get(*n) {
                Some(Binding::Def(def)) => Some(*def),
                _ => None,
            })
            .collect();
        for def in enums {
            if let Some(variants) = self.members.get(&def) {
                for (name, variant) in variants.clone() {
                    self.prelude_for_std
                        .insert(name.clone(), Binding::Def(variant));
                    self.prelude.insert(name, Binding::Def(variant));
                }
            }
        }
    }

    /// Prelude names cannot be declared again at the top level of a module (E0617).
    fn check_prelude_redeclarations(&mut self, module: ModuleId) {
        if self.is_std(module) {
            return;
        }
        let Some(state) = self.module(module) else {
            return;
        };
        let clashes: Vec<(String, Span)> = state
            .items
            .iter()
            .filter(|(name, _)| self.prelude.contains_key(*name))
            .filter_map(|(name, def)| self.def(*def).map(|d| (name.clone(), d.span)))
            .collect();
        let state = self.module(module);
        let helper_clashes: Vec<(String, Span, Option<Span>)> = state
            .map(|s| {
                s.test_items
                    .iter()
                    .filter_map(|(name, def)| {
                        let span = self.def(*def)?.span;
                        let other = s.items.get(name).and_then(|d| self.def(*d)).map(|d| d.span);
                        if other.is_some() || self.prelude.contains_key(name) {
                            Some((name.clone(), span, other))
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        for (name, span, other) in helper_clashes {
            match other {
                Some(first) => self.report(
                    Diagnostic::new(codes::E0605, format!("`{name}` is declared twice"))
                        .primary(span, "test helper with the name of a declaration")
                        .secondary(first, "declared here")
                        .help("rename the helper"),
                ),
                None => self.report(
                    Diagnostic::new(codes::E0617, format!("`{name}` is a prelude name"))
                        .primary(span, "this would hide the prelude name in the tests")
                        .help("choose another name"),
                ),
            }
        }
        for (name, span) in clashes {
            self.report(
                Diagnostic::new(codes::E0617, format!("`{name}` is a prelude name"))
                    .primary(
                        span,
                        "this would hide the prelude name everywhere in the module",
                    )
                    .help("choose another name"),
            );
        }
    }

    /// Registers the functions of inherent `impl` blocks as members of their type.
    fn inherent_impls(&mut self, index: usize, input: &ModuleInput<'_>) {
        let module = ModuleId(index as u32);
        for (s, section) in input.ast.sections.iter().enumerate() {
            for (i, item) in section.items.iter().enumerate() {
                let ItemKind::Impl(block) = &item.kind else {
                    continue;
                };
                let Some(Binding::Def(owner)) =
                    self.lookup_module_level(module, &block.type_name.name, false, true)
                else {
                    continue;
                };
                if !self.def(owner).is_some_and(|d| d.kind.is_type()) {
                    continue;
                }
                self.impl_owners.insert((index, s, i), owner);
                if block.interface.is_some() {
                    continue;
                }
                // Coherence (an impl lives in the module of its type) is checked in M4; members
                // are registered only for the type's own module.
                if self.def(owner).map(|d| d.module) != Some(module) {
                    continue;
                }
                for method in &block.methods {
                    let sig = &method.decl.sig;
                    let is_method = sig
                        .params
                        .first()
                        .is_some_and(|p| matches!(p.kind, ParamKind::SelfParam));
                    let def = self.new_def(
                        DefKind::Member {
                            owner,
                            method: is_method,
                        },
                        &sig.name,
                        module,
                        method.decl.public,
                    );
                    self.members
                        .entry(owner)
                        .or_default()
                        .entry(sig.name.name.clone())
                        .or_insert(def);
                }
            }
        }
    }

    /// Looks up a name at module level: test helpers (in tests), declarations, imports, prelude.
    /// Marks an import as used when `mark` is set.
    pub(crate) fn lookup_module_level(
        &mut self,
        module: ModuleId,
        name: &str,
        in_tests: bool,
        mark: bool,
    ) -> Option<Binding> {
        let std_module = self.is_std(module);
        let state = self.modules.get_mut(module.0 as usize)?;
        if in_tests {
            if let Some(def) = state.test_items.get(name) {
                return Some(Binding::Def(*def));
            }
        }
        if let Some(def) = state.items.get(name) {
            return Some(Binding::Def(*def));
        }
        if let Some(import) = state.imports.get_mut(name) {
            if mark {
                import.used = true;
            }
            return Some(import.binding);
        }
        if std_module {
            return self.prelude_for_std.get(name).copied();
        }
        self.prelude.get(name).copied()
    }

    // ----- cycles ---------------------------------------------------------------------------

    /// Reports each import cycle once, starting at its alphabetically first module (E0604).
    fn cycles(&mut self) {
        let count = self.modules.len();
        let mut reported: BTreeSet<Vec<ModuleId>> = BTreeSet::new();
        let mut starts: Vec<ModuleId> = (0..count).map(|i| ModuleId(i as u32)).collect();
        starts.sort_by_key(|m| self.module(*m).map(|s| join(&s.path)));
        for start in starts {
            let Some(cycle) = self.find_cycle(start) else {
                continue;
            };
            let mut key = cycle.clone();
            key.sort();
            if !reported.insert(key) {
                continue;
            }
            // Rotate to start at the smallest path.
            let names: Vec<String> = cycle
                .iter()
                .map(|m| self.module(*m).map(|s| join(&s.path)).unwrap_or_default())
                .collect();
            let first = names
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.cmp(b.1))
                .map(|(i, _)| i)
                .unwrap_or(0);
            let mut ordered: Vec<ModuleId> = cycle.iter().skip(first).copied().collect();
            ordered.extend(cycle.iter().take(first).copied());
            let mut text: Vec<String> = ordered
                .iter()
                .map(|m| self.module(*m).map(|s| join(&s.path)).unwrap_or_default())
                .collect();
            if let Some(head) = text.first().cloned() {
                text.push(head);
            }
            let (Some(a), Some(b)) = (ordered.first(), ordered.get(1).or(ordered.first())) else {
                continue;
            };
            let span = self
                .module(*a)
                .and_then(|s| s.edges.iter().find(|(t, _)| t == b).map(|(_, span)| *span));
            let Some(span) = span else {
                continue;
            };
            self.report(
                Diagnostic::new(codes::E0604, format!("import cycle: {}", text.join(" -> ")))
                    .primary(span, "this import closes the cycle")
                    .help("move the shared declarations into a module that both can import"),
            );
        }
    }

    /// A cycle through `start`, as the list of modules on it, if one exists.
    fn find_cycle(&self, start: ModuleId) -> Option<Vec<ModuleId>> {
        // Breadth-first search from the successors of `start` back to `start`, for the shortest cycle.
        let mut previous: BTreeMap<ModuleId, ModuleId> = BTreeMap::new();
        let mut queue = std::collections::VecDeque::new();
        let mut targets: Vec<ModuleId> =
            self.module(start)?.edges.iter().map(|(t, _)| *t).collect();
        targets.sort();
        targets.dedup();
        for t in targets {
            if t == start {
                return Some(vec![start]);
            }
            if previous.insert(t, start).is_none() {
                queue.push_back(t);
            }
        }
        while let Some(current) = queue.pop_front() {
            let mut next: Vec<ModuleId> = self
                .module(current)?
                .edges
                .iter()
                .map(|(t, _)| *t)
                .collect();
            next.sort();
            next.dedup();
            for t in next {
                if t == start {
                    let mut path = vec![current];
                    let mut node = current;
                    while let Some(p) = previous.get(&node) {
                        if *p == start {
                            break;
                        }
                        path.push(*p);
                        node = *p;
                    }
                    path.push(start);
                    path.reverse();
                    return Some(path);
                }
                if let std::collections::btree_map::Entry::Vacant(e) = previous.entry(t) {
                    e.insert(current);
                    queue.push_back(t);
                }
            }
        }
        None
    }
}
