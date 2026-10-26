//! The functions and closures that the IR marks as able to call themselves (spec chapter 8,
//! section 4.3): Go and Rust count only their calls, so everything else must stay unmarked.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wolf_ir::{Block, ExprKind, Function, Item, ModuleRole, Stmt};

/// The marked functions of the program, as `name` or `Type.name`, and the number of marked
/// closures.
fn marked(source: &str) -> (Vec<String>, usize) {
    let mut single = wolf_driver::single::check(source);
    let program = single.program_for_vm().unwrap_or_else(|| {
        panic!(
            "{}",
            wolf_diag::render_human_to_string(&single.map, &single.diagnostics)
        )
    });
    let mut names = Vec::new();
    let mut closures = 0;
    for m in program
        .modules
        .iter()
        .filter(|m| m.role == ModuleRole::Entry)
    {
        for item in &m.items {
            let (owner, fns): (Option<&str>, Vec<&Function>) = match item {
                Item::Fn(f) => (None, vec![f]),
                Item::Struct(s) => (Some(&s.name), s.methods.iter().collect()),
                Item::Enum(e) => (Some(&e.name), e.methods.iter().collect()),
                Item::Interface(i) => (Some(&i.name), i.methods.iter().collect()),
                _ => (None, Vec::new()),
            };
            for f in fns {
                if f.recursive {
                    names.push(match owner {
                        Some(o) => format!("{o}.{}", f.name),
                        None => f.name.clone(),
                    });
                }
                if let Some(b) = &f.body {
                    closures += marked_closures(b);
                }
            }
        }
    }
    names.sort();
    (names, closures)
}

fn marked_closures(b: &Block) -> usize {
    let mut n = 0;
    for s in &b.stmts {
        match s {
            Stmt::Let { value, .. } | Stmt::Expr(value) => n += in_expr(value),
            _ => {}
        }
    }
    n + b.tail.as_deref().map_or(0, in_expr)
}

/// The marked closures of an expression, enough of the IR for the programs below.
fn in_expr(e: &wolf_ir::Expr) -> usize {
    match &e.kind {
        ExprKind::Closure {
            body, recursive, ..
        } => usize::from(*recursive) + marked_closures(body),
        ExprKind::Call { args, .. } => args.iter().map(|a| in_expr(&a.value)).sum(),
        ExprKind::Block(b) => marked_closures(b),
        _ => 0,
    }
}

#[test]
fn direct_and_mutual_recursion() {
    let (names, _) = marked(
        "module main\n\nmain {\n    print(f\"{fact(5)} {even(4)} {plain(1)}\")\n}\n\n\
         functions {\n    fn fact(n: Int) -> Int {\n        if n == 0 {\n            return 1\n        }\n        n * fact(n - 1)\n    }\n\n\
         \x20   fn even(n: Int) -> Bool {\n        if n == 0 {\n            return true\n        }\n        odd(n - 1)\n    }\n\n\
         \x20   fn odd(n: Int) -> Bool {\n        if n == 0 {\n            return false\n        }\n        even(n - 1)\n    }\n\n\
         \x20   fn plain(n: Int) -> Int {\n        fact(n) + 1\n    }\n}\n",
    );
    assert_eq!(names, ["even", "fact", "odd"]);
}

#[test]
fn calls_through_values_and_interfaces() {
    let (names, closures) = marked(
        "module main\n\ntypes {\n    struct Down {\n        step: Int\n    }\n}\n\n\
         interfaces {\n    interface Counter {\n        fn count(self, n: Int) -> Int\n    }\n}\n\n\
         impl {\n    Down: Counter {\n        fn count(self, n: Int) -> Int {\n            if n <= 0 {\n                return 0\n            }\n            let next: Counter = Down { step: self.step }\n            next.count(n - self.step)\n        }\n    }\n}\n\n\
         main {\n    print(f\"{apply(again, 3)} {deep(3)}\")\n}\n\n\
         functions {\n    fn apply(f: fn(Int) -> Int, n: Int) -> Int {\n        f(n)\n    }\n\n\
         \x20   fn again(n: Int) -> Int {\n        if n == 0 {\n            return 0\n        }\n        apply(again, n - 1)\n    }\n\n\
         \x20   fn deep(n: Int) -> Int {\n        if n == 0 {\n            return 0\n        }\n        let parts = [n - 1].map(|k| deep(k))\n        parts[0]\n    }\n}\n",
    );
    assert_eq!(names, ["Down.count", "again", "apply", "deep"]);
    assert_eq!(closures, 1);
}

