# Chapter 3: Grammar

Status: **APPROVED** (author, 2026-10-07). Every **[PROPOSAL]** item in this chapter is accepted.

This chapter gives the syntax of declarations, types, statements, expressions and patterns, and the
operator precedence. Tokens are defined in chapter 1, the file skeleton in chapter 2. Meaning
(typing, ownership) is in chapters 4 and 5; this chapter only says what parses.

Decisions already taken by the author for this chapter (2026-10-07): closures use `|x|` as in Rust;
explicit type arguments in expressions are written `.<T>`; ranges are syntax only, in `for` and in
index brackets, with an inclusive form `..=`; a `Map` literal is `["a": 1]`.

## 1. Notation

EBNF with these conventions: `"x"` is a token, `a?` optional, `a*` zero or more, `a+` one or more,
`a | b` alternatives, `( )` grouping. `IDENT`, `INT`, `FLOAT`, `STRING` are literal tokens from
chapter 1. `NL` is the `Newline` token. `sep(x, s)` means `x ( s x )* s?`: a list with an optional
trailing separator.

**Separators** **[PROPOSAL]**: inside `( )`, `[ ]` and brace-delimited *literals* newlines are
ignored and `,` separates items. Inside brace-delimited *declarations* (struct fields, enum variants,
interface and impl members, section entries) items are separated by `NL`; a `,` is also accepted
there, and `wolf fmt` writes one item per line. Inside blocks (`{ }` of a body) statements are
separated by `NL`.

## 2. Declarations

```
visibility   = "pub"?
generics     = "<" sep(generic_param, ",") ">"
generic_param = IDENT ( ":" bounds )?
bounds       = type_path ( "+" type_path )*

fn_decl      = visibility "intrinsic"? "fn" IDENT generics? "(" params? ")" ( "->" type )? where_clause? block?
where_clause = "where" sep(IDENT ":" bounds, ",")    // methods of generic inherent impl blocks only, chapter 4, section 8.6
params       = sep(param, ",")
param        = self_param | mode? IDENT ":" type
self_param   = mode? "self"
mode         = "mut" | "own"

main_decl    = "main" ( "(" main_param? ")" )? ( "->" type )? block   // the `main` section, chapter 2
main_param   = IDENT ( ":" type )?

struct_decl  = visibility "struct" IDENT generics? "{" field* "}"
field        = visibility IDENT ":" type

enum_decl    = visibility "enum" IDENT generics? "{" variant+ "}"
variant      = IDENT ( "(" sep(type, ",") ")" | "{" sep(field_sig, ",") "}" )?
field_sig    = IDENT ":" type

alias_decl   = visibility "type" IDENT generics? "=" type
intrinsic_type = visibility "intrinsic" "type" IDENT generics?   // std only, chapter 7

interface_decl = visibility "interface" IDENT generics? "{" ( fn_sig block? )* "}"   // a block is a default, chapter 4, section 8.2
fn_sig       = "fn" IDENT generics? "(" params? ")" ( "->" type )?

impl_block   = IDENT generics? ( ":" type_path )? "{" ( "override"? fn_decl )* "}"   // `override`: chapter 4, section 8.3, rule 1

config_entry = visibility IDENT ":" type "=" expr
test_decl    = "test" STRING block
```

Rules:

1. A function body is required except for `intrinsic` functions (chapter 7) and `extern`
   declarations. **[PLAN]**
2. Omitting `-> type` means the result is `Void`. **[PROPOSAL]** The same holds for `main_decl`; its
   parameter and result type are checked in chapter 6, section 5 (author, 2026-10-17).
3. `self_param` may appear only as the first parameter of a method in `impl` or of a signature in an
   `interface`. A function in an inherent `impl` block without `self` is an **associated function**,
   called as `Type.name(...)` (for example `List.new()`, `FileStorage.open(path)`). **[PROPOSAL]**
4. **Enum variants** have no payload (`Full`), a positional payload (`Io(String)`) **[PLAN]**, or
   named fields (`Moved { x: Int, y: Int }`) **[PROPOSAL]** (see D3.1).
5. **Struct fields** may be `pub`. A field without `pub` is visible only inside its module.
   **[PROPOSAL]** (rules in chapter 6)
