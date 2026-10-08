//! A span-free text form of the tree, for snapshot tests and `wolf check --emit ast`.
//!
//! Each node becomes `(head child child)`. A node that does not fit on one line is printed with
//! its head on a line and its children indented below it.

use crate::*;

/// Renders a file as an indented tree without spans.
pub fn dump(file: &File) -> String {
    let mut out = String::new();
    render(&file_tree(file), 0, &mut out);
    out
}

const WIDTH: usize = 100;

struct Tree {
    head: String,
    kids: Vec<Tree>,
}

fn leaf(head: impl Into<String>) -> Tree {
    Tree {
        head: head.into(),
        kids: Vec::new(),
    }
}

fn node(head: impl Into<String>, kids: Vec<Tree>) -> Tree {
    Tree {
        head: head.into(),
        kids,
    }
}

/// A list-like node; when it is empty it prints as `empty` so it cannot be confused with a name.
fn group(head: &str, empty: &str, kids: Vec<Tree>) -> Tree {
    if kids.is_empty() {
        leaf(empty)
    } else {
        node(head, kids)
    }
}

fn flat(tree: &Tree, out: &mut String) {
    if tree.kids.is_empty() {
        out.push_str(&tree.head);
        return;
    }
    out.push('(');
    out.push_str(&tree.head);
    for kid in &tree.kids {
        out.push(' ');
        flat(kid, out);
    }
    out.push(')');
}

fn render(tree: &Tree, indent: usize, out: &mut String) {
    let mut line = String::new();
    flat(tree, &mut line);
    out.push_str(&" ".repeat(indent));
    if tree.kids.is_empty() || indent + line.len() <= WIDTH {
        out.push_str(&line);
        out.push('\n');
        return;
    }
    out.push_str(&tree.head);
    out.push('\n');
    for kid in &tree.kids {
        render(kid, indent + 2, out);
    }
}

