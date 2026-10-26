# Chapter 5: Ownership

Status: **APPROVED** (author, 2026-10-07). Every **[PROPOSAL]** item in this chapter is accepted.

This chapter defines ownership in wolfpack: which values are copied and which are moved, the three
parameter modes, exclusive access, and closures. The model is "ownership without lifetimes"
**[PLAN]** (section 4): references exist only for the duration of a call, a loop or a `match` arm,
and can never be stored or returned, so no lifetime annotations are needed. This is the approach
known as *mutable value semantics* (Hylo, Swift).

Decisions already taken by the author for this chapter (2026-10-07):

- Closures capture by **move**; `Int`, `Float`, `Bool` are copied. Captured values cannot be
  changed inside the closure.
- `match place` binds by **read** access; `match own place` (or a `match` on a temporary) moves the
  value and the bindings own their parts.
- **No partial moves**: a field or a list element cannot be moved out.
- `for x in xs` reads, `for x in mut xs` changes elements in place, `for x in own xs` consumes the
  list.

## 1. Terms

- A **place** is something that names storage: a local variable, a parameter, `self`, and paths from
  them through fields and indices: `user`, `user.address.city`, `items[i]`, `self.count`.
- A **temporary** is any other expression: a literal, a call result, `x.copy()`, an arithmetic
  result.
- The **root** of a place is its first segment (`user` in `user.address.city`).
- An **access** to a place is a read, a mutation or a move.
- A **borrow** is temporary access to a place without taking ownership. Wolfpack has two kinds:
  **read** and **mut**. A borrow lasts only for a call, a `for` loop or a `match` arm.

## 2. Copy types and move types

1. Values of the **copy types** are copied implicitly on every use: `Int`, `Float`, `Bool`
   **[PLAN]**, and also `Void`, `Ordering` and every enum whose variants have no payload
   (author, D5.1; ADR 0004). A generic enum is never a copy type.
   **[DRAFT]** (2026-10-26, chapter 4, D4.23): `Char` is a copy type too.
   **[DRAFT]** (2026-10-26, chapter 4, D4.24): so are `Int8`, `Int16`, `Int32`, `UInt8`, `UInt16`
   and `UInt32`.
2. All other types are **move types**: `String`, `List`, `Map`, `Option`, `Result`, structs, enums,
   interface values, function values. They are copied only explicitly with `x.copy()` **[PLAN]**,
   which requires `Copyable` (chapter 4, section 9) and returns a new owned value.
3. `Option<Int>` and similar are move types even if their contents are copy types. **[PROPOSAL]**
   A **type parameter** `T` is a move type inside the generic code, whatever it is instantiated with:
   `self.items.push(own item)` needs the marker even if `T` turns out to be `Int`.
   (A simpler rule; `.copy()` is cheap for them.)

## 3. Ownership and moves

1. Every value has exactly one owner: a variable, a parameter in `own` mode, a field of an owned
   value, an element of an owned collection, or a temporary **[PLAN]**.
2. A value of a move type is **moved** when a place holding it is used in an *owning position*
   **[PLAN]**:
   - the initializer of `let` / `var`: `let b = a`;
   - the right side of an assignment: `x = a`;
   - an argument to an `own` parameter: `save(own user)`;
   - `return a`, or `a` as the final expression of a function body;
   - a field value in a struct literal, an element in a list or map literal, a variant payload:
     `Wrapper { inner: a }`, `[a]`, `Some(a)`;
   - a capture by a closure (section 8);
   - the scrutinee of `match own a`, the collection of `for x in own a`;
   - the value of a destructuring `let` and of `if let` / `while let` marked `own` (section 7.4);
   - the base of a struct update `User { name: "B", ..a }` (0.2): `a` is moved as a whole,
     and the fields that are not listed are taken from it. The fields of `a` that the literal
     replaces are dropped. To keep `a`, write `..a.copy()`.
3. **Only whole variables can be moved** (author). The place in an owning position must be a local
   variable or an `own` parameter, never a field path or an index. `let n = user.name` is an error
   with three suggestions: copy it (`user.name.copy()`), take the whole value apart with
   `match own user`, or use a std function that takes a part out (the method `items.remove(i)`,
   `replace(mut place, own value)` from the prelude, rule 7).
