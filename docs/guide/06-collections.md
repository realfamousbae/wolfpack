# 6. Lists and maps

Wolfpack has two collections: `List<T>`, an ordered list that can grow, and `Map<K, V>`, which maps
keys to values. Both behave the same way on every target, including the order in which a map
returns its keys.

## Lists

```wolf
module main

main {
    var items = [3, 1, 4]
    items.push(1)
    items.insert(0, 9)
    let removed = items.remove(1)
    print(items.len().to_string() + " " + removed.to_string())
    print(items[0].to_string() + " " + items[items.len() - 1].to_string())
    items[0] = 7
    let middle = items[1..3]
    print(middle[0].to_string() + " " + middle[1].to_string())
    match items.pop() {
        Some(last) => print("popped " + last.to_string())
        None => print("empty")
    }
}
```

```output
4 3
9 1
1 4
popped 1
```

- `[a, b, c]` builds a list; all elements have one type. `[]` is an empty list, whose element type
  comes from the context: `var names: List<String> = []`, or a later `names.push("Ann")`.
- `items[i]` reads an element and `items[i] = v` replaces it. An index outside the list stops the
  program with a panic: `panic: index 5 out of range for length 3`.
- `items[a..b]` is a new list with copies of the elements from `a` up to `b`.
- `push`, `pop`, `insert`, `remove`, `swap`, `reverse`, `clear` and `append` change the list, so
  the list must be a `var`. `pop` returns an `Option` (chapter 7): `None` when the list is empty.

### Working with every element

`for x in items` reads each element. `for x in mut items` lets the body change each element in
place, and `for x in own items` takes the elements out and consumes the list:

```wolf
module main

main {
    var prices = [100, 250, 40]
    for p in mut prices {
        p = p * 2
    }
    let labels = prices.map(|p| "$" + p.to_string())
    let cheap = prices.filter(|p| p < 300)
    print(labels[1] + " " + cheap.len().to_string())
    let words = ["red", "green", "blue"]
    print(words.any(|w| w == "blue").to_string())
    print(words.all(|w| w.len() > 3).to_string())
    print(words.count(|w| w.contains("e")).to_string())
}
```

```output
$500 2
true
false
3
```

`map` builds a new list from the results of a function, `any`, `all` and `count` test each
element. `filter` keeps the elements for which the function returns `true`; it **consumes** the
list, so after `prices.filter(...)` the variable `prices` is moved. Pass a copy to keep it:
`prices.copy().filter(...)`.

### The `std.list` module

Some operations need more from the element type than a list can promise for every `T`: sorting
needs elements that can be compared, searching needs elements that can be checked for equality.
They are free functions of the module `std.list`, which is imported:

```wolf
module main

imports {
    std.list
}

main {
    var scores = [42, 7, 19, 7]
    list.sort(mut scores)
    print(scores[0].to_string() + " " + scores[3].to_string())
    print(list.contains(scores, 19).to_string())
    print(list.sum(scores).to_string())
    match list.max(scores) {
        Some(best) => print("best " + best.to_string())
        None => print("no scores")
    }
    var names = ["Cid", "ann", "Bob"]
    list.sort_by(mut names, |a, b| a.to_lower_ascii().compare(b.to_lower_ascii()))
    print(names[0] + " " + names[1] + " " + names[2])
    print(list.range(0, 4).len().to_string())
}
```

```output
7 42
true
75
best 42
ann Bob Cid
4
```

| Function | |
| --- | --- |
| `list.contains(items, x)`, `list.index_of(items, x)` | search for an element |
| `list.get(items, i)` | a copy of the element, or `None` outside the list |
| `list.sort(mut items)`, `list.sort_by(mut items, compare)` | stable sorts |
| `list.min(items)`, `list.max(items)` | `None` for an empty list |
| `list.sum(items)` | sum of a `List<Int>` |
| `list.range(from, to)` | the integers from `from` up to `to`, as a list |

`list.sort` needs elements that are `Comparable`: `Int`, `String`, or your own types (chapter 9).
`Float` is not `Comparable`, because NaN has no place in an order; sort floats with `sort_by`.
`compare` returns an `Ordering`: `Ordering.Less`, `Ordering.Equal` or `Ordering.Greater`.

## Maps

```wolf
module main

imports {
    std.map
}

main {
    var ages = ["Ann": 31, "Bob": 25]
    ages.insert("Cid", 40)
    ages["Bob"] += 1
    print(ages["Bob"].to_string())
    print(ages.contains_key("Dan").to_string())
    match map.get(ages, "Dan") {
        Some(age) => print(age.to_string())
        None => print("no Dan")
    }
    ages.remove("Ann")
    for name in ages.keys() {
        print(name + " " + ages[name].to_string())
    }
}
```

```output
26
false
no Dan
Bob 26
Cid 40
```

- `["a": 1, "b": 2]` builds a map, `[:]` is an empty one.
- `m[key]` reads a value and **panics if the key is missing**; `map.get(m, key)` returns an
  `Option` with a copy of the value instead.
- `m[key] = v` replaces the value of an existing key; it also panics if the key is missing. New keys
  are added with `m.insert(key, value)`, which returns the value it replaced, if any.
- `keys()`, `map.values(m)` and `entries()` return the contents in **insertion order**: the order
  in which the keys were first added. Replacing a value keeps the key in its place.
- Keys are `Int`, `String`, `Bool` or enums without data. These are the types that every target can
  use as keys by their value.

`insert` and `remove` return an `Option` that is not needed here; such a result may simply be
dropped. Only a `Result` must always be handled, as chapter 7 explains.
