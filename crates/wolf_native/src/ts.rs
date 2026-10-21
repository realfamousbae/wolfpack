//! The reader of TypeScript files of native modules (spec chapter 7, section 8.7, rules 12 to 16):
//! the exported declarations of `native/ts/<path>.ts`, parsed with `oxc_parser`.

use std::collections::{BTreeMap, BTreeSet};

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    BindingPatternKind, ClassElement, Declaration, FormalParameters, Function,
    MethodDefinitionKind, Statement, TSAccessibility, TSSignature, TSType, TSTypeName,
    TSTypeOperatorOperator,
};
use oxc_parser::Parser;
use oxc_span::SourceType;
use wolf_ast::{NativeSig, NativeTy};

use crate::{
    Function as NativeFn, Method, MethodKind, Module, ReadError, Record, RecordField, Wolf, snake,
};

/// Where a type appears, which decides the forms it may take.
#[derive(Clone, Copy, PartialEq)]
enum Pos {
    /// A parameter.
    Param,
    /// The result.
    Ret,
    /// Inside a list, a map, an `Option` or a field.
    Inner,
}

/// Reads the exported declarations of a TypeScript file of `native/ts/` (spec chapter 7, section
/// 8.7, rules 12 to 16). A `number` is read as `Float`; the agreement of targets may make it
/// `Int` (rule 14).
pub fn read_ts(source: &str) -> Result<Module, ReadError> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
    if let Some(e) = parsed.errors.first() {
        let offset = e
            .labels
            .as_ref()
            .and_then(|l| l.first())
            .map_or(0, |l| l.offset());
        let (line, column) = position(source, offset);
        return Err(ReadError {
            message: e.message.to_string(),
            line,
            column,
        });
    }
    if parsed.panicked {
        return Err(ReadError {
            message: "the file cannot be parsed".to_string(),
            line: 1,
            column: 1,
        });
    }
    let mut decls = Vec::new();
    for stmt in &parsed.program.body {
        if let Statement::ExportNamedDeclaration(e) = stmt {
            if let Some(d) = &e.declaration {
                decls.push(d);
            }
        }
    }
    let mut module = Module::default();
    // Types first: object types whose members are all fields may be records; the others are
    // opaque.
    let mut opaque = BTreeSet::new();
    let mut candidates: Vec<(String, &oxc_allocator::Vec<'_, TSSignature<'_>>)> = Vec::new();
    for d in &decls {
        let (name, generic, members) = match d {
            Declaration::TSInterfaceDeclaration(i) => (
                i.id.name.to_string(),
                i.type_parameters.is_some(),
                (i.extends.is_empty()).then_some(&i.body.body),
            ),
            Declaration::TSTypeAliasDeclaration(t) => (
                t.id.name.to_string(),
                t.type_parameters.is_some(),
                match &t.type_annotation {
                    TSType::TSTypeLiteral(l) => Some(&l.members),
                    _ => None,
                },
            ),
            Declaration::ClassDeclaration(c) => match &c.id {
                Some(id) => (id.name.to_string(), c.type_parameters.is_some(), None),
                None => continue,
            },
            Declaration::TSEnumDeclaration(e) => (e.id.name.to_string(), false, None),
            _ => continue,
        };
        if generic {
            module
                .unavailable
                .insert(name, "it is a generic type".to_string());
            continue;
        }
        match members {
            Some(m) if !m.is_empty() && m.iter().all(is_field) => candidates.push((name, m)),
            _ => {
                opaque.insert(name.clone());
                module.types.push(name);
            }
        }
    }
    // A record whose field uses a type that is not a record is not one either: repeat until
    // nothing changes.
    let mut records: BTreeSet<String> = candidates.iter().map(|(n, _)| n.clone()).collect();
    let mut unavailable: BTreeMap<String, String> = BTreeMap::new();
    loop {
        let reader = Reader {
            opaque: &opaque,
            records: &records,
        };
        let failed: Vec<(String, Option<String>)> = candidates
            .iter()
            .filter(|(n, _)| records.contains(n))
            .filter_map(|(n, m)| reader.record(n, m).err().map(|e| (n.clone(), e)))
            .collect();
        if failed.is_empty() {
            break;
        }
        for (n, names) in failed {
            records.remove(&n);
            match names {
                // Field names that collide: the type is not available at all.
                Some(reason) => {
                    unavailable.insert(n, reason);
                }
                None => {
                    opaque.insert(n);
                }
            }
        }
    }
    let reader = Reader {
        opaque: &opaque,
        records: &records,
    };
    for (name, members) in &candidates {
        if let Ok(r) = reader.record(name, members) {
            module.records.push(r);
        } else if let Some(reason) = unavailable.remove(name) {
            module.unavailable.insert(name.clone(), reason);
        } else {
            module.types.push(name.clone());
        }
    }
    // Functions: a name declared more than once has overloads.
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for d in &decls {
        if let Declaration::FunctionDeclaration(f) = d {
            if let Some(id) = &f.id {
                *count.entry(id.name.to_string()).or_default() += 1;
            }
        }
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut done: BTreeSet<String> = BTreeSet::new();
    for d in &decls {
        let Declaration::FunctionDeclaration(f) = d else {
            continue;
        };
        let Some(id) = &f.id else {
            continue;
        };
        let name = id.name.to_string();
        if !done.insert(name.clone()) {
            continue;
        }
        let wolf_name = snake(&name);
        if !seen.insert(wolf_name.clone()) {
            module.fns.retain(|f| f.name != wolf_name);
            module.unavailable.insert(
                wolf_name,
                format!("two exports have this name in `snake_case`, one of them `{name}`"),
            );
            continue;
        }
        let result = if count.get(&name).copied().unwrap_or(0) > 1 {
            Err("it has overloads".to_string())
        } else {
            reader.function(&wolf_name, &name, f)
        };
        match result {
            Ok(f) => module.fns.push(f),
            Err(reason) => {
                module.unavailable.insert(wolf_name, reason);
            }
        }
    }
    // Methods, static methods and constructors of the classes (rule 17).
    for d in &decls {
        let Declaration::ClassDeclaration(c) = d else {
            continue;
        };
        let Some(id) = &c.id else {
            continue;
        };
        let owner = id.name.to_string();
        if !module.types.contains(&owner) {
            continue;
        }
        let constructors = c
            .body
            .body
            .iter()
            .filter(|e| {
                matches!(e, ClassElement::MethodDefinition(m) if m.kind == MethodDefinitionKind::Constructor)
            })
            .count();
        for e in &c.body.body {
            let ClassElement::MethodDefinition(m) = e else {
                continue;
            };
            let Some(name) = m.key.static_name() else {
                continue;
            };
            let constructor = m.kind == MethodDefinitionKind::Constructor;
            let wolf_name = if constructor {
                "new".to_string()
            } else {
                snake(&name)
            };
            let key = format!("{owner}.{wolf_name}");
            let private = m.key.is_private_identifier()
                || matches!(
                    m.accessibility,
                    Some(TSAccessibility::Private | TSAccessibility::Protected)
                );
            let result = if private {
                Err("it is private".to_string())
            } else if matches!(
                m.kind,
                MethodDefinitionKind::Get | MethodDefinitionKind::Set
            ) {
                Err("it is a getter or a setter".to_string())
            } else if constructor && constructors > 1 {
                Err("it has overloads".to_string())
            } else if constructor {
                reader.constructor(&owner, &m.value)
            } else {
                reader.function(&wolf_name, &name, &m.value)
            };
            let kind = if constructor {
                MethodKind::Constructor
            } else if m.r#static {
                MethodKind::Assoc
            } else {
                MethodKind::Method
            };
            match result {
                Ok(mut func) => {
                    if kind == MethodKind::Method {
                        func.native.params.insert(
                            0,
                            NativeTy::Named {
                                name: owner.clone(),
                                by_ref: false,
                            },
                        );
                    }
                    module
                        .methods
                        .retain(|x| !(x.owner == owner && x.func.name == wolf_name));
                    module.methods.push(Method {
                        owner: owner.clone(),
                        kind,
                        func,
                    });
                }
                Err(reason) => {
                    module.unavailable.insert(key, reason);
                }
            }
        }
    }
    Ok(module)
}

