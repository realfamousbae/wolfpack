//! Readers of native modules (spec chapter 7, section 8.7): the declarations of the target code in
//! `native/<target>/`, mapped to wolfpack. Only declarations are read, never bodies, and no tool of
//! the target is needed.
//!
//! In 0.2 the reader of Rust is here (step N3a of `docs/plans/native-code.md`). It turns a Rust
//! file into a [`Module`]: the functions and opaque types that wolfpack can call, and the items it
//! cannot, each with the reason. The driver writes the available ones as an `extern` block of a
//! wolfpack module, so that name resolution, types and the backends treat them as any extern
//! declaration.

use std::collections::{BTreeMap, BTreeSet};

mod go;
mod ts;

pub use go::{read_go, snake};
pub use ts::read_ts;

use syn::{
    FnArg, GenericArgument, GenericParam, Item, PathArguments, ReturnType, Type, TypeParamBound,
    Visibility,
};
use wolf_ast::{NativeSig, NativeTy};

/// The declarations read from a native file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Module {
    /// The functions wolfpack can call, in the order of the file.
    pub fns: Vec<Function>,
    /// The opaque types, in the order of the file.
    pub types: Vec<String>,
    /// The opaque types used through a pointer (Go struct types: `*X`).
    pub pointers: BTreeSet<String>,
    /// The records: structs copied field by field at the boundary, in the order of the file.
    pub records: Vec<Record>,
    /// The public items wolfpack cannot use, with the reason.
    pub unavailable: BTreeMap<String, String>,
}

/// A function of a native module.
#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    /// Its wolfpack name.
    pub name: String,
    /// Its name in the target code (`CountWords` for `count_words` in Go).
    pub target_name: String,
    /// The parameters: name and wolfpack type.
    pub params: Vec<(String, Wolf)>,
    /// The wolfpack result type.
    pub ret: Wolf,
    /// The Rust types that the generated call must match.
    pub native: NativeSig,
}

/// A record of a native module (spec chapter 7, section 8.7, rules 2 and 8).
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    /// Its name, the same in wolfpack and in the target code.
    pub name: String,
    /// Its fields, in order.
    pub fields: Vec<RecordField>,
}

/// A field of a record.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordField {
    /// Its wolfpack name.
    pub name: String,
    /// Its name in the target code (`MaxSize` for `max_size` in Go).
    pub target_name: String,
    /// Its wolfpack type.
    pub ty: Wolf,
    /// Its type in the target code.
    pub native: NativeTy,
}

/// A wolfpack type, as the reader derives it.
#[derive(Clone, Debug, PartialEq)]
pub enum Wolf {
    /// `Int`
    Int,
    /// `Float`
    Float,
    /// `Bool`
    Bool,
    /// `String`
    String,
    /// `Void`
    Void,
    /// `List<T>`
    List(Box<Wolf>),
    /// `Map<K, V>`
    Map(Box<Wolf>, Box<Wolf>),
    /// `Option<T>`
    Option(Box<Wolf>),
    /// `Result<T, Error>`
    Result(Box<Wolf>),
    /// An opaque type or a record of the module.
    Named(String),
    /// A callback `fn(A, ..) -> R`.
    Fn(Vec<Wolf>, Box<Wolf>),
}

impl Wolf {
    /// The type as wolfpack code writes it.
    pub fn text(&self) -> String {
        match self {
            Wolf::Int => "Int".to_string(),
            Wolf::Float => "Float".to_string(),
            Wolf::Bool => "Bool".to_string(),
            Wolf::String => "String".to_string(),
            Wolf::Void => "Void".to_string(),
            Wolf::List(t) => format!("List<{}>", t.text()),
            Wolf::Map(k, v) => format!("Map<{}, {}>", k.text(), v.text()),
            Wolf::Option(t) => format!("Option<{}>", t.text()),
            Wolf::Result(t) => format!("Result<{}, Error>", t.text()),
            Wolf::Named(n) => n.clone(),
            Wolf::Fn(params, ret) => {
                let params: Vec<String> = params.iter().map(Wolf::text).collect();
                format!("fn({}) -> {}", params.join(", "), ret.text())
            }
        }
    }
}

/// A Rust file that cannot be parsed (E0714).
#[derive(Clone, Debug, PartialEq)]
pub struct ReadError {
    /// The message of the parser.
    pub message: String,
    /// The line, from 1.
    pub line: usize,
    /// The column, from 1.
    pub column: usize,
}