6. **Impl block head.** `Stack<T: Comparable> { ... }` declares the type parameters of the block
   and their bounds in the head. An `impl` for a specific instantiation (`Stack<Int> { }`) does not
   exist in v1. The interface after `:` may have arguments: `Stack<T>: Container<T> { }`.
   **[PROPOSAL]** (see D3.2) The bounds of the type's own declaration apply in the block without
   being repeated: with `struct Sorted<T: Comparable>`, the head `Sorted<T> { }` has
   `T: Comparable`. The head may add further bounds. (Author, 2026-10-09.)
7. Interfaces in v1 contain only signatures: no default method bodies, no super-interfaces.
   **[PROPOSAL]** (see D3.3). Whether a signature without `self` is allowed in an interface is
   decided in chapter 4 together with register item 13. Since D4.19 (author, 2026-10-23) a method
   of an interface may have a body, its default, and a method of an implementation block that
   replaces one starts with `override` (chapter 4, sections 8.2 and 8.3); there are still no
   super-interfaces.
8. Generic parameters are written in declarations as `<T: Bound + Other>`. There is no `where`
   clause in v1. **[PROPOSAL]** Since D4.16 (author, 2026-10-23) a method of a generic inherent
   `impl` block may end its signature with `where T: Bound`, which adds bounds to the parameters
   of the block for that method only (chapter 4, section 8.6); there is no other `where`.
9. The grammar of `imports` entries is in chapter 6, the grammar of `extern` declarations in
   chapter 7.

## 3. Types

```
type       = type_path
           | "fn" "(" sep(mode? type, ",")? ")" ( "->" type )?
           | "Self"
type_path  = IDENT ( "." IDENT )* type_args?
type_args  = "<" sep(type, ",") ">"
```

1. A type is a name with optional arguments: `Int`, `List<User>`, `Map<String, List<Int>>`,
   `model.User` (qualified by an imported module, chapter 6).
2. A function type is `fn(Int, String) -> Bool`. It is the type of closures and of named functions
   used as values. **[PROPOSAL]**
3. An interface name used as a type means "any value whose type implements this interface"
   (register item 12, gap G6). There is no separate syntax such as `dyn`. **[PROPOSAL]**
4. There are no tuple types, no array types with a fixed size, no reference types and no `?`
   shorthand for `Option`. **[PLAN]** (none of these appear in `PLAN.md`)
5. In type position `>>` closes two argument lists (chapter 1, section 6).

## 4. Statements

```
block      = "{" NL* ( stmt ( NL+ stmt )* NL* )? "}"
           // the last statement may be an expression: it is the value of the block
stmt       = let_stmt | assign_stmt | expr_stmt
           | ( label )? ( while_stmt | for_stmt | loop_expr )
let_stmt   = ( "let" | "var" ) IDENT ( ":" type )? "=" expr
           | "let" "_" "=" expr
           | ( "let" | "var" ) pattern "=" expr      // destructuring, rule 2
label      = IDENT ":"
assign_stmt = place assign_op expr
assign_op  = "=" | "+=" | "-=" | "*=" | "/=" | "%="
           | "&=" | "|=" | "^=" | "<<=" | ">>="
place      = IDENT ( "." IDENT | "[" expr "]" )*
           | "self" ( "." IDENT | "[" expr "]" )*
expr_stmt  = expr
while_stmt = "while" condition block                      // condition: section 5
for_stmt   = "for" IDENT "in" ( range | mode? expr_no_struct ) block
range      = expr ( ".." | "..=" ) expr
```

1. **Value of a block.** A block may end with an expression without anything after it; that
   expression is the value of the block **[PLAN]** (`None` and `Ok(())` at the end of functions in
   sections 3 and 5). A block without a final expression has the value `()`.
