//! Values of the virtual machine.
//!
//! Values of the copy types (spec chapter 5, section 2) are held directly. Values of the move
//! types that can change in place (lists, maps, structs and enum variants) are shared objects:
//! the ownership rules guarantee that a value is never observed through two names while it can
//! change (chapter 5, section 9), so sharing is invisible, as in the TypeScript target.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// A value.
#[derive(Clone, Debug)]
pub enum Value {
    /// `()`
    Unit,
    /// An `Int`.
    Int(i64),
    /// A `Float`.
    Float(f64),
    /// A `Bool`.
    Bool(bool),
    /// A `String`; strings never change in place.
    Str(Rc<str>),
    /// A `List`.
    List(Rc<RefCell<Vec<Value>>>),
    /// A `Map`.
    Map(Rc<RefCell<MapObj>>),
    /// A struct or an enum variant (also `Some`, `None`, `Ok`, `Err` and `Ordering`).
    Obj(Rc<Obj>),
    /// A function value.
    Closure(Rc<Closure>),
    /// The error made by `error(message)`.
    Error(Rc<str>),
    /// A reference to a place, held by a `mut` parameter or binding.
    Ref(Place),
}

/// A struct or a variant: the declaration and the fields or payload in declaration order.
#[derive(Debug)]
pub struct Obj {
    /// The struct or the variant.
    pub tag: u32,
    /// The fields.
    pub fields: RefCell<Vec<Value>>,
}

/// A function value: the function and the values it captured.
#[derive(Debug)]
pub struct Closure {
    /// The function.
    pub func: u32,
    /// The captured values, in the order of the function's capture slots.
    pub captures: Vec<Value>,
}

/// A place that a reference points to.
#[derive(Clone, Debug)]
pub enum Place {
    /// A slot of the stack, by absolute index.
    Slot(usize),
    /// A field of a struct or a value of a variant's payload.
    Field(Rc<Obj>, usize),
    /// An element of a list.
    Elem(Rc<RefCell<Vec<Value>>>, usize),
    /// The value of a map entry, by position.
    Entry(Rc<RefCell<MapObj>>, usize),
}

/// A key of a map: the values of the `Hashable` types.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// An `Int`.
    Int(i64),
    /// A `String`.
    Str(Rc<str>),
    /// A `Bool`.
    Bool(bool),
    /// A variant without payload.
    Tag(u32),
}

impl Key {
    /// The key of a value, if it is `Hashable`.
    pub fn of(v: &Value) -> Option<Key> {
        Some(match v {
            Value::Int(i) => Key::Int(*i),
            Value::Str(s) => Key::Str(s.clone()),
            Value::Bool(b) => Key::Bool(*b),
            Value::Obj(o) => Key::Tag(o.tag),
            _ => return None,
        })
    }
}

/// A map that keeps the order in which keys were first inserted (ADR 0003).
#[derive(Clone, Debug, Default)]
pub struct MapObj {
    /// The keys in order.
    pub keys: Vec<Value>,
    /// The values, at the positions of their keys.
    pub values: Vec<Value>,
    /// The position of each key.
    pub index: HashMap<Key, usize>,
}

impl MapObj {
    /// The position of a key.
    pub fn position(&self, key: &Value) -> Option<usize> {
        Key::of(key).and_then(|k| self.index.get(&k).copied())
    }

    /// Inserts or replaces; the replaced value is returned.
    pub fn insert(&mut self, key: Value, value: Value) -> Option<Value> {
        let k = Key::of(&key)?;
        if let Some(&i) = self.index.get(&k) {
            return self
                .values
                .get_mut(i)
                .map(|slot| std::mem::replace(slot, value));
        }
        self.index.insert(k, self.keys.len());
        self.keys.push(key);
        self.values.push(value);
        None
    }

    /// Removes a key; its value is returned.
    pub fn remove(&mut self, key: &Value) -> Option<Value> {
        let k = Key::of(key)?;
        let i = self.index.remove(&k)?;
        if i >= self.keys.len() {
            return None;
        }
        self.keys.remove(i);
        let value = self.values.remove(i);
        for (j, k) in self.keys.iter().enumerate().skip(i) {
            if let Some(k) = Key::of(k) {
                self.index.insert(k, j);
            }
        }
        Some(value)
    }

    /// Removes every entry.
    pub fn clear(&mut self) {
        self.keys.clear();
        self.values.clear();
        self.index.clear();
    }
}

impl Value {
    /// A string value.
    pub fn str(s: &str) -> Value {
        Value::Str(Rc::from(s))
    }

    /// A list value.
    pub fn list(items: Vec<Value>) -> Value {
        Value::List(Rc::new(RefCell::new(items)))
    }

    /// A struct or variant.
    pub fn obj(tag: u32, fields: Vec<Value>) -> Value {
        Value::Obj(Rc::new(Obj {
            tag,
            fields: RefCell::new(fields),
        }))
    }

    /// A deep copy (`.copy()`): changes of the copy are not seen in the original.
    ///
    /// The copy is made without recursion, so the depth of the value does not matter (spec
    /// chapter 4, section 7.4, rule 3): each list, map or object is first copied with the parts of
    /// the original in it, and those parts are then replaced by their own copies, one at a time.
    pub fn deep_copy(&self) -> Value {
        let Some((top, mut pending)) = self.copy_one_level() else {
            return self.clone();
        };
        while let Some(place) = pending.pop() {
            let part = match &place {
                Place::Field(o, i) => o.fields.borrow().get(*i).cloned(),
                Place::Elem(l, i) => l.borrow().get(*i).cloned(),
                Place::Entry(m, i) => m.borrow().values.get(*i).cloned(),
                Place::Slot(_) => None,
            };
            let Some((copy, parts)) = part.and_then(|p| p.copy_one_level()) else {
                continue;
            };
            pending.extend(parts);
            match &place {
                Place::Field(o, i) => set(o.fields.borrow_mut().get_mut(*i), copy),
                Place::Elem(l, i) => set(l.borrow_mut().get_mut(*i), copy),
                Place::Entry(m, i) => set(m.borrow_mut().values.get_mut(*i), copy),
                Place::Slot(_) => {}
            }
        }
        top
    }