#[test]
fn code_without_recursion_stays_unmarked() {
    // A generic wrapper calls the method of its `T`, a default calls `Self`, std calls closures
    // that call other functions, and a closure in a local is called by name: none of these can
    // repeat without end.
    let (names, closures) = marked(
        "module main\n\ntypes {\n    struct Holder<T> {\n        inner: T\n    }\n\n    struct Leaf {\n        label: String\n    }\n}\n\n\
         interfaces {\n    interface Named {\n        fn name(self) -> String\n\n        fn shout(self) -> String {\n            self.name() + \"!\"\n        }\n    }\n}\n\n\
         impl {\n    Leaf: Named {\n        fn name(self) -> String {\n            self.label.copy()\n        }\n    }\n\n\
         \x20   Holder<T: Named>: Named {\n        fn name(self) -> String {\n            \"[\" + self.inner.shout() + \"]\"\n        }\n    }\n}\n\n\
         main {\n    let h = Holder { inner: Holder { inner: Leaf { label: \"x\" } } }\n    print(h.shout())\n    print(twice_all([1, 2]).len().to_string())\n    let add = |y: Int| y + 1\n    let outer = |x: Int| add(x)\n    print(outer(1).to_string())\n}\n\n\
         functions {\n    fn double(n: Int) -> Int {\n        n * 2\n    }\n\n\
         \x20   fn twice_all(items: List<Int>) -> List<Int> {\n        items.map(|n| double(n))\n    }\n}\n",
    );
    assert!(names.is_empty(), "{names:?}");
    assert_eq!(closures, 0);
}

/// The wrapper of `code_without_recursion_stays_unmarked`, used with `main` as given.
fn wrapper(main: &str) -> String {
    format!(
        "module main\n\ntypes {{\n    struct Holder<T> {{\n        inner: T\n    }}\n\n    struct Leaf {{\n        label: String\n    }}\n}}\n\n\
         interfaces {{\n    interface Named {{\n        fn name(self) -> String\n\n        fn shout(self) -> String {{\n            self.name() + \"!\"\n        }}\n    }}\n}}\n\n\
         impl {{\n    Leaf: Named {{\n        fn name(self) -> String {{\n            self.label.copy()\n        }}\n    }}\n\n\
         \x20   Holder<T: Named>: Named {{\n        fn name(self) -> String {{\n            \"[\" + self.inner.shout() + \"]\"\n        }}\n    }}\n}}\n\n\
         {main}"
    )
}

#[test]
fn wrappers_of_interface_values_are_marked() {
    // `Holder<Named>` may hold another `Holder`, as deep as the data.
    let (names, _) = marked(&wrapper(
        "main {\n    let leaf: Named = Leaf { label: \"x\" }\n    let h: Holder<Named> = Holder { inner: leaf }\n    print(h.shout())\n}\n",
    ));
    assert_eq!(names, ["Holder.name", "Named.shout"]);
    // So may `Holder<U>` of generic code, which may be called with an interface value.
    let (names, _) = marked(&wrapper(
        "main {\n    print(wrap(Leaf { label: \"x\" }).shout())\n}\n\n\
         functions {\n    fn wrap<U: Named>(own x: U) -> Holder<U> {\n        Holder { inner: x }\n    }\n}\n",
    ));
    assert_eq!(names, ["Holder.name", "Named.shout"]);
}