/// Where a type appears, which decides the forms it may take.
#[derive(Clone, Copy, PartialEq)]
enum Pos {
    /// A parameter.
    Param,
    /// The result.
    Ret,
    /// Directly inside an `Option` or the `Ok` of a `Result` that is the result.
    RetInner,
    /// Inside a list or a map.
    Inner,
}

/// Reads the declarations of a Rust file of `native/rust/` (spec chapter 7, section 8.7).
pub fn read_rust(source: &str) -> Result<Module, ReadError> {
    let file = syn::parse_file(source).map_err(|e| {
        let start = e.span().start();
        ReadError {
            message: e.to_string(),
            line: start.line,
            column: start.column + 1,
        }
    })?;
    let mut module = Module::default();
    // Types first: functions refer to them in any order. A struct with only `pub` named fields is
    // a record if every field maps; the others are opaque.
    let mut opaque = BTreeSet::new();
    let mut candidates: Vec<(String, &syn::FieldsNamed)> = Vec::new();
    for item in &file.items {
        let (vis, name, attrs, fields) = match item {
            Item::Struct(s) => {
                let fields = match &s.fields {
                    syn::Fields::Named(f)
                        if s.generics.params.is_empty()
                            && !f.named.is_empty()
                            && f.named
                                .iter()
                                .all(|f| matches!(f.vis, Visibility::Public(_))) =>
                    {
                        Some(f)
                    }
                    _ => None,
                };
                (&s.vis, s.ident.to_string(), &s.attrs, fields)
            }
            Item::Enum(e) => (&e.vis, e.ident.to_string(), &e.attrs, None),
            Item::Type(t) => (&t.vis, t.ident.to_string(), &t.attrs, None),
            _ => continue,
        };
        if !matches!(vis, Visibility::Public(_)) || has_cfg(attrs) {
            continue;
        }
        match fields {
            Some(f) => candidates.push((name, f)),
            None => {
                opaque.insert(name.clone());
                module.types.push(name);
            }
        }
    }
    // A record whose field uses a struct that is not a record is not one either: repeat until
    // nothing changes.
    let mut records: BTreeSet<String> = candidates.iter().map(|(n, _)| n.clone()).collect();
    loop {
        let reader = Reader {
            opaque: &opaque,
            records: &records,
        };
        let failed: Vec<String> = candidates
            .iter()
            .filter(|(n, f)| records.contains(n) && reader.record(n, f).is_err())
            .map(|(n, _)| n.clone())
            .collect();
        if failed.is_empty() {
            break;
        }
        for n in failed {
            records.remove(&n);
            opaque.insert(n);
        }
    }
    let reader = Reader {
        opaque: &opaque,
        records: &records,
    };
    for (name, fields) in &candidates {
        match reader.record(name, fields) {
            Ok(r) => module.records.push(r),
            Err(_) => module.types.push(name.clone()),
        }
    }
    for item in &file.items {
        let Item::Fn(f) = item else {
            continue;
        };
        if !matches!(f.vis, Visibility::Public(_)) || has_cfg(&f.attrs) {
            continue;
        }
        let name = f.sig.ident.to_string();
        match reader.function(&f.sig) {
            Ok(func) => module.fns.push(func),
            Err(reason) => {
                module.unavailable.insert(name, reason);
            }
        }
    }
    Ok(module)
}

fn has_cfg(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("cfg"))
}

struct Reader<'a> {
    opaque: &'a BTreeSet<String>,
    records: &'a BTreeSet<String>,
}