    /// A new list, map or object whose parts are still those of `self`, with the places of the
    /// parts that must be copied in turn; `None` for the values that are copied as they are.
    fn copy_one_level(&self) -> Option<(Value, Vec<Place>)> {
        fn deep(parts: &[Value]) -> Vec<usize> {
            let nested = |(i, v): (usize, &Value)| {
                matches!(v, Value::List(_) | Value::Map(_) | Value::Obj(_)).then_some(i)
            };
            parts.iter().enumerate().filter_map(nested).collect()
        }
        Some(match self {
            Value::List(l) => {
                let items = l.borrow().clone();
                let at = deep(&items);
                let new = Rc::new(RefCell::new(items));
                let places = at.into_iter().map(|i| Place::Elem(new.clone(), i));
                (Value::List(new.clone()), places.collect())
            }
            Value::Map(m) => {
                let m = m.borrow();
                let at = deep(&m.values);
                let new = Rc::new(RefCell::new(MapObj {
                    keys: m.keys.clone(),
                    values: m.values.clone(),
                    index: m.index.clone(),
                }));
                let places = at.into_iter().map(|i| Place::Entry(new.clone(), i));
                (Value::Map(new.clone()), places.collect())
            }
            Value::Obj(o) => {
                let fields = o.fields.borrow().clone();
                let at = deep(&fields);
                let new = Rc::new(Obj {
                    tag: o.tag,
                    fields: RefCell::new(fields),
                });
                let places = at.into_iter().map(|i| Place::Field(new.clone(), i));
                (Value::Obj(new.clone()), places.collect())
            }
            _ => return None,
        })
    }

    /// Structural equality (spec chapter 4, sections 3 and 4): IEEE for floats, maps without
    /// regard to order. Like [`Value::deep_copy`] it keeps a list of the pairs of parts still to
    /// compare instead of calling itself.
    pub fn equals(&self, other: &Value) -> bool {
        let mut pending = Vec::new();
        let mut pair = (self.clone(), other.clone());
        loop {
            if !equals_one_level(&pair.0, &pair.1, &mut pending) {
                return false;
            }
            match pending.pop() {
                Some(next) => pair = next,
                None => return true,
            }
        }
    }
}

/// Stores `value` in `slot`, when there is one.
fn set(slot: Option<&mut Value>, value: Value) {
    if let Some(slot) = slot {
        *slot = value;
    }
}

/// Compares two values one level deep; the pairs of parts that must also be equal are pushed to
/// `pending`.
fn equals_one_level(a: &Value, b: &Value, pending: &mut Vec<(Value, Value)>) -> bool {
    match (a, b) {
        (Value::Unit, Value::Unit) => true,
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Str(a), Value::Str(b)) => a == b,
        (Value::List(a), Value::List(b)) => {
            let (a, b) = (a.borrow(), b.borrow());
            if a.len() != b.len() {
                return false;
            }
            pending.extend(a.iter().cloned().zip(b.iter().cloned()));
            true
        }
        (Value::Map(a), Value::Map(b)) => {
            let (a, b) = (a.borrow(), b.borrow());
            if a.keys.len() != b.keys.len() {
                return false;
            }
            for (k, v) in a.keys.iter().zip(a.values.iter()) {
                match b.position(k).and_then(|j| b.values.get(j)) {
                    Some(w) => pending.push((v.clone(), w.clone())),
                    None => return false,
                }
            }
            true
        }
        (Value::Obj(a), Value::Obj(b)) => {
            if a.tag != b.tag {
                return false;
            }
            let (x, y) = (a.fields.borrow(), b.fields.borrow());
            if x.len() != y.len() {
                return false;
            }
            pending.extend(x.iter().cloned().zip(y.iter().cloned()));
            true
        }
        (Value::Error(a), Value::Error(b)) => a == b,
        _ => false,
    }
}

/// Drops values without recursion (spec chapter 4, section 7.4, rule 3). The parts of a list, map,
/// object or closure that is freed now are moved out of it before it is freed, so freeing a chain
/// a million levels deep takes no more stack than freeing one link.
fn drop_flat(mut pending: Vec<Value>) {
    while let Some(v) = pending.pop() {
        match v {
            Value::Obj(o) => {
                if let Ok(mut o) = Rc::try_unwrap(o) {
                    pending.append(o.fields.get_mut());
                }
            }
            Value::List(l) => {
                if let Ok(l) = Rc::try_unwrap(l) {
                    pending.append(&mut l.into_inner());
                }
            }
            Value::Map(m) => {
                if let Ok(m) = Rc::try_unwrap(m) {
                    pending.append(&mut m.into_inner().values);
                }
            }
            Value::Closure(c) => {
                if let Ok(mut c) = Rc::try_unwrap(c) {
                    pending.append(&mut c.captures);
                }
            }
            _ => {}
        }
    }
}

impl Drop for Obj {
    fn drop(&mut self) {
        let fields = std::mem::take(self.fields.get_mut());
        if !fields.is_empty() {
            drop_flat(fields);
        }
    }
}

impl Drop for Closure {
    fn drop(&mut self) {
        let captures = std::mem::take(&mut self.captures);
        if !captures.is_empty() {
            drop_flat(captures);
        }
    }
}