2. `let` and `var` always have an initializer. **[PROPOSAL]** `let _ = expr` evaluates `expr` and
   drops the value; it is the explicit way to discard a value such as a `Result` (chapter 6, D6.5).
   **Destructuring** (added in 0.2, author, 2026-10-18): the left side may be a pattern of section 7
   that always matches, such as `let Point { x, y } = p` or `var Config { name, port, .. } = load()`.
   A pattern that may fail (`let Some(x) = find(id)`) is an error (E0433, with the hint to use
   `if let` or `match`). In `let` a lone name is always a new binding, never a comparison with a
   constant or a variant (section 7, rule 3 does not apply). A type annotation is written only
   after a plain name. `var` makes every binding of the pattern mutable. Ownership of the parts is
   in chapter 5, section 7.4.
3. **Assignment is a statement, not an expression.** `a = b = c` does not parse. **[PROPOSAL]**
4. `return`, `break` and `continue` are **expressions** of type Never (chapter 4, D4.2), so they can
   stand in a `match` arm: `None => return 0`. `return` without a value returns `()`. A `return`
   inside a closure returns from the closure. The value after `return` extends to the end of the
   line, as for any expression.
5. **`loop`, labels and `break` with a value** (added in 0.2, author, 2026-10-18).
   - `loop { ... }` repeats its body until a `break`. It is an expression: `break value` ends it
     with `value`, as in Rust: `let line = loop { ... break text ... }`. A `loop` without a `break`
     of its own never ends (chapter 4, section 6, rule 9). `while true` keeps working.
   - `while`, `for` and `loop` may have a **label** at the start of the statement:
     `outer: for row in rows { ... }`. A label is a lower-case name followed by `:` on the line of
     the loop. `break outer` and `continue outer` leave or continue the labelled loop instead of
     the innermost one. A loop inside another loop cannot reuse an enclosing label (E0438). A
     label before anything that is not a loop is an error (E0315). A labelled `loop` is a
     statement, so its value is not used.
   - `break value` is allowed only in a `loop` and always ends the innermost loop (E0435 in a
     `while` or a `for`). A `break` with both a label and a value does not exist.
   - The parser reads `break name` as `break` with an expression. Name resolution decides: if an
     enclosing loop has the label `name`, it is a labelled `break`; otherwise `name` is the value.
     A name that is both an enclosing label and a variable in scope is an error (E0437). After
     `continue` a name is always a label (E0436 if no enclosing loop has it; E0436 also for a
     `break name` where `name` is neither a label nor a variable).
   - `while let pattern = expr { ... }` repeats while the value matches the pattern; the bindings
     are visible in the body: `while let Some(task) = queue.pop() { ... }`. **[DRAFT]** (D3.10):
     joined with other conditions by `&&`, section 5, rule 14a.
6. `for x in collection` iterates a collection **[PLAN]**; `for i in 0..n` and `for i in 1..=n`
   iterate integers (author). The loop variable is a plain name. **[PROPOSAL]** The collection may
   be marked `mut` or `own` (`for u in mut users`, `for u in own users`); the meaning is in
   chapter 5. A range cannot be marked.
7. A free-standing block `{ ... }` as a statement does not exist in v1. **[PROPOSAL]**

## 5. Expressions

```
expr       = closure | binary
closure    = "|" sep(closure_param, ",")? "|" ( "->" type block | expr )
           | "||" ( "->" type block | expr )
closure_param = IDENT ( ":" type )?

binary     = unary ( binop unary )*          // precedence in section 6
unary      = ( "-" | "!" | "~" )* postfix
postfix    = primary suffix*
suffix     = "." IDENT                       // field, method, module member, variant
           | "." "<" sep(type, ",") ">"      // explicit type arguments
           | call_args
           | "[" ( expr | slice ) "]"
           | "?"
call_args  = "(" sep(arg, ",")? ")"
arg        = mode? expr
slice      = expr? ( ".." | "..=" ) expr?

primary    = INT | FLOAT | STRING | "true" | "false"
           | FSTRING
           | IDENT | "self" | "Self"
           | "(" expr ")" | "(" ")"
           | list_lit | map_lit | struct_lit
           | if_expr | match_expr
           | "return" expr? | "break" expr? | "continue" IDENT?
           | loop_expr
loop_expr  = "loop" block
list_lit   = "[" sep(expr, ",")? "]"
map_lit    = "[" sep(expr ":" expr, ",") "]" | "[" ":" "]"
struct_lit = path ( "." "<" sep(type, ",") ">" )? "{" sep(field_init, ",")? ( ","? ".." expr )? "}"
path       = ( IDENT | "Self" ) ( "." IDENT )*
field_init = IDENT ( ":" expr )?

if_expr    = "if" condition block ( "else" ( if_expr | block ) )?
condition  = expr_no_struct
           | cond_item ( "&&" cond_item )*               // a `let` among the items: rules 14, 14a
cond_item  = "let" pattern "=" mode? operand | operand   // [DRAFT] chains with `&&`: rule 14a
operand    = expr_no_struct of level 9 or higher (section 6): no `&&`, `||` or closure outside
             parentheses
match_expr = "match" mode? expr_no_struct "{" NL* arm ( NL+ arm )* NL* "}"
arm        = pattern ( "if" expr )? "=>" ( expr | block )
```