fn path(p: &Path) -> String {
    p.segments
        .iter()
        .map(|s| s.name.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

fn docs(list: &[Doc], kids: &mut Vec<Tree>) {
    for doc in list {
        let kind = if doc.inner { "doc!" } else { "doc" };
        kids.push(leaf(format!("{kind} {:?}", doc.text)));
    }
}

fn flag(on: bool, name: &str, kids: &mut Vec<Tree>) {
    if on {
        kids.push(leaf(name));
    }
}

fn file_tree(file: &File) -> Tree {
    let mut kids = Vec::new();
    docs(&file.docs, &mut kids);
    match &file.module {
        Some(p) => kids.push(leaf(format!("module {}", path(p)))),
        None => kids.push(leaf("module <missing>")),
    }
    for section in &file.sections {
        let items = section.items.iter().map(item).collect();
        kids.push(node(format!("section {}", section.kind.name()), items));
    }
    node("file", kids)
}

fn generics(list: &[GenericParam], kids: &mut Vec<Tree>) {
    if list.is_empty() {
        return;
    }
    let params = list
        .iter()
        .map(|g| {
            if g.bounds.is_empty() {
                leaf(g.name.name.clone())
            } else {
                node(g.name.name.clone(), g.bounds.iter().map(ty).collect())
            }
        })
        .collect();
    kids.push(node("generics", params));
}

fn item(item: &Item) -> Tree {
    let mut kids = Vec::new();
    docs(&item.docs, &mut kids);
    let head = match &item.kind {
        ItemKind::Import(import) => {
            flag(import.public, "pub", &mut kids);
            kids.push(leaf(path(&import.path)));
            if let Some(alias) = &import.alias {
                kids.push(leaf(format!("as {}", alias.name)));
            }
            if let Some(names) = &import.names {
                let group = names
                    .iter()
                    .map(|n| match &n.alias {
                        Some(alias) => leaf(format!("{} as {}", n.name.name, alias.name)),
                        None => leaf(n.name.name.clone()),
                    })
                    .collect();
                kids.push(node("names", group));
            }
            "import".to_string()
        }
        ItemKind::ExternTarget(target) => {
            for decl in &target.decls {
                let mut d = Vec::new();
                docs(&decl.docs, &mut d);
                flag(decl.public, "pub", &mut d);
                let head = match &decl.kind {
                    ExternDeclKind::Fn(s) => {
                        sig(s, &mut d);
                        "fn"
                    }
                    ExternDeclKind::Type(name) => {
                        d.push(leaf(name.name.clone()));
                        "type"
                    }
                };
                d.push(leaf(format!("from {:?}", decl.from.value)));
                kids.push(node(head, d));
            }
            format!("extern {}", target.target.name)
        }
        ItemKind::Config(entry) => {
            flag(entry.public, "pub", &mut kids);
            kids.push(leaf(entry.name.name.clone()));
            kids.push(ty(&entry.ty));
            kids.push(expr(&entry.value));
            "const".to_string()
        }
        ItemKind::Struct(decl) => {
            flag(decl.public, "pub", &mut kids);
            kids.push(leaf(decl.name.name.clone()));
            generics(&decl.generics, &mut kids);
            kids.extend(decl.fields.iter().map(field));
            "struct".to_string()
        }
        ItemKind::Enum(decl) => {
            flag(decl.public, "pub", &mut kids);
            kids.push(leaf(decl.name.name.clone()));
            generics(&decl.generics, &mut kids);
            for v in &decl.variants {
                let mut vk = Vec::new();
                docs(&v.docs, &mut vk);
                vk.push(leaf(v.name.name.clone()));
                match &v.payload {
                    VariantPayload::None => {}
                    VariantPayload::Tuple(types) => {
                        vk.push(node("tuple", types.iter().map(ty).collect()))
                    }
                    VariantPayload::Struct(fields) => {
                        vk.push(node("fields", fields.iter().map(field).collect()))
                    }
                }
                kids.push(node("variant", vk));
            }
            "enum".to_string()
        }
        ItemKind::Alias(decl) => {
            flag(decl.public, "pub", &mut kids);
            kids.push(leaf(decl.name.name.clone()));
            generics(&decl.generics, &mut kids);
            kids.push(ty(&decl.ty));
            "alias".to_string()
        }
        ItemKind::IntrinsicType(decl) => {
            flag(decl.public, "pub", &mut kids);
            kids.push(leaf(decl.name.name.clone()));
            generics(&decl.generics, &mut kids);
            "intrinsic type".to_string()
        }
        ItemKind::Interface(decl) => {
            flag(decl.public, "pub", &mut kids);
            kids.push(leaf(decl.name.name.clone()));
            generics(&decl.generics, &mut kids);
            for m in &decl.methods {
                let mut mk = Vec::new();
                docs(&m.docs, &mut mk);
                sig(&m.sig, &mut mk);
                kids.push(node("sig", mk));
            }
            "interface".to_string()
        }
        ItemKind::Impl(block) => {
            kids.push(leaf(block.type_name.name.clone()));
            generics(&block.generics, &mut kids);
            if let Some(interface) = &block.interface {
                kids.push(node("for", vec![ty(interface)]));
            }
            for m in &block.methods {
                let mut mk = Vec::new();
                docs(&m.docs, &mut mk);
                fn_decl(&m.decl, &mut mk);
                kids.push(node("fn", mk));
            }
            "impl".to_string()
        }
        ItemKind::Fn(decl) => {
            fn_decl(decl, &mut kids);
            "fn".to_string()
        }
        ItemKind::Test(test) => {
            kids.push(leaf(format!("{:?}", test.name.value)));
            kids.push(block(&test.body));
            "test".to_string()
        }
    };
    node(head, kids)
}

fn field(f: &Field) -> Tree {
    let mut kids = Vec::new();
    docs(&f.docs, &mut kids);
    flag(f.public, "pub", &mut kids);
    kids.push(leaf(f.name.name.clone()));
    kids.push(ty(&f.ty));
    node("field", kids)
}

fn fn_decl(decl: &FnDecl, kids: &mut Vec<Tree>) {
    flag(decl.public, "pub", kids);
    flag(decl.intrinsic, "intrinsic", kids);
    sig(&decl.sig, kids);
    if let Some(body) = &decl.body {
        kids.push(block(body));
    }
}

fn mode(m: Mode) -> &'static str {
    match m {
        Mode::Read => "",
        Mode::Mut => "mut ",
        Mode::Own => "own ",
    }
}

fn sig(s: &FnSig, kids: &mut Vec<Tree>) {
    kids.push(leaf(s.name.name.clone()));
    generics(&s.generics, kids);
    let params = s
        .params
        .iter()
        .map(|p| match &p.kind {
            ParamKind::SelfParam => leaf(format!("{}self", mode(p.mode))),
            ParamKind::Named { name, ty: t } => {
                node(format!("{}{}", mode(p.mode), name.name), vec![ty(t)])
            }
        })
        .collect();
    kids.push(group("params", "()", params));
    if let Some(ret) = &s.ret {
        kids.push(node("->", vec![ty(ret)]));
    }
}

fn ty(t: &Ty) -> Tree {
    match &t.kind {
        TyKind::Path { path: p, args } => {
            if args.is_empty() {
                leaf(path(p))
            } else {
                node(path(p), args.iter().map(ty).collect())
            }
        }
        TyKind::Fn { params, ret } => {
            let mut kids: Vec<Tree> = params
                .iter()
                .map(|(m, t)| {
                    if *m == Mode::Read {
                        ty(t)
                    } else {
                        node(mode(*m).trim(), vec![ty(t)])
                    }
                })
                .collect();
            if let Some(ret) = ret {
                kids.push(node("->", vec![ty(ret)]));
            }
            node("fn", kids)
        }
        TyKind::SelfType => leaf("Self"),
        TyKind::Error => leaf("<error>"),
    }
}

fn block(b: &Block) -> Tree {
    group("block", "{}", b.stmts.iter().map(stmt).collect())
}

fn stmt(s: &Stmt) -> Tree {
    match &s.kind {
        StmtKind::Let {
            mutable,
            name,
            ty: t,
            value,
        } => {
            let mut kids = vec![leaf(name.as_ref().map_or("_", |n| n.name.as_str()))];
            if let Some(t) = t {
                kids.push(ty(t));
            }
            kids.push(expr(value));
            node(if *mutable { "var" } else { "let" }, kids)
        }
        StmtKind::Assign { target, op, value } => {
            node(assign_symbol(*op), vec![expr(target), expr(value)])
        }
        StmtKind::While { cond, body } => node("while", vec![expr(cond), block(body)]),
        StmtKind::For { var, iter, body } => {
            let iter = match iter {
                ForIter::Collection { mode: m, expr: e } => {
                    if *m == Mode::Read {
                        expr(e)
                    } else {
                        node(mode(*m).trim(), vec![expr(e)])
                    }
                }
                ForIter::Range {
                    start,
                    end,
                    inclusive,
                } => node(
                    if *inclusive { "..=" } else { ".." },
                    vec![expr(start), expr(end)],
                ),
            };
            node("for", vec![leaf(var.name.clone()), iter, block(body)])
        }
        StmtKind::Expr(e) => expr(e),
    }
}

fn assign_symbol(op: AssignOp) -> &'static str {
    match op {
        AssignOp::Assign => "=",
        AssignOp::Add => "+=",
        AssignOp::Sub => "-=",
        AssignOp::Mul => "*=",
        AssignOp::Div => "/=",
        AssignOp::Rem => "%=",
        AssignOp::BitAnd => "&=",
        AssignOp::BitOr => "|=",
        AssignOp::BitXor => "^=",
        AssignOp::Shl => "<<=",
        AssignOp::Shr => ">>=",
    }
}

