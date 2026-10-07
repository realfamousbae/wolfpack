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

fn_decl      = visibility "intrinsic"? "fn" IDENT generics? "(" params? ")" ( "->" type )? block?
params       = sep(param, ",")
param        = self_param | mode? IDENT ":" type
self_param   = mode? "self"
mode         = "mut" | "own"

struct_decl  = visibility "struct" IDENT generics? "{" field* "}"
field        = visibility IDENT ":" type

enum_decl    = visibility "enum" IDENT generics? "{" variant+ "}"
variant      = IDENT ( "(" sep(type, ",") ")" | "{" sep(field_sig, ",") "}" )?
field_sig    = IDENT ":" type

alias_decl   = visibility "type" IDENT generics? "=" type

interface_decl = visibility "interface" IDENT generics? "{" fn_sig* "}"
fn_sig       = "fn" IDENT generics? "(" params? ")" ( "->" type )?

impl_block   = IDENT generics? ( ":" type_path )? "{" fn_decl* "}"

config_entry = visibility IDENT ":" type "=" expr
test_decl    = "test" STRING block
```

Rules:

1. A function body is required except for `intrinsic` functions (chapter 7) and `extern`
   declarations. **[PLAN]**
2. Omitting `-> type` means the result is `Void`. **[PROPOSAL]**
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
   **[PROPOSAL]** (see D3.2)
7. Interfaces in v1 contain only signatures: no default method bodies, no super-interfaces.
   **[PROPOSAL]** (see D3.3). Whether a signature without `self` is allowed in an interface is
   decided in chapter 4 together with register item 13.
8. Generic parameters are written in declarations as `<T: Bound + Other>`. There is no `where`
   clause in v1. **[PROPOSAL]**
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
block      = "{" ( stmt NL )* expr? "}"
stmt       = let_stmt | assign_stmt | expr_stmt
           | while_stmt | for_stmt
let_stmt   = ( "let" | "var" ) IDENT ( ":" type )? "=" expr
assign_stmt = place assign_op expr
assign_op  = "=" | "+=" | "-=" | "*=" | "/=" | "%="
           | "&=" | "|=" | "^=" | "<<=" | ">>="
place      = IDENT ( "." IDENT | "[" expr "]" )*
           | "self" ( "." IDENT | "[" expr "]" )*
expr_stmt  = expr
while_stmt = "while" expr_no_struct block
for_stmt   = "for" IDENT "in" ( range | expr_no_struct ) block
range      = expr ( ".." | "..=" ) expr
```

1. **Value of a block.** A block may end with an expression without anything after it; that
   expression is the value of the block **[PLAN]** (`None` and `Ok(())` at the end of functions in
   sections 3 and 5). A block without a final expression has the value `()`.
2. `let` and `var` always have an initializer. **[PROPOSAL]** The binding is a plain name; there is
   no destructuring in `let` and no `if let` in v1 (use `match`). **[PROPOSAL]** (see D3.4)
3. **Assignment is a statement, not an expression.** `a = b = c` does not parse. **[PROPOSAL]**
4. `return`, `break` and `continue` are **expressions** of type Never (chapter 4, D4.2), so they can
   stand in a `match` arm: `None => return 0`. `return` without a value returns `()`. A `return`
   inside a closure returns from the closure. The value after `return` extends to the end of the
   line, as for any expression.
5. `break` and `continue` have no labels and no values. There is no `loop`; write `while true`.
   **[PROPOSAL]**
6. `for x in collection` iterates a collection **[PLAN]**; `for i in 0..n` and `for i in 1..=n`
   iterate integers (author). The loop variable is a plain name. **[PROPOSAL]**
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
           | IDENT | "self" | "Self"
           | "(" expr ")" | "(" ")"
           | list_lit | map_lit | struct_lit
           | if_expr | match_expr
           | "return" expr? | "break" | "continue"
list_lit   = "[" sep(expr, ",")? "]"
map_lit    = "[" sep(expr ":" expr, ",") "]" | "[" ":" "]"
struct_lit = path ( "." "<" sep(type, ",") ">" )? "{" sep(field_init, ",")? "}"
path       = IDENT ( "." IDENT )*
field_init = IDENT ( ":" expr )?

if_expr    = "if" expr_no_struct block ( "else" ( if_expr | block ) )?
match_expr = "match" expr_no_struct "{" ( arm NL )+ "}"
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
   use the expression form: `Stack.<Int> { items: [] }`, not `Stack<Int> { ... }`. A field whose value is a
   variable of the same name may be shortened: `User { id, name }` **[PROPOSAL]**. There is no
   update syntax (`..other`).
7. **Struct literals in conditions.** In the condition of `if`, `while`, the scrutinee of `match`
   and the collection of `for` (`expr_no_struct`), a struct literal must be in parentheses:
   `if p == (Point { x: 0, y: 0 }) { }`. Otherwise `if x == Point {` would be ambiguous. This is
   the Rust rule. **[PROPOSAL]**
8. **List and Map literals** (author): `[1, 2, 3]`, `["a": 1, "b": 2]`, empty list `[]`, empty map
   `[:]`. The parser decides between list and map by the `:` after the first element.
9. **Slices** (author): `s[1..3]`, `items[2..]`, `items[..n]`, `s[0..=i]`. A slice produces a new
   value (references cannot be stored, PLAN section 4). Ranges appear nowhere else.
10. `if` and `match` are expressions **[PLAN]** and may be operands: `let x = if ok { 1 } else { 2 }`.
    An `if` without `else` has the value `()`.
11. `else` must be on the same line as the closing `}` of the previous branch (chapter 1, section 7).
12. `()` is the `Void` value **[PLAN]**.

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
path        = IDENT ( "." IDENT )*
```

1. Patterns appear only in `match` arms in v1. **[PROPOSAL]**
2. Forms **[PROPOSAL]** (see D3.6): wildcard `_`; binding `name`; literal; variant without payload
   `StorageError.Full`, `None`; variant with positional payload `Some(x)`, `StorageError.Io(msg)`;
   variant or struct with named fields `Moved { x, y: 0 }`, `Point { x, .. }`; alternatives
   `A | B`; a guard `if cond` after the pattern.
3. A lone name is a binding unless it resolves to a variant without payload or to a `config`
   constant; in that case it is compared. Variants of user enums are written qualified
   (`StorageError.Full`), so in practice only `None` and constants are affected. **[PROPOSAL]**
4. In an alternative `A | B`, both sides must bind the same names (checked in chapter 4).
5. Float literals in patterns are not allowed (rejected after parsing). **[PROPOSAL]**

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

    pub fn big(shapes: List<Shape>) -> List<Shape> {
        shapes.filter(|s| area(s) > 10.0)
    }
}
```

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