1. **Calls and modes.** An argument may be marked `mut` or `own`: `sort(mut items)`,
   `save(own user)` **[PLAN]**. The receiver of a method call is **not** marked (author, D3.5): `items.push(4)`, `user.archive()`.
   Calling a `mut self` method requires the receiver to be a `var` (or a `mut` parameter); an
   `own self` call moves the receiver, and later use is reported by the ownership check (chapter 5).
2. **Explicit type arguments** are written with a dot: `parse.<Int>(text)`, `List.<Int>.new()`
   (author). Usually they are inferred and omitted.
3. **Paths.** Module members, variants and associated functions are reached with `.`:
   `fs.read_text(p)`, `StorageError.Full`, `StorageError.Io("disk")`, `List.new()` **[PLAN]**.
   `Some`, `None`, `Ok`, `Err` are used without qualification (chapter 4).
4. **Closures** (author): `|u| u.active`, `|x: Int| -> Int { x + 1 }`, `|| print("hi")`. As in
   Rust, a closure with a written result type must have a block body. The body of a closure without
   braces extends as far to the right as possible. Captures are copies or moves **[PLAN]**; how
   this is written is decided in chapter 5.
5. **`|` disambiguation.** `|` (and `||`) at the start of an operand begins a closure; between two
   operands it is the bitwise (logical) operator.
6. **Struct literals** are `User { id: 1, name: "Ann" }` **[PLAN]**. Type arguments in a literal
   use the expression form: `Stack.<Int> { items: [] }`, not `Stack<Int> { ... }`.
   Inside an `impl` block the path may start with `Self`, which names the type of the block:
   `Self { count: 0 }`, `Self.Leaf` (author, 2026-10-08). Outside `impl` blocks `Self` is an
   error reported by name resolution. A field whose value is a
   variable of the same name may be shortened: `User { id, name }` **[PROPOSAL]**.
   **Update syntax** (added in 0.2, author, 2026-10-18): `User { name: "B", ..old }`. The
   `..base` comes last, after the listed fields (E0316 otherwise, or when it appears twice). The
   base is an expression of the same struct type with the same type arguments (E0401); every field
   that is not listed takes its value from the base. The listed fields are evaluated first, left
   to right, then the base. The update syntax is for structs only, not for enum variants with
   named fields (E0434). Ownership of the base is in chapter 5, section 3.
7. **Struct literals in conditions.** In the condition of `if`, `while`, the scrutinee of `match`
   and the collection of `for` (`expr_no_struct`), a struct literal must be in parentheses:
   `if p == (Point { x: 0, y: 0 }) { }`. Otherwise `if x == Point {` would be ambiguous. This is
   the Rust rule. **[PROPOSAL]**
8. **List and Map literals** (author): `[1, 2, 3]`, `["a": 1, "b": 2]`, empty list `[]`, empty map
   `[:]`. The parser decides between list and map by the `:` after the first element.
9. **Slices** (author): `s[1..3]`, `items[2..]`, `items[..n]`, `s[0..=i]`. A slice produces a new
   value (references cannot be stored, PLAN section 4). Ranges appear nowhere else.
10. The scrutinee of `match` may be marked `mut` or `own` (`match own result { ... }`); the meaning
    is in chapter 5.
11. `if` and `match` are expressions **[PLAN]** and may be operands: `let x = if ok { 1 } else { 2 }`.
    An `if` without `else` has the value `()`.