impl Reader<'_> {
    /// A struct with only `pub` named fields as a record, if every field maps (rule 2).
    fn record(&self, name: &str, fields: &syn::FieldsNamed) -> Result<Record, String> {
        let mut out = Vec::new();
        for f in &fields.named {
            let field = f
                .ident
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default();
            let (ty, native) = self
                .ty(&f.ty, Pos::Inner, &BTreeMap::new())
                .map_err(|r| format!("its field `{field}`: {r}"))?;
            out.push(RecordField {
                name: field.clone(),
                target_name: field,
                ty,
                native,
            });
        }
        Ok(Record {
            name: name.to_string(),
            fields: out,
        })
    }

    fn function(&self, sig: &syn::Signature) -> Result<Function, String> {
        if sig.asyncness.is_some() {
            return Err("it is `async`".to_string());
        }
        if sig.unsafety.is_some() {
            return Err("it is `unsafe`".to_string());
        }
        if sig.variadic.is_some() {
            return Err("it is variadic".to_string());
        }
        // Type parameters are allowed only for callbacks: `F: Fn(A) -> R`.
        let mut callbacks: BTreeMap<String, syn::ParenthesizedGenericArguments> = BTreeMap::new();
        for p in &sig.generics.params {
            match p {
                GenericParam::Type(t) => {
                    let mut bound = fn_bound(t.bounds.iter());
                    if let Some(w) = &sig.generics.where_clause {
                        for pred in &w.predicates {
                            if let syn::WherePredicate::Type(pt) = pred {
                                if matches!(&pt.bounded_ty, Type::Path(tp) if tp.path.is_ident(&t.ident))
                                {
                                    bound = bound.or_else(|| fn_bound(pt.bounds.iter()));
                                }
                            }
                        }
                    }
                    match bound {
                        Some(args) => {
                            callbacks.insert(t.ident.to_string(), args);
                        }
                        None => {
                            return Err(format!(
                                "it is generic over `{}`, which is not a callback (`Fn`)",
                                t.ident
                            ));
                        }
                    }
                }
                GenericParam::Lifetime(_) => return Err("it has lifetime parameters".to_string()),
                GenericParam::Const(_) => return Err("it has const parameters".to_string()),
            }
        }
        let mut params = Vec::new();
        let mut natives = Vec::new();
        for (i, arg) in sig.inputs.iter().enumerate() {
            let FnArg::Typed(pt) = arg else {
                return Err("it takes `self`".to_string());
            };
            let name = match pt.pat.as_ref() {
                syn::Pat::Ident(p) => p.ident.to_string(),
                _ => format!("arg{i}"),
            };
            let (w, n) = self
                .ty(&pt.ty, Pos::Param, &callbacks)
                .map_err(|reason| format!("its parameter `{name}`: {reason}"))?;
            params.push((name, w));
            natives.push(n);
        }
        let (ret, native_ret) = match &sig.output {
            ReturnType::Default => (Wolf::Void, NativeTy::Unit),
            ReturnType::Type(_, t) => self
                .ty(t, Pos::Ret, &callbacks)
                .map_err(|reason| format!("its result: {reason}"))?,
        };
        Ok(Function {
            name: sig.ident.to_string(),
            target_name: sig.ident.to_string(),
            params,
            ret,
            native: NativeSig {
                params: natives,
                ret: native_ret,
            },
        })
    }

    fn ty(
        &self,
        ty: &Type,
        pos: Pos,
        callbacks: &BTreeMap<String, syn::ParenthesizedGenericArguments>,
    ) -> Result<(Wolf, NativeTy), String> {
        match ty {
            Type::Paren(p) => self.ty(&p.elem, pos, callbacks),
            Type::Group(g) => self.ty(&g.elem, pos, callbacks),
            Type::Tuple(t) if t.elems.is_empty() => Ok((Wolf::Void, NativeTy::Unit)),
            Type::Reference(r) => {
                if r.mutability.is_some() {
                    return Err(
                        "a `&mut` reference: extern code cannot change wolfpack values".to_string(),
                    );
                }
                if pos != Pos::Param {
                    return Err("a reference, which wolfpack cannot keep".to_string());
                }
                if r.lifetime.as_ref().is_some_and(|l| l.ident != "_") {
                    return Err("a reference with a named lifetime".to_string());
                }
                self.reference(&r.elem, callbacks)
            }
            Type::ImplTrait(t) if pos == Pos::Param => match fn_bound(t.bounds.iter()) {
                Some(args) => self.callback(&args),
                None => Err("an `impl` type that is not `Fn`".to_string()),
            },
            Type::Path(tp) if tp.qself.is_none() => self.path(&tp.path, pos, callbacks),
            _ => Err("a type that wolfpack does not map".to_string()),
        }
    }

    /// The type behind `&` of a parameter.
    fn reference(
        &self,
        elem: &Type,
        callbacks: &BTreeMap<String, syn::ParenthesizedGenericArguments>,
    ) -> Result<(Wolf, NativeTy), String> {
        match elem {
            Type::Slice(s) => {
                let (w, n) = self.ty(&s.elem, Pos::Inner, callbacks)?;
                Ok((Wolf::List(Box::new(w)), NativeTy::Slice(Box::new(n))))
            }
            Type::TraitObject(t) => match fn_bound(t.bounds.iter()) {
                Some(args) => self.callback(&args),
                None => Err("a `dyn` type that is not `Fn`".to_string()),
            },
            Type::Path(tp) if tp.qself.is_none() => {
                let Some(last) = tp.path.segments.last() else {
                    return Err("an empty path".to_string());
                };
                let name = last.ident.to_string();
                match name.as_str() {
                    "str" => Ok((Wolf::String, NativeTy::Str)),
                    "String" => Ok((Wolf::String, NativeTy::RefString)),
                    "Vec" => {
                        let (w, n) = self.ty(one_arg(&last.arguments)?, Pos::Inner, callbacks)?;
                        Ok((Wolf::List(Box::new(w)), NativeTy::RefVec(Box::new(n))))
                    }
                    "HashMap" => self.map(&last.arguments, "HashMap", true, callbacks),
                    _ if tp.path.segments.len() == 1 && self.records.contains(&name) => Ok((
                        Wolf::Named(name.clone()),
                        NativeTy::Record { name, by_ref: true },
                    )),
                    _ if tp.path.segments.len() == 1 && self.opaque.contains(&name) => Ok((
                        Wolf::Named(name.clone()),
                        NativeTy::Named { name, by_ref: true },
                    )),
                    _ => Err(format!(
                        "a reference to `{name}`, which wolfpack does not map"
                    )),
                }
            }
            _ => Err("a reference to a type that wolfpack does not map".to_string()),
        }
    }

    fn path(
        &self,
        path: &syn::Path,
        pos: Pos,
        callbacks: &BTreeMap<String, syn::ParenthesizedGenericArguments>,
    ) -> Result<(Wolf, NativeTy), String> {
        let Some(last) = path.segments.last() else {
            return Err("an empty path".to_string());
        };
        let name = last.ident.to_string();
        let single = path.segments.len() == 1;
        let ints = [
            "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "isize", "usize",
        ];
        match name.as_str() {
            n if single && ints.contains(&n) => Ok((Wolf::Int, NativeTy::Int(name))),
            "f32" | "f64" if single => Ok((Wolf::Float, NativeTy::Float(name))),
            "bool" if single => Ok((Wolf::Bool, NativeTy::Bool)),
            "String" => Ok((Wolf::String, NativeTy::String)),
            "Vec" => {
                let (w, n) = self.ty(one_arg(&last.arguments)?, Pos::Inner, callbacks)?;
                Ok((Wolf::List(Box::new(w)), NativeTy::Vec(Box::new(n))))
            }
            "HashMap" => self.map(&last.arguments, "HashMap", false, callbacks),
            "BTreeMap" if pos != Pos::Param => {
                self.map(&last.arguments, "BTreeMap", false, callbacks)
            }
            "BTreeMap" => {
                Err("a `BTreeMap` parameter: pass a `&HashMap` or a `HashMap`".to_string())
            }
            "Option" => {
                let inner = if pos == Pos::Ret {
                    Pos::RetInner
                } else {
                    Pos::Inner
                };
                let (w, n) = self.ty(one_arg(&last.arguments)?, inner, callbacks)?;
                Ok((Wolf::Option(Box::new(w)), NativeTy::Option(Box::new(n))))
            }
            "Result" if pos == Pos::Ret => {
                let ok = first_arg(&last.arguments)?;
                let (w, n) = self.ty(ok, Pos::RetInner, callbacks)?;
                Ok((Wolf::Result(Box::new(w)), NativeTy::Result(Box::new(n))))
            }
            "Result" => Err("a `Result` that is not the result".to_string()),
            _ if single && callbacks.contains_key(&name) && pos == Pos::Param => {
                match callbacks.get(&name) {
                    Some(args) => self.callback(args),
                    None => Err("an unknown type parameter".to_string()),
                }
            }
            // A record is copied: by value anywhere.
            _ if single && self.records.contains(&name) => Ok((
                Wolf::Named(name.clone()),
                NativeTy::Record {
                    name,
                    by_ref: false,
                },
            )),
            _ if single && self.opaque.contains(&name) => match pos {
                Pos::Ret | Pos::RetInner => Ok((
                    Wolf::Named(name.clone()),
                    NativeTy::Named {
                        name,
                        by_ref: false,
                    },
                )),
                Pos::Param => Err(format!(
                    "takes `{name}` by value, but values of extern types cannot be copied: take `&{name}`"
                )),
                Pos::Inner => Err(format!(
                    "`{name}` inside a list or a map, where values of extern types cannot be copied"
                )),
            },
            _ => Err(format!("the type `{name}`, which wolfpack does not map")),
        }
    }

    fn map(
        &self,
        args: &PathArguments,
        kind: &str,
        by_ref: bool,
        callbacks: &BTreeMap<String, syn::ParenthesizedGenericArguments>,
    ) -> Result<(Wolf, NativeTy), String> {
        let (k, v) = two_args(args)?;
        let (kw, kn) = self.ty(k, Pos::Inner, callbacks)?;
        if !matches!(kw, Wolf::Int | Wolf::String | Wolf::Bool) {
            return Err("a map whose keys are not integers, strings or `bool`".to_string());
        }
        let (vw, vn) = self.ty(v, Pos::Inner, callbacks)?;
        Ok((
            Wolf::Map(Box::new(kw), Box::new(vw)),
            NativeTy::Map {
                kind: kind.to_string(),
                by_ref,
                key: Box::new(kn),
                value: Box::new(vn),
            },
        ))
    }

    /// A callback `Fn(A, ..) -> R` in the forms the backends pass (spec chapter 8, section 8):
    /// parameters `i64`, `f64`, `bool`, `&str`, `&[T]` or `&` of an extern type; a result `i64`,
    /// `f64`, `bool`, `String`, `()` or `Vec<T>` of these.
    fn callback(
        &self,
        args: &syn::ParenthesizedGenericArguments,
    ) -> Result<(Wolf, NativeTy), String> {
        let simple = |t: &Type| -> Option<Wolf> {
            match t {
                Type::Path(tp) if tp.path.is_ident("i64") => Some(Wolf::Int),
                Type::Path(tp) if tp.path.is_ident("f64") => Some(Wolf::Float),
                Type::Path(tp) if tp.path.is_ident("bool") => Some(Wolf::Bool),
                _ => None,
            }
        };
        let mut params = Vec::new();
        for a in &args.inputs {
            let w = match a {
                Type::Reference(r) if r.mutability.is_none() => match r.elem.as_ref() {
                    Type::Path(tp) if tp.path.is_ident("str") => Some(Wolf::String),
                    Type::Slice(s) => simple(&s.elem).map(|w| Wolf::List(Box::new(w))),
                    Type::Path(tp) => tp
                        .path
                        .get_ident()
                        .map(ToString::to_string)
                        .filter(|n| self.opaque.contains(n))
                        .map(Wolf::Named),
                    _ => None,
                },
                other => simple(other),
            };
            params.push(w.ok_or("a callback parameter that wolfpack does not map: callbacks take `i64`, `f64`, `bool`, `&str`, `&[T]` or `&` of an extern type")?);
        }
        let ret = match &args.output {
            ReturnType::Default => Wolf::Void,
            ReturnType::Type(_, t) => match t.as_ref() {
                Type::Tuple(tu) if tu.elems.is_empty() => Wolf::Void,
                Type::Path(tp) if tp.path.is_ident("String") => Wolf::String,
                other => simple(other).ok_or(
                    "a callback result that wolfpack does not map: callbacks return `i64`, `f64`, `bool`, `String` or `()`",
                )?,
            },
        };
        Ok((Wolf::Fn(params, Box::new(ret)), NativeTy::Callback))
    }
}