/// The line and column, from 1, of a byte offset.
fn position(source: &str, offset: usize) -> (usize, usize) {
    let before = source.get(..offset).unwrap_or(source);
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, column)
}

/// Whether a member of an object type is a plain field: `name: T`, not optional, not computed.
fn is_field(m: &TSSignature<'_>) -> bool {
    match m {
        TSSignature::TSPropertySignature(p) => {
            !p.computed && !p.optional && p.type_annotation.is_some() && p.key.is_identifier()
        }
        _ => false,
    }
}

struct Reader<'a> {
    opaque: &'a BTreeSet<String>,
    records: &'a BTreeSet<String>,
}

impl Reader<'_> {
    /// An object type of fields as a record (rule 12): the fields in `snake_case`, with the types
    /// of rule 13. `Err(Some(reason))` when two fields collide, `Err(None)` when a field does not
    /// map.
    fn record(
        &self,
        name: &str,
        members: &oxc_allocator::Vec<'_, TSSignature<'_>>,
    ) -> Result<Record, Option<String>> {
        let mut fields: Vec<RecordField> = Vec::new();
        for m in members {
            let TSSignature::TSPropertySignature(p) = m else {
                return Err(None);
            };
            let Some(field) = p.key.static_name() else {
                return Err(None);
            };
            let Some(t) = &p.type_annotation else {
                return Err(None);
            };
            let wolf = snake(&field);
            if let Some(other) = fields.iter().find(|f| f.name == wolf) {
                return Err(Some(format!(
                    "its fields `{}` and `{field}` are both `{wolf}` in `snake_case`",
                    other.target_name
                )));
            }
            let (ty, native) = self.ty(&t.type_annotation, Pos::Inner).map_err(|_| None)?;
            fields.push(RecordField {
                name: wolf,
                target_name: field.to_string(),
                ty,
                native,
            });
        }
        Ok(Record {
            name: name.to_string(),
            fields,
        })
    }

    /// The `constructor` of a class: the associated function `new`, whose result is the class.
    fn constructor(&self, owner: &str, f: &Function<'_>) -> Result<NativeFn, String> {
        let (params, natives) = self.params(&f.params, Pos::Param)?;
        Ok(NativeFn {
            name: "new".to_string(),
            target_name: "constructor".to_string(),
            params,
            ret: Wolf::Named(owner.to_string()),
            native: NativeSig {
                params: natives,
                ret: NativeTy::Named {
                    name: owner.to_string(),
                    by_ref: false,
                },
            },
        })
    }

    fn function(&self, wolf_name: &str, name: &str, f: &Function<'_>) -> Result<NativeFn, String> {
        if f.r#async {
            return Err("it is `async`".to_string());
        }
        if f.generator {
            return Err("it is a generator".to_string());
        }
        if f.type_parameters.is_some() {
            return Err("it has type parameters".to_string());
        }
        if f.body.is_none() && !f.declare {
            return Err("it has no body".to_string());
        }
        let (params, natives) = self.params(&f.params, Pos::Param)?;
        let (ret, native_ret) = match &f.return_type {
            Some(t) => self
                .ty(&t.type_annotation, Pos::Ret)
                .map_err(|r| format!("its result: {r}"))?,
            None => return Err("its result has no written type".to_string()),
        };
        Ok(NativeFn {
            name: wolf_name.to_string(),
            target_name: name.to_string(),
            params,
            ret,
            native: NativeSig {
                params: natives,
                ret: native_ret,
            },
        })
    }

    /// The parameters of a function: name and wolfpack type, and the TypeScript types.
    #[allow(clippy::type_complexity)]
    fn params(
        &self,
        params: &FormalParameters<'_>,
        pos: Pos,
    ) -> Result<(Vec<(String, Wolf)>, Vec<NativeTy>), String> {
        if params.rest.is_some() {
            return Err("it has a rest parameter".to_string());
        }
        let mut out = Vec::new();
        let mut natives = Vec::new();
        for (i, p) in params.items.iter().enumerate() {
            let name = match &p.pattern.kind {
                BindingPatternKind::BindingIdentifier(id) => id.name.to_string(),
                BindingPatternKind::AssignmentPattern(_) => {
                    return Err(format!("its parameter {} has a default value", i + 1));
                }
                _ => format!("arg{i}"),
            };
            let Some(t) = &p.pattern.type_annotation else {
                return Err(format!("its parameter `{name}` has no type"));
            };
            let (w, n) = self
                .ty(&t.type_annotation, pos)
                .map_err(|r| format!("its parameter `{name}`: {r}"))?;
            // `x?: T` is `Option<T>` (rule 13).
            let (w, n) = if p.pattern.optional {
                if matches!(w, Wolf::Option(_)) {
                    return Err(format!("its parameter `{name}` is optional twice"));
                }
                (Wolf::Option(Box::new(w)), NativeTy::Option(Box::new(n)))
            } else {
                (w, n)
            };
            out.push((snake(&name), w));
            natives.push(n);
        }
        Ok((out, natives))
    }

    /// A TypeScript type as a wolfpack type, at the place `pos`.
    fn ty(&self, t: &TSType<'_>, pos: Pos) -> Result<(Wolf, NativeTy), String> {
        match t {
            TSType::TSParenthesizedType(p) => self.ty(&p.type_annotation, pos),
            TSType::TSBigIntKeyword(_) => Ok((Wolf::Int, NativeTy::Int("bigint".to_string()))),
            TSType::TSNumberKeyword(_) => Ok((Wolf::Float, NativeTy::Float("number".to_string()))),
            TSType::TSBooleanKeyword(_) => Ok((Wolf::Bool, NativeTy::Bool)),
            TSType::TSStringKeyword(_) => Ok((Wolf::String, NativeTy::String)),
            TSType::TSVoidKeyword(_) if pos == Pos::Ret => Ok((Wolf::Void, NativeTy::Unit)),
            TSType::TSVoidKeyword(_) => Err("`void` that is not the result".to_string()),
            TSType::TSArrayType(a) => self.list(&a.element_type),
            TSType::TSTypeOperatorType(o) if o.operator == TSTypeOperatorOperator::Readonly => {
                match &o.type_annotation {
                    TSType::TSArrayType(a) => self.readonly(&a.element_type),
                    _ => Err("a `readonly` type that is not an array".to_string()),
                }
            }
            TSType::TSUnionType(u) => self.union(&u.types, pos),
            TSType::TSFunctionType(f) if pos == Pos::Param => {
                if f.type_parameters.is_some() {
                    return Err("a generic function type".to_string());
                }
                let (params, _) = self.params(&f.params, Pos::Param)?;
                let (ret, _) = self.ty(&f.return_type.type_annotation, Pos::Ret)?;
                let params: Vec<Wolf> = params.into_iter().map(|(_, w)| w).collect();
                if !params.iter().all(callback_ty) || !(ret == Wolf::Void || callback_ty(&ret)) {
                    return Err("a callback that wolfpack does not map: callbacks take and return `number`, `boolean`, `string`, arrays of these, or types of the module".to_string());
                }
                Ok((Wolf::Fn(params, Box::new(ret)), NativeTy::Callback))
            }
            TSType::TSFunctionType(_) => Err("a function that is not a parameter".to_string()),
            TSType::TSTypeReference(r) => {
                let TSTypeName::IdentifierReference(id) = &r.type_name else {
                    return Err("a type of another module".to_string());
                };
                let name = id.name.to_string();
                let args: Vec<&TSType<'_>> = r
                    .type_arguments
                    .as_ref()
                    .map(|a| a.params.iter().collect())
                    .unwrap_or_default();
                match (name.as_str(), args.as_slice()) {
                    ("Array", [x]) => self.list(x),
                    ("ReadonlyArray", [x]) => self.readonly(x),
                    ("Map" | "ReadonlyMap", [k, v]) => self.map(k, v, "Map"),
                    ("Record", [k, v]) if pos == Pos::Ret => match k {
                        TSType::TSStringKeyword(_) => self.map(k, v, "Record"),
                        _ => Err("a `Record` whose keys are not `string`".to_string()),
                    },
                    ("Record", _) => Err("a `Record` that is not the result".to_string()),
                    (n, []) if self.records.contains(n) => Ok((
                        Wolf::Named(name.clone()),
                        NativeTy::Record {
                            name,
                            by_ref: false,
                        },
                    )),
                    (n, []) if self.opaque.contains(n) => {
                        if pos == Pos::Inner {
                            Err(format!(
                                "`{n}` inside a list or a map, where values of extern types cannot be copied"
                            ))
                        } else {
                            Ok((
                                Wolf::Named(name.clone()),
                                NativeTy::Named {
                                    name,
                                    by_ref: false,
                                },
                            ))
                        }
                    }
                    _ => Err(format!("the type `{name}`, which wolfpack does not map")),
                }
            }
            _ => Err("a type that wolfpack does not map".to_string()),
        }
    }

    fn list(&self, element: &TSType<'_>) -> Result<(Wolf, NativeTy), String> {
        let (w, n) = self.ty(element, Pos::Inner)?;
        Ok((Wolf::List(Box::new(w)), NativeTy::Vec(Box::new(n))))
    }

    /// A `readonly` array: a list that the TypeScript code does not change (`NativeTy::Slice`).
    fn readonly(&self, element: &TSType<'_>) -> Result<(Wolf, NativeTy), String> {
        let (w, n) = self.ty(element, Pos::Inner)?;
        Ok((Wolf::List(Box::new(w)), NativeTy::Slice(Box::new(n))))
    }

    fn map(&self, k: &TSType<'_>, v: &TSType<'_>, kind: &str) -> Result<(Wolf, NativeTy), String> {
        let (kw, kn) = self.ty(k, Pos::Inner)?;
        // A `number` key may become `Int` (rule 14); the loader checks it after.
        if !matches!(kw, Wolf::Int | Wolf::String | Wolf::Bool | Wolf::Float) {
            return Err("a map whose keys are not numbers, strings or `boolean`".to_string());
        }
        let (vw, vn) = self.ty(v, Pos::Inner)?;
        Ok((
            Wolf::Map(Box::new(kw), Box::new(vw)),
            NativeTy::Map {
                kind: kind.to_string(),
                by_ref: false,
                key: Box::new(kn),
                value: Box::new(vn),
            },
        ))
    }

    /// `T | undefined` (and, as a result, `T | null`) is `Option<T>`; other unions do not map.
    fn union(
        &self,
        types: &oxc_allocator::Vec<'_, TSType<'_>>,
        pos: Pos,
    ) -> Result<(Wolf, NativeTy), String> {
        let mut undefined = false;
        let mut null = false;
        let mut rest = Vec::new();
        for t in types {
            match t {
                TSType::TSUndefinedKeyword(_) => undefined = true,
                TSType::TSNullKeyword(_) => null = true,
                other => rest.push(other),
            }
        }
        let [inner] = rest.as_slice() else {
            return Err("a union that is not `T | undefined`".to_string());
        };
        if null && pos != Pos::Ret {
            return Err(
                "`null` in a type that is not the result: write `T | undefined`".to_string(),
            );
        }
        if !(undefined || null) {
            return Err("a union that is not `T | undefined`".to_string());
        }
        let inner_pos = if pos == Pos::Param {
            Pos::Param
        } else {
            Pos::Inner
        };
        let (w, n) = self.ty(inner, inner_pos)?;
        if matches!(w, Wolf::Option(_)) {
            return Err("an optional `Option`".to_string());
        }
        Ok((Wolf::Option(Box::new(w)), NativeTy::Option(Box::new(n))))
    }
}

/// A type of a callback parameter or result (section 8.4, rule 7). A callback takes an `Int` as a
/// `number` (chapter 8, section 8), so `bigint` is not one of them.
fn callback_ty(w: &Wolf) -> bool {
    match w {
        Wolf::Float | Wolf::Bool | Wolf::String | Wolf::Named(_) => true,
        Wolf::List(x) => matches!(**x, Wolf::Float | Wolf::Bool | Wolf::String),
        _ => false,
    }
}