12. `else` must be on the same line as the closing `}` of the previous branch (chapter 1, section 7).
13. `()` is the `Void` value **[PLAN]**.
14. **`if let`** (added in 0.2, author, 2026-10-18): `if let Some(user) = find(id) { ... } else
    { ... }` runs the first block with the bindings of the pattern when the value matches, and the
    `else` branch otherwise. The value may be marked `mut` or `own` as the scrutinee of `match`
    (chapter 5, section 7.4). `if let` is an expression with the typing of `if`. **[DRAFT]**
    (D3.10): `if let` and `while let` may be joined with other conditions, rule 14a.
14a. **[DRAFT]** (2026-10-25, D3.10) **Chains in conditions.** The condition of an `if` or a
    `while` may join `let` items and `Bool` expressions with `&&`:
    ```
    if let Some(user) = find(id) && user.active && let Some(mail) = user.mail {
        send(mail)
    }
    while let Some(line) = read_line() && line != "" {
        lines.push(line)
    }
    ```
    - The items are tried from left to right. The first one that fails, a pattern that does not
      match or a `false`, skips the items after it: an `if` runs its `else` branch, a `while`
      ends. When every item succeeds, the first block runs.
    - The bindings of a `let` item are visible in the items after it and in the first block, not
      in the `else` branch.
    - In a condition with a `let`, the value of a `let` item and every other item end at the
      next `&&` outside parentheses: `if let Some(x) = a && b` matches `a`, then tests `b`. A `||`
      outside parentheses in such a condition is an error (E0317), because it would be unclear
      what it joins; `if let Some(x) = a && (b || c)` is the way to write it. A closure as an item
      or as the value of a `let` needs parentheses too.
    - `let` is not an expression: it appears only as an item of a condition, never inside
      parentheses or after `!`, `||` or another operator (`if (let Some(x) = a)` is E0301).
    - Before this rule, `if let P = a && b` matched `P` against `a && b`, and `if let P = a || b`
      against `a || b`. The first is now the chain of `let P = a` and `b`, the second is E0317;
      matching a pattern against such a value needs parentheses: `if let true = (a && b)`. No
      program of the examples or the tests is affected.
    - There are no chains in the guards of `match` arms (`Some(x) if let ...`).
15. **`FSTRING`** is an interpolated string (chapter 1, section 5.3a); its interpolations are
    expressions of this section.

## 6. Operator precedence

From highest to lowest **[PROPOSAL]**:

| Level | Operators | Associativity |
| --- | --- | --- |
| 1 | postfix: `.field`, `.method(...)`, `(...)`, `[...]`, `?`, `.<T>` | left |
| 2 | prefix: `-`, `!`, `~` | right |
| 3 | `*`, `/`, `%` | left |
| 4 | `+`, `-` | left |
| 5 | `<<`, `>>` | left |
| 6 | `&` | left |
| 7 | `^` | left |
| 8 | `\|` | left |
| 9 | `==`, `!=`, `<`, `<=`, `>`, `>=` | **none**: `a < b < c` is an error |
| 10 | `&&` | left |
| 11 | `\|\|` | left |
| 12 | closure `\|x\| ...` | extends right |

Notes:

1. Bitwise operators bind tighter than comparisons, as in Rust and unlike C: `x & mask == 0` means
   `(x & mask) == 0`.
2. Postfix binds tighter than prefix: `-x.abs()` is `-(x.abs())`, as in Rust.
3. Ranges are not operators in this table: they exist only in the positions of section 4 rule 6 and
   section 5 rule 9.

## 7. Patterns

```
pattern     = alt ( "|" alt )*
alt         = "_"
            | IDENT                                   // binding
            | "-"? INT | FLOAT | STRING | "true" | "false"
            | path ( "(" sep(pattern, ",") ")"
                   | "{" sep(field_pat, ",") ( "," ".." )? "}" )?
field_pat   = IDENT ( ":" pattern )?
path        = ( IDENT | "Self" ) ( "." IDENT )*
```

1. Patterns appear in `match` arms, and since 0.2 in `let` / `var` (only patterns that always
   match), `if let` and `while let`.