/// The arguments of an `Fn(A, ..) -> R` bound among `bounds`, if there is one.
fn fn_bound<'a>(
    mut bounds: impl Iterator<Item = &'a TypeParamBound>,
) -> Option<syn::ParenthesizedGenericArguments> {
    bounds.find_map(|b| match b {
        TypeParamBound::Trait(t) => {
            let last = t.path.segments.last()?;
            match (&last.arguments, last.ident.to_string().as_str()) {
                (PathArguments::Parenthesized(args), "Fn") => Some((*args).clone()),
                _ => None,
            }
        }
        _ => None,
    })
}

fn type_args(args: &PathArguments) -> Vec<&Type> {
    match args {
        PathArguments::AngleBracketed(a) => a
            .args
            .iter()
            .filter_map(|g| match g {
                GenericArgument::Type(t) => Some(t),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn one_arg(args: &PathArguments) -> Result<&Type, String> {
    match type_args(args).as_slice() {
        [t] => Ok(t),
        _ => Err("a type with an unexpected number of type arguments".to_string()),
    }
}

fn first_arg(args: &PathArguments) -> Result<&Type, String> {
    type_args(args)
        .first()
        .copied()
        .ok_or_else(|| "a `Result` without type arguments".to_string())
}

fn two_args(args: &PathArguments) -> Result<(&Type, &Type), String> {
    match type_args(args).as_slice() {
        [k, v] => Ok((k, v)),
        _ => Err("a map with an unexpected number of type arguments".to_string()),
    }
}

/// The code of one target of a native module: the target (`rust`, `go`), the module path inside
/// `native/<target>/` (`text::slug` for Rust, `text/slug` for Go) and what was read.
pub struct Block<'a> {
    /// `rust` or `go`.
    pub target: &'a str,
    /// The path of the module or package inside `native/<target>/`.
    pub path: String,
    /// The declarations.
    pub module: &'a Module,
}

/// The wolfpack source of a native module: one `extern` block per target with its functions and
/// types. `module_path` is the wolfpack path (`native.text.slug`). `name_ok` says whether a
/// wolfpack parameter name is usable; other parameters are named `arg<i>`.
pub fn source(module_path: &str, blocks: &[Block<'_>], name_ok: &dyn Fn(&str) -> bool) -> String {
    let mut out = format!("module {module_path}\n");
    let blocks: Vec<&Block<'_>> = blocks
        .iter()
        .filter(|b| {
            !(b.module.fns.is_empty() && b.module.types.is_empty() && b.module.records.is_empty())
        })
        .collect();
    if blocks.is_empty() {
        return out;
    }
    // Each record is declared once, as the first target that has it reads it (the targets agree,
    // rule 10).
    let mut records: Vec<&Record> = Vec::new();
    for b in &blocks {
        for r in &b.module.records {
            if !records.iter().any(|x| x.name == r.name) {
                records.push(r);
            }
        }
    }
    out.push_str("\nextern {\n");
    for b in blocks {
        let path = |name: &str, pointer: bool| match b.target {
            "go" => format!("native:{}{}.{name}", if pointer { "*" } else { "" }, b.path),
            "ts" => format!("native:{}#{name}", b.path),
            _ => format!("native:{}::{name}", b.path),
        };
        out.push_str(&format!("    {} {{\n", b.target));
        for t in &b.module.types {
            let p = path(t, b.module.pointers.contains(t));
            out.push_str(&format!("        pub type {t} from \"{p}\"\n"));
        }
        for r in &b.module.records {
            let p = path(&r.name, false);
            out.push_str(&format!("        struct {} from \"{p}\"\n", r.name));
        }
        for f in &b.module.fns {
            let params: Vec<String> = f
                .params
                .iter()
                .enumerate()
                .map(|(i, (n, w))| {
                    let n = if name_ok(n) {
                        n.clone()
                    } else {
                        format!("arg{i}")
                    };
                    format!("{n}: {}", w.text())
                })
                .collect();
            let ret = match &f.ret {
                Wolf::Void => String::new(),
                other => format!(" -> {}", other.text()),
            };
            out.push_str(&format!(
                "        pub fn {}({}){ret} from \"{}\"\n",
                f.name,
                params.join(", "),
                path(&f.target_name, false)
            ));
        }
        out.push_str("    }\n");
    }
    out.push_str("}\n");
    if !records.is_empty() {
        out.push_str("\ntypes {\n");
        for (i, r) in records.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&format!("    pub struct {} {{\n", r.name));
            for f in &r.fields {
                out.push_str(&format!("        pub {}: {}\n", f.name, f.ty.text()));
            }
            out.push_str("    }\n");
        }
        out.push_str("}\n");
    }
    out
}

/// The wolfpack source of a native module read from Rust code only.
pub fn rust_source(
    module_path: &str,
    rust_path: &str,
    module: &Module,
    name_ok: &dyn Fn(&str) -> bool,
) -> String {
    source(
        module_path,
        &[Block {
            target: "rust",
            path: rust_path.to_string(),
            module,
        }],
        name_ok,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(src: &str) -> Module {
        read_rust(src).unwrap_or_default()
    }

    #[test]
    fn functions_and_types() {
        let m = read(
            r#"
use std::collections::HashMap;

pub struct Index { words: Vec<String> }
pub struct Point { pub x: f64, pub y: f64 }
pub struct Line { pub from: Point, pub to: Point, pub tags: Vec<String>, pub width: Option<u8> }
pub struct Holder { pub index: Index }
pub struct Later { pub held: Holder }

pub fn slugify(title: &str) -> String { todo!() }
pub fn count(words: &[String], limit: u32) -> usize { 0 }
pub fn open(path: String) -> Result<Index, std::io::Error> { todo!() }
pub fn size(index: &Index) -> Option<u64> { None }
pub fn tally(m: &HashMap<String, i64>) -> std::collections::BTreeMap<String, i32> { todo!() }
pub fn each<F: Fn(&str) -> bool>(items: Vec<String>, f: F) {}
pub fn each2(items: &Vec<String>, f: impl Fn(i64)) {}
pub fn bad(index: Index) {}
pub fn change(v: &mut Vec<i64>) {}
pub fn borrow(s: &str) -> &str { s }
pub fn origin() -> Point { todo!() }
pub async fn later() {}
pub fn length(l: &Line, scale: f32) -> f64 { 0.0 }
pub fn moved(p: Point) -> Vec<Point> { vec![] }
fn private() {}
#[cfg(test)]
pub fn hidden() {}
"#,
        );
        let names: Vec<&str> = m.fns.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "slugify", "count", "open", "size", "tally", "each", "each2", "origin", "length",
                "moved"
            ]
        );
        // A struct whose field is opaque is opaque, and so is one that holds it.
        assert_eq!(m.types, ["Index", "Holder", "Later"]);
        let records: Vec<&str> = m.records.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(records, ["Point", "Line"]);
        let unavailable: Vec<&str> = m.unavailable.keys().map(String::as_str).collect();
        assert_eq!(unavailable, ["bad", "borrow", "change", "later"]);
        let length = m.fns.iter().find(|f| f.name == "length");
        assert_eq!(
            length.and_then(|f| f.native.params.first()),
            Some(&NativeTy::Record {
                name: "Line".to_string(),
                by_ref: true
            })
        );
        let source = rust_source("native.slug", "slug", &m, &|_| true);
        assert!(
            source
                .contains("pub fn slugify(title: String) -> String from \"native:slug::slugify\""),
            "{source}"
        );
        assert!(
            source.contains("pub fn count(words: List<String>, limit: Int) -> Int"),
            "{source}"
        );
        assert!(
            source.contains("pub fn open(path: String) -> Result<Index, Error>"),
            "{source}"
        );
        assert!(
            source.contains("pub fn tally(m: Map<String, Int>) -> Map<String, Int>"),
            "{source}"
        );
        assert!(
            source.contains("pub fn each(items: List<String>, f: fn(String) -> Bool)"),
            "{source}"
        );
        assert!(
            source.contains("pub fn each2(items: List<String>, f: fn(Int) -> Void)"),
            "{source}"
        );
        assert!(
            source.contains("pub type Index from \"native:slug::Index\""),
            "{source}"
        );
        assert!(
            source.contains("struct Line from \"native:slug::Line\""),
            "{source}"
        );
        assert!(
            source.contains("    pub struct Line {\n        pub from: Point\n        pub to: Point\n        pub tags: List<String>\n        pub width: Option<Int>\n    }"),
            "{source}"
        );
        assert!(
            source.contains("pub fn moved(p: Point) -> List<Point>"),
            "{source}"
        );
    }

    #[test]
    fn go_functions_and_types() {
        let src = r#"
// Package slug makes slugs.
package slug

import (
	"errors"
	"strings"
)

type Index struct {
	words []string
}

type Point struct {
	X, Y float64
}

type Pair struct {
	ID  int32
	URL string
	At  Point
}

type Clash struct {
	URL string
	Url string
}

type ID int64

const limit = 3

var cache = map[string]int{}

// Slugify returns the slug.
func Slugify(title string) string {
	return strings.ToLower(title) + "}"
}

func CountWords(words []string, max int32) int {
	if max > 0 {
		return len(words)
	}
	return 0
}

func Open(path string) (*Index, error) { return nil, errors.New("x") }
func Find(name string) (ID, bool) { return 0, false }
func Each(items []string, f func(string) bool) int { return 0 }
func ParseURL(s string) error { return nil }
func Tally(m map[string]int64) map[string]uint8 { return nil }
func (i *Index) Add(w string) {}
func Bad(i Index) {}
func Many(a ...int) {}
func Generic[T any](x T) {}
func Ch(c chan int) {}
func Near(a Point, b []Pair) (Point, bool) { return a, true }
func Far(p *Point) {}
func private() {}
"#;
        let m = read_go(&[("slug.go".to_string(), src.to_string())]).unwrap_or_default();
        let names: Vec<&str> = m.fns.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "slugify",
                "count_words",
                "open",
                "find",
                "each",
                "parse_url",
                "tally",
                "near"
            ]
        );
        assert_eq!(m.types, ["Index", "ID"]);
        let records: Vec<&str> = m.records.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(records, ["Point", "Pair"]);
        let pair: Vec<(&str, &str)> = m
            .records
            .iter()
            .filter(|r| r.name == "Pair")
            .flat_map(|r| &r.fields)
            .map(|f| (f.name.as_str(), f.target_name.as_str()))
            .collect();
        assert_eq!(pair, [("id", "ID"), ("url", "URL"), ("at", "At")]);
        let unavailable: Vec<&str> = m.unavailable.keys().map(String::as_str).collect();
        assert_eq!(
            unavailable,
            ["Clash", "bad", "ch", "far", "generic", "many"]
        );
        let out = source(
            "native.slug",
            &[Block {
                target: "go",
                path: "slug".to_string(),
                module: &m,
            }],
            &|_| true,
        );
        assert!(
            out.contains("pub type Index from \"native:*slug.Index\""),
            "{out}"
        );
        assert!(out.contains("pub type ID from \"native:slug.ID\""), "{out}");
        assert!(out.contains("pub fn count_words(words: List<String>, max: Int) -> Int from \"native:slug.CountWords\""), "{out}");
        assert!(
            out.contains("pub fn open(path: String) -> Result<Index, Error>"),
            "{out}"
        );
        assert!(
            out.contains("pub fn find(name: String) -> Option<ID>"),
            "{out}"
        );
        assert!(
            out.contains("pub fn each(items: List<String>, f: fn(String) -> Bool) -> Int"),
            "{out}"
        );
        assert!(
            out.contains("pub fn parse_url(s: String) -> Result<Void, Error>"),
            "{out}"
        );
        assert!(
            out.contains("pub fn tally(m: Map<String, Int>) -> Map<String, Int>"),
            "{out}"
        );
        assert!(
            out.contains("struct Pair from \"native:slug.Pair\""),
            "{out}"
        );
        assert!(
            out.contains("pub fn near(a: Point, b: List<Pair>) -> Option<Point>"),
            "{out}"
        );
        assert!(
            out.contains("    pub struct Pair {\n        pub id: Int\n        pub url: String\n        pub at: Point\n    }"),
            "{out}"
        );
    }

    #[test]
    fn ts_functions_and_types() {
        let src = r#"
import { other } from "./other";

export interface Size { width: number; tags: string[]; maxSize: bigint }
export type Pair = { left: Size; right: Size };
export interface Shape { area(): number }
export class Index { words: string[] = [] }
export enum Color { Red }

export function slugify(title: string): string { return title; }
export function countWords(words: readonly string[], limit?: bigint): bigint { return 0n; }
export function parseURL(s: string): Map<string, number> | undefined { return undefined; }
export function scale(s: Size, by: number): Size { return s; }
export function each(items: Array<string>, f: (s: string) => boolean): void {}
export function lookup(i: Index, w: string): string | null { return null; }
export function counts(): Record<string, bigint> { return {}; }
export async function later(): Promise<void> {}
export function generic<T>(x: T): T { return x; }
export function untyped(x): string { return ""; }
export function noResult(x: string) { return x; }
export function over(x: string): string;
export function over(x: bigint): string;
export function over(x: any): string { return ""; }
export function bad(x: string | number): void {}
export function nul(x: string | null): void {}
function hidden(): void {}
"#;
        let m = read_ts(src).unwrap_or_default();
        let names: Vec<&str> = m.fns.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "slugify",
                "count_words",
                "parse_url",
                "scale",
                "each",
                "lookup",
                "counts"
            ]
        );
        let records: Vec<&str> = m.records.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(records, ["Size", "Pair"]);
        assert_eq!(m.types, ["Shape", "Index", "Color"]);
        let unavailable: Vec<&str> = m.unavailable.keys().map(String::as_str).collect();
        assert_eq!(
            unavailable,
            [
                "bad",
                "generic",
                "later",
                "no_result",
                "nul",
                "over",
                "untyped"
            ]
        );
        let out = source(
            "native.slug",
            &[Block {
                target: "ts",
                path: "text/slug".to_string(),
                module: &m,
            }],
            &|_| true,
        );
        assert!(
            out.contains("pub fn count_words(words: List<String>, limit: Option<Int>) -> Int from \"native:text/slug#countWords\""),
            "{out}"
        );
        assert!(
            out.contains("pub fn parse_url(s: String) -> Option<Map<String, Float>>"),
            "{out}"
        );
        assert!(
            out.contains("pub fn each(items: List<String>, f: fn(String) -> Bool)"),
            "{out}"
        );
        assert!(out.contains("pub fn counts() -> Map<String, Int>"), "{out}");
        assert!(
            out.contains("struct Size from \"native:text/slug#Size\""),
            "{out}"
        );
        assert!(
            out.contains("    pub struct Size {\n        pub width: Float\n        pub tags: List<String>\n        pub max_size: Int\n    }"),
            "{out}"
        );
        assert!(read_ts("export function f(: string {}").is_err());
    }

    #[test]
    fn go_snake_names() {
        assert_eq!(snake("Slugify"), "slugify");
        assert_eq!(snake("CountWords"), "count_words");
        assert_eq!(snake("ParseURL"), "parse_url");
        assert_eq!(snake("URLPath"), "url_path");
        assert_eq!(snake("Base64Encode"), "base64_encode");
    }

    #[test]
    fn syntax_errors_have_positions() {
        let e = read_rust("pub fn f( {").err();
        assert!(e.is_some_and(|e| e.line == 1 && e.column > 1));
    }
}