fn expr(e: &Expr) -> Tree {
    match &e.kind {
        ExprKind::Int(v) => leaf(v.to_string()),
        ExprKind::Float(v) => leaf(format!("{v:?}")),
        ExprKind::Str(v) => leaf(format!("{v:?}")),
        ExprKind::Bool(v) => leaf(v.to_string()),
        ExprKind::Unit => leaf("()"),
        ExprKind::Name(n) => leaf(n.name.clone()),
        ExprKind::SelfValue => leaf("self"),
        ExprKind::SelfType => leaf("Self"),
        ExprKind::Paren(inner) => node("paren", vec![expr(inner)]),
        ExprKind::List(items) => group("list", "[]", items.iter().map(expr).collect()),
        ExprKind::Map(entries) => group(
            "map",
            "[:]",
            entries
                .iter()
                .map(|(k, v)| node(":", vec![expr(k), expr(v)]))
                .collect(),
        ),
        ExprKind::StructLit {
            path: p,
            type_args,
            fields,
        } => {
            let mut kids = vec![leaf(path(p))];
            if !type_args.is_empty() {
                kids.push(node(".<>", type_args.iter().map(ty).collect()));
            }
            for f in fields {
                kids.push(match &f.value {
                    Some(v) => node(format!("{}:", f.name.name), vec![expr(v)]),
                    None => leaf(f.name.name.clone()),
                });
            }
            node("struct", kids)
        }
        ExprKind::Field { base, name } => node(".", vec![expr(base), leaf(name.name.clone())]),
        ExprKind::TypeArgs { base, args } => {
            let mut kids = vec![expr(base)];
            kids.extend(args.iter().map(ty));
            node(".<>", kids)
        }
        ExprKind::Call { callee, args } => {
            let mut kids = vec![expr(callee)];
            for a in args {
                kids.push(if a.mode == Mode::Read {
                    expr(&a.value)
                } else {
                    node(mode(a.mode).trim(), vec![expr(&a.value)])
                });
            }
            node("call", kids)
        }
        ExprKind::Index { base, index } => node("index", vec![expr(base), expr(index)]),
        ExprKind::Slice {
            base,
            start,
            end,
            inclusive,
        } => {
            let bound = |b: &Option<Box<Expr>>| b.as_ref().map_or(leaf("_"), |e| expr(e));
            node(
                if *inclusive { "slice..=" } else { "slice.." },
                vec![expr(base), bound(start), bound(end)],
            )
        }
        ExprKind::Try(inner) => node("?", vec![expr(inner)]),
        ExprKind::Unary { op, operand } => {
            let symbol = match op {
                UnaryOp::Neg => "neg",
                UnaryOp::Not => "!",
                UnaryOp::BitNot => "~",
            };
            node(symbol, vec![expr(operand)])
        }
        ExprKind::Binary { op, lhs, rhs } => node(op.symbol(), vec![expr(lhs), expr(rhs)]),
        ExprKind::Closure { params, ret, body } => {
            let params = params
                .iter()
                .map(|p| match &p.ty {
                    Some(t) => node(p.name.name.clone(), vec![ty(t)]),
                    None => leaf(p.name.name.clone()),
                })
                .collect();
            let mut kids = vec![group("params", "||", params)];
            if let Some(ret) = ret {
                kids.push(node("->", vec![ty(ret)]));
            }
            kids.push(expr(body));
            node("closure", kids)
        }
        ExprKind::Block(b) => block(b),
        ExprKind::If {
            cond,
            then,
            otherwise,
        } => {
            let mut kids = vec![expr(cond), block(then)];
            if let Some(other) = otherwise {
                kids.push(node("else", vec![expr(other)]));
            }
            node("if", kids)
        }
        ExprKind::Match {
            mode: m,
            scrutinee,
            arms,
        } => {
            let mut kids = vec![if *m == Mode::Read {
                expr(scrutinee)
            } else {
                node(mode(*m).trim(), vec![expr(scrutinee)])
            }];
            for arm in arms {
                let mut ak = vec![pat(&arm.pattern)];
                if let Some(guard) = &arm.guard {
                    ak.push(node("if", vec![expr(guard)]));
                }
                ak.push(expr(&arm.body));
                kids.push(node("=>", ak));
            }
            node("match", kids)
        }
        ExprKind::Return(value) => match value {
            Some(v) => node("return", vec![expr(v)]),
            None => leaf("return"),
        },
        ExprKind::Break => leaf("break"),
        ExprKind::Continue => leaf("continue"),
        ExprKind::Error => leaf("<error>"),
    }
}

fn pat(p: &Pat) -> Tree {
    match &p.kind {
        PatKind::Wild => leaf("_"),
        PatKind::Name(n) => leaf(n.name.clone()),
        PatKind::Int { negative, value } => {
            leaf(format!("{}{}", if *negative { "-" } else { "" }, value))
        }
        PatKind::Str(s) => leaf(format!("{s:?}")),
        PatKind::Bool(b) => leaf(b.to_string()),
        PatKind::Path(p) => leaf(path(p)),
        PatKind::Tuple { path: p, items } => node(path(p), items.iter().map(pat).collect()),
        PatKind::Struct {
            path: p,
            fields,
            rest,
        } => {
            let mut kids: Vec<Tree> = fields
                .iter()
                .map(|f| match &f.pat {
                    Some(sub) => node(format!("{}:", f.name.name), vec![pat(sub)]),
                    None => leaf(f.name.name.clone()),
                })
                .collect();
            if *rest {
                kids.push(leaf(".."));
            }
            node(format!("{} {{}}", path(p)), kids)
        }
        PatKind::Or(alts) => node("|", alts.iter().map(pat).collect()),
        PatKind::Error => leaf("<error>"),
    }
}