4. A **borrowed** value cannot be moved: a read or `mut` parameter, `self` in read or `mut` mode, a
   binding of `match place`, a variable of `for x in xs`. The error suggests `.copy()`.
5. For copy types, an owning position simply copies; none of these restrictions apply.
6. **Operands of operators are only read.** `"a" + s`, `a == b`, `-x` read their operands and
   produce a new value; they never move. (Author, 2026-10-07.)
7. To take a whole value out of a place that cannot be moved from (a field, an element, a `mut`
   parameter), use the prelude function `replace(mut place, own value) -> T`, which puts `value`
   in and returns the old value: `let old = replace(mut self.tasks, [])`.

## 4. Use after move

1. After a move, the variable is **moved**. Any later use of it (read, borrow, move) is an error
   that points to the place of the move **[PLAN]**.
2. A moved `var` becomes usable again after it is assigned a whole new value. A moved `let` stays
   moved.
3. The check is **flow-sensitive**: if a variable is moved on some path (in one branch of an `if`,
   one arm of a `match`), it counts as moved after the join. The error says "may have been moved"
   and points at the branch.
4. A move inside a loop of a variable declared outside the loop is an error, because the next
   iteration would use a moved value, unless every path through the loop body assigns the variable
   again before the next iteration, or leaves the loop with `break` or `return`.
5. When an owner goes out of scope its value is dropped. Wolfpack has no destructors, so dropping
   has no observable effect. **[PROPOSAL]**

## 5. Parameter modes

A parameter has one of three modes **[PLAN]**. The mode of `self` works the same way.

| Mode | Declaration | Call site | Callee may | Argument must be |
| --- | --- | --- | --- | --- |
| read | `u: User` | `show(user)` | read | any expression |
| mut | `mut u: User` | `rename(mut user)` | read, change, replace as a whole | a mutable place |
| own | `own u: User` | `save(own user)` or `save(make())` | anything: it is the owner | a temporary, or a whole variable (moved) |

1. **Read.** The callee can read the value and pass it on in read mode. It cannot change it, move it
   or store it; to keep it, it must `.copy()` it. The caller still owns the value.
2. **Mut.** The callee has exclusive access for the duration of the call. It can change fields,
   call `mut self` methods, assign a whole new value (`u = User { ... }`), and pass it on in read
   or `mut` mode. It cannot move it out (the caller still owns it). The argument must be a
   **mutable place**: rooted in a `var`, a `mut` parameter, an `own` parameter or `mut self`. A
   temporary is an error.
3. **Own.** The callee becomes the owner. Inside the callee an `own` parameter behaves like a `var`:
   it can be changed and moved. **[PROPOSAL]** (see D5.4)
4. **Markers at the call site** **[PLAN]** (rule 5): `mut` is written exactly when the parameter is
   `mut`. `own` is written exactly when the parameter is `own` and the argument is a place of a move
   type. A missing or extra marker is an error with a fix hint. **[PROPOSAL]** (see D5.3)
5. **Receivers** are not marked (author, D3.5). A `mut self` method needs a mutable place as the
   receiver. An `own self` method moves the receiver, which therefore must be a whole variable or a
   temporary.