2. Forms **[PROPOSAL]** (see D3.6): wildcard `_`; binding `name`; literal; variant without payload
   `StorageError.Full`, `None`; variant with positional payload `Some(x)`, `StorageError.Io(msg)`;
   variant or struct with named fields `Moved { x, y: 0 }`, `Point { x, .. }`; alternatives
   `A | B`; a guard `if cond` after the pattern.
3. A lone name is a binding unless it resolves to a variant without payload or to a `config`
   constant; in that case it is compared. Variants of user enums are written qualified
   (`StorageError.Full`), so in practice only `None` and constants are affected. **[PROPOSAL]**
4. In an alternative `A | B`, both sides must bind the same names (checked in chapter 4).
5. Float literals in patterns are not allowed (rejected after parsing). **[PROPOSAL]**
6. Inside an `impl` block a pattern path may start with `Self`: `Self.Node(left, v, right)`,
   `Self { x, .. }` (author, 2026-10-08).

## 8. Full example

```
module shapes

types {
    pub enum Shape {
        Circle(Float)
        Rect { w: Float, h: Float }
    }
}

functions {
    pub fn area(s: Shape) -> Float {
        match s {
            Shape.Circle(r) => 3.14159 * r * r
            Shape.Rect { w, h } => w * h
        }
    }

    pub fn total(shapes: List<Shape>) -> Float {
        var sum = 0.0
        for s in shapes {
            sum += area(s)
        }
        sum
    }

    pub fn big(own shapes: List<Shape>) -> List<Shape> {   // filter consumes the list (chapter 7)
        shapes.filter(|s| area(s) > 10.0)
    }
}
```

## 8a. Formatting (`wolf fmt`)

`wolf fmt` (milestone M12) rewrites source files in one canonical layout, the layout of the examples
(author, 2026-10-16). It changes only whitespace, line breaks, trailing commas and the separators of
declarations; the syntax tree and every comment stay the same. `wolf fmt --check` reports the files
that would change and changes nothing. A file with syntax errors is not formatted.

1. **Lines.** Indentation is 4 spaces per level. Lines are at most 100 columns wide where the rules
   below can break them. Lines end with `LF`, have no trailing spaces, and the file ends with one
   newline.
2. **Blank lines.** One blank line follows the `module` line and separates sections. Between two
   items of a section, two members of a declaration or two statements, a blank line is kept if the
   source has one or more, and none is added; blocks do not start or end with a blank line.
3. **Declarations.** One field, variant, interface method, impl method, extern declaration or
   section entry per line; a `,` separating them is removed. The payload of a struct variant stays
   on the line of the variant: `Done { note: String }`. The order of declarations is not changed.
4. **Spaces.** One space around binary operators, `=`, compound assignments, `->` and `=>`; after
   `,` and `:`; inside the braces of a struct literal or pattern (`Point { x: 1 }`); none inside
   `( )` and `[ ]`. Empty forms are `[]`, `[:]`, `()` and `Name {}`.
5. **One-line blocks.** A block written on one line stays on one line if it holds one statement
   and fits: `fn area(self) -> Float { self.w * self.h }`. Every other block has one statement per
   line.
6. **Breaking long lines.** When a construct does not fit, it is broken from the outside in:
   - arguments, list and map items, struct literal fields and parameters go one per line, indented
     once, with a trailing `,`;
   - a last argument that is a closure with a block body, a `match`, an `if` or a block stays on the
     line of the call: `items.map(|x| {`;
   - a chain of two or more method calls puts each `.call()` on its own line, indented once;
   - a binary expression breaks after its operators, the continuation indented once;
   - **[DRAFT]** (D3.10) a chain in a condition (section 5, rule 14a) breaks the same way, after
     each `&&` and so one item per line, the items after the first indented once;
   - the value of `let`, `var` or an assignment moves to the next line after `=`, indented once, if
     that makes it fit.
7. **Comments.** Doc comments and ordinary comments are kept as written. A comment on its own line
   stays before the statement, member or item that follows it; a comment after code on the same line
   stays at the end of that line, after one space.
8. **Strings** (0.2). A string literal is kept as written, including the expressions inside
   `f"..."`. When a `"""` string moves to another indentation level, every line of its content and
   the closing `"""` move by the same amount, so its value stays the same; trailing spaces inside
   it are kept. A label stays on the line of its loop: `outer: for x in xs {`.

