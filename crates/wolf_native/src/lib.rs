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
    /// The public items wolfpack cannot use, with the reason.
    pub unavailable: BTreeMap<String, String>,
}

/// A function of a native module.
#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    /// Its name.
    pub name: String,
    /// The parameters: name and wolfpack type.
    pub params: Vec<(String, Wolf)>,
    /// The wolfpack result type.
    pub ret: Wolf,
    /// The Rust types that the generated call must match.
    pub native: NativeSig,
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
    /// An opaque type of the module.
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
    // Types first: functions refer to them in any order.
    let mut opaque = BTreeSet::new();
    for item in &file.items {
        let (vis, name, attrs, record) = match item {
            Item::Struct(s) => {
                let record = matches!(&s.fields, syn::Fields::Named(f) if f.named.iter().all(|f| matches!(f.vis, Visibility::Public(_))))
                    && s.generics.params.is_empty();
                (&s.vis, s.ident.to_string(), &s.attrs, record)
            }
            Item::Enum(e) => (&e.vis, e.ident.to_string(), &e.attrs, false),
            Item::Type(t) => (&t.vis, t.ident.to_string(), &t.attrs, false),
            _ => continue,
        };
        if !matches!(vis, Visibility::Public(_)) || has_cfg(attrs) {
            continue;
        }
        if record {
            // Records of native modules come with the record conversions (D7.15); until then
            // such a struct is not mapped at all, so that its meaning does not change later.
            module.unavailable.insert(
                name,
                "a struct with only `pub` fields becomes a record of wolfpack, which a later version supports".to_string(),
            );
        } else {
            opaque.insert(name.clone());
            module.types.push(name);
        }
    }
    let reader = Reader { opaque: &opaque };
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
}

impl Reader<'_> {
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

/// The wolfpack source of a native module: an `extern` block of its functions and types for the
/// target `rust`. `module_path` is the wolfpack path (`native.text.slug`), `rust_path` the module
/// path inside `native/rust/` (`text::slug`). `name_ok` says whether a wolfpack parameter name is
/// usable; other parameters are named `arg<i>`.
pub fn rust_source(
    module_path: &str,
    rust_path: &str,
    module: &Module,
    name_ok: &dyn Fn(&str) -> bool,
) -> String {
    let mut out = format!("module {module_path}\n");
    if module.fns.is_empty() && module.types.is_empty() {
        return out;
    }
    out.push_str("\nextern {\n    rust {\n");
    for t in &module.types {
        out.push_str(&format!(
            "        pub type {t} from \"native:{rust_path}::{t}\"\n"
        ));
    }
    for f in &module.fns {
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
            "        pub fn {}({}){ret} from \"native:{rust_path}::{}\"\n",
            f.name,
            params.join(", "),
            f.name
        ));
    }
    out.push_str("    }\n}\n");
    out
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
fn private() {}
#[cfg(test)]
pub fn hidden() {}
"#,
        );
        let names: Vec<&str> = m.fns.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            ["slugify", "count", "open", "size", "tally", "each", "each2"]
        );
        assert_eq!(m.types, ["Index"]);
        let unavailable: Vec<&str> = m.unavailable.keys().map(String::as_str).collect();
        assert_eq!(
            unavailable,
            ["Point", "bad", "borrow", "change", "later", "origin"]
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
    }

    #[test]
    fn syntax_errors_have_positions() {
        let e = read_rust("pub fn f( {").err();
        assert!(e.is_some_and(|e| e.line == 1 && e.column > 1));
    }
}