6. **Results** of functions are always owned by the caller. A reference can never be a result
   **[PLAN]** (rule 6), so `fn name(self) -> String { self.name }` is an error ("cannot move out of
   borrowed `self`"); write `self.name.copy()`.
7. A reference can never be a struct field **[PLAN]** (rule 6). This needs no rule of its own: there
   is no reference type in the language.

## 6. Mutability of bindings

1. `let` bindings are immutable, `var` bindings are mutable **[PLAN]**. Mutability is deep: the
   fields and elements of a `let` value cannot be changed either.
2. Read parameters and read `self` are immutable. `mut` and `own` parameters and `mut self`/`own self`
   are mutable.
3. Bindings introduced by `match place` and `for x in xs` are immutable. Bindings of
   `for x in mut xs` and `match mut place` are mutable (they give access to the part in place). Bindings of
   `match own` and `for x in own xs` are owned but immutable, like `let`; write `var y = x` to
   change them. **[PROPOSAL]**
4. Every form of change requires a mutable place: assignment, compound assignment, `mut` argument,
   `mut self` method call, and `for x in mut`.

## 7. Exclusive access

### 7.1 Overlapping places

Two places **overlap** if one is a prefix of the other: `user` and `user.name` overlap,
`user.name` and `user.age` do not. Two index paths into the same list (`items[i]` and `items[j]`)
**always** overlap, because the indices are not known at compile time. **[PROPOSAL]** (see D5.5)

### 7.2 Within one call

1. A place passed `mut` (or the receiver of a `mut self` call) must not overlap any other place used
   in the same call, as a receiver or an argument, in any mode **[PLAN]** (rule 7):
   `transfer(mut a, mut a)`, `merge(mut items, items)` and `items.append(items)` are errors.
2. A place moved by an argument (`own x`) must not appear anywhere else in the same call:
   `pair(own x, x)` is an error.
3. Temporaries do not take part. In `items.push(items.len())` the argument is computed before the
   call into an `Int`, so it does not conflict with the `mut self` receiver.
4. Disjoint fields are fine: `swap_values(mut p.x, mut p.y)`.

### 7.3 Loops

1. `for x in xs` borrows `xs` for reading during the whole loop. The body may read `xs`, but may not
   change or move anything that overlaps `xs`.
2. `for x in mut xs` borrows `xs` exclusively. Inside the body, `xs` (and anything overlapping it)
   cannot be used at all, except through `x`.
3. `for x in own xs` moves `xs` before the first iteration; each `x` owns one element.
4. Ranges (`for i in 0..n`) borrow nothing; `n` is evaluated once before the loop. **[PROPOSAL]**
5. **A temporary collection** (`for w in text.split(" ")`, `for x in [1, 2]`) lives for the whole
   loop. `for x in temp` gives read bindings, `for x in own temp` gives owned elements, and
   `for x in mut temp` is an error (E0506), like a temporary passed to a `mut` parameter.
   (Author, 2026-10-07.)

### 7.4 `match`

1. `match place { ... }` borrows the place for reading in every arm. Inside an arm the place and
   anything overlapping it can be read but not changed or moved. **[PROPOSAL]**
2. `match own place` moves the place before the arms; the bindings own the parts.
3. `match temporary` (for example `match find(id)`) behaves like `match own`: the temporary has no
   other owner.
4. `match mut place` (author, D5.2) borrows the place exclusively. The bindings give in-place,
   mutable access to the parts: `match mut shape { Shape.Circle(r) => { r *= 2.0 } }` (an arm
   that assigns is a block, since an assignment is a statement, chapter 3). Inside the arm the
   place itself cannot be used except through the bindings. The place must be a mutable place.
5. **`if let` and `while let`** (0.2) follow the rules of `match` with one arm: `if let P = place`
   borrows the place for reading in the first block, `if let P = mut place` borrows it
   exclusively, and `if let P = own place` or a temporary moves the value and the bindings own the
   parts. In `while let` the value is evaluated again before every iteration, so
   `while let Some(x) = stack.pop()` moves each element out of the temporary result.
   (author, 2026-10-25, D3.10) **Chains** (chapter 3, section 5, rule 14a). Each `let` item of
   a chain follows these rules from its own place to the end of the first block: the borrow it
   takes lasts through the items after it and the first block, so a later item that changes or
   moves an overlapping place, or borrows it in another mode, is an error (E0509), as inside the
   arm of a `match`: `if let Some(a) = mut x && let Some(b) = x` is E0509. A place that a `let`
   item moves (`own place`) is moved in the `else` branch and after the `if`, like a move in one
   branch of an `if`: when the item is the first one it is moved (E0501 for a later use), and
   when it is a later one it may have been moved (E0502), since a later item runs only when the
   earlier ones succeed.
6. **A destructuring `let`** (0.2) is an owning position: `let Point { x, y } = p` moves `p` (if
   `Point` is a move type) and the bindings own the parts, as in `match own p`. The value must
   therefore be a whole variable or a temporary (section 3, rule 3).

### 7.5 Assignment

`a = b`: the right side is evaluated first, then the old value of `a` is dropped and replaced.
`a[i] = a[j]` is fine for copy types; for move types it is a move out of an element and therefore an
error (section 3, rule 3).

## 8. Closures

1. A closure **captures** every variable from the enclosing scope that its body uses. It captures
   the **whole variable**, even if the body uses only a field. **[PROPOSAL]** (see D5.6)
2. Copy types are captured by copy. Move types are captured by **move** (author): after the closure
   is created, the variable is moved (section 4). To keep using it, copy it first:

   ```
   let n = name.copy()
   let named = users.filter(|u| u.name == n)
   print(name)          // ok: the closure owns `n`, not `name`
   ```

3. A borrowed value (read or `mut` parameter, `self`, `for` and `match` bindings) cannot be
   captured; the error suggests copying it into a local first.
4. Captured values are **immutable** inside the closure (author). Assigning to a captured variable,
   or passing it `mut`, is an error. This keeps closures free of shared mutable state, which matters
   for Go and TS, where a closure value can be referenced from several places.
5. A closure is a value of a function type, which is a move type and not `Copyable` (chapter 4).
   It is called with read access; calling it does not consume it.
6. A closure may change its own parameters only if they are declared `mut` or `own` in its
   function type, like any function. **[PROPOSAL]**

## 9. How this maps to the targets

**[PLAN]** (section 4), detailed in chapter 8:

| Mode | Rust | Go | TypeScript |
| --- | --- | --- | --- |
| read | `&T` | value, or `*T` for large structs | the object reference |
| mut | `&mut T` | `*T` | the object reference |
| own | `T` | value | the object reference |
| `.copy()` | `.clone()` | generated copy function | generated copy function |

Why Go and TS need no defensive copies: the rules above guarantee that while a value can be changed,
nothing else can observe it, and that after a move the old name is never used. Sharing a reference
in Go or TS is therefore invisible to the program.

## 10. Diagnostics

Proposed range E05xx (confirmed in chapter 9). Every error points to the place of the conflicting
earlier action (the move, the borrow) as a secondary label.

| Code | Error |
| --- | --- |
| E0501 | Use of a moved value |
| E0502 | Use of a value that may have been moved (in a branch or a previous iteration) |
| E0503 | Move out of a field or a list element (partial move) |
| E0504 | Move out of a borrowed value |
| E0505 | Change of an immutable binding |
| E0506 | `mut` argument or `mut self` receiver is not a mutable place |
| E0507 | Missing or unexpected `mut` / `own` marker at the call site |
| E0508 | Overlapping places in one call while one of them is `mut` or moved |
| E0509 | Change or move of a place that a `for` loop or a `match` arm is borrowing |
| E0510 | Capture of a borrowed value by a closure |
| E0511 | Change of a captured value inside a closure |

Each rule of this chapter gets a pair of tests: correct code and code with the error
**[PLAN]** (milestone M8).

## 11. Decisions for the author

| # | Decision | Recommendation |
| --- | --- | --- |
| D5.1 | Extend implicit copying beyond `Int`, `Float`, `Bool` to `Void`, `Ordering` and enums without payloads | **answered**: yes (ADR 0004) |
| D5.2 | Add `match mut place` | **answered**: yes |
| D5.3 | Call-site markers are required exactly where they are meaningful; an extra marker is an error | **answered**: yes |
| D5.4 | An `own` parameter is mutable inside the callee | **answered**: yes |
| D5.5 | Overlap: disjoint fields are independent, any two indices into one list overlap | **answered**: yes |
| D5.6 | Closures capture whole variables, not individual fields | **answered**: yes |
| D5.7 | Rest: moves only of whole variables, flow-sensitive move check and loop rule, mutable place rule, `match` borrows the place in each arm, diagnostics | **answered**: yes |
| D5.8 | 0.2: `if let` / `while let` as one-arm `match`, destructuring `let` and the base of `..base` are owning positions (sections 3 and 7.4) | **answered** (author, 2026-10-18): yes |
| D5.9 | Chains in conditions (section 7.4, rule 5; chapter 3, D3.10): every `let` item keeps its borrow through the later items and the first block (E0509); a move by the first item is a move in `else` and after the `if` (E0501), a move by a later item a possible move (E0502) | **answered** (author, 2026-10-25): yes |