## 9. Diagnostics

Proposed range E03xx for parser errors (confirmed in chapter 9). This list is the minimum; the
parser may add specific codes for frequent mistakes.

| Code | Error |
| --- | --- |
| E0301 | Unexpected token (expected X) |
| E0302 | Unclosed delimiter `(`, `[` or `{` (points at the opener) |
| E0303 | Binary operator at the start of a line (hint: move it to the end of the previous line) |
| E0304 | `else` on a new line (hint: put it after `}`) |
| E0305 | Chained comparison `a < b < c` |
| E0306 | Struct literal in a condition without parentheses |
| E0307 | Range outside `for` or index brackets |
| E0308 | Assignment used as an expression, or assignment to something that is not a place |
| E0309 | `self` parameter outside a method, or not first |
| E0310 | Closure with a result type but without a block body |
| E0311 | Generic arguments in an expression without the dot (`parse<Int>(s)`, hint: `parse.<Int>(s)`) |
| E0312 | Float literal in a pattern |
| E0313 | Two statements on one line (hint: there is no `;`, use a new line) |
| E0314 | Nesting too deep (more than 256 levels of expressions, blocks, types or patterns). Protects the compiler from stack overflow; added in M2 |
| E0315 | A label that is not followed by `while`, `for` or `loop` (added in 0.2) |
| E0316 | `..base` that is not the last item of a struct literal, or appears twice (added in 0.2) |
| E0317 | **[DRAFT]** `\|\|` outside parentheses in a condition with a `let` (section 5, rule 14a; D3.10) |

Recovery **[PLAN]** (section 9): on an error the parser skips to the next `NL` at the current nesting
level, or to the closing delimiter, and continues.

## 10. Decisions for the author

| # | Decision | Recommendation |
| --- | --- | --- |
| D3.1 | Enum variants may also have named fields: `Moved { x: Int, y: Int }` | **answered**: yes |
| D3.2 | Impl head declares type parameters: `Stack<T: Comparable> { }`; no impl for a concrete instantiation | **answered**: yes |
| D3.3 | Interfaces: no default method bodies and no super-interfaces in v1 (to `ideas.md`) | **answered**: yes, not in v1 |
| D3.4 | No destructuring `let`, no `if let`, no `while let` in v1 (to `ideas.md`) | **answered**: yes, not in v1 |
| D3.5 | Receiver of a method with `mut self` or `own self` is marked at the call site or not | **answered**: not marked |
| D3.6 | Pattern forms in section 7, including `\|` alternatives and `if` guards | **answered**: yes |
| D3.7 | Rest: separators (section 1), associated functions `Type.name()`, `pub` fields, function types `fn(A) -> B`, assignment as a statement, no `loop`/labels/free blocks, field shorthand in struct literals, struct literal restriction in conditions, precedence table, diagnostic list | **answered**: yes |
| D3.8 | 0.2: destructuring `let`, `if let`, `while let`, `loop` with `break value`, labels `outer: while`, update syntax `..base` (reverses D3.4 and part of D3.7) | **answered** (author, 2026-10-18): yes; labels without `'` |
| D3.9 | 0.2: the details of section 4 rules 2 and 5 and section 5 rules 6 and 14 (no `break label value`, the name after `break` resolved as a label first, no `if let` chains, `..base` for structs only), codes E0315, E0316 | **answered** (author, 2026-10-18): yes |
| D3.10 | **[DRAFT]** Chains in conditions (section 5, rule 14a; chapter 4, section 6; chapter 5, section 7.4): `let` items and `Bool` items joined by `&&` in `if` and `while`, tried left to right with short-circuit, bindings visible to the later items and the first block; `\|\|` outside parentheses in a condition with a `let` is E0317; `if let P = a && b` becomes a chain and `if let P = a \|\| b` an error (no program is affected). Reverses "no `if let` chains" of D3.9. **[INTERNAL]** expected: the IR lowers a chain into the `match`, `loop` and `break` it already has, so the targets and the VM need no new construct | **proposed** (2026-10-25), waiting for the author |
