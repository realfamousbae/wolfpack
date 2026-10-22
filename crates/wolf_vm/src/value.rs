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
    pub fn deep_copy(&self) -> Value {
        match self {
            Value::List(l) => Value::list(l.borrow().iter().map(Value::deep_copy).collect()),
            Value::Map(m) => {
                let m = m.borrow();
                Value::Map(Rc::new(RefCell::new(MapObj {
                    keys: m.keys.clone(),
                    values: m.values.iter().map(Value::deep_copy).collect(),
                    index: m.index.clone(),
                })))
            }
            Value::Obj(o) => Value::obj(
                o.tag,
                o.fields.borrow().iter().map(Value::deep_copy).collect(),
            ),
            other => other.clone(),
        }
    }

    /// Structural equality (spec chapter 4, sections 3 and 4): IEEE for floats, maps without
    /// regard to order.
    pub fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Unit, Value::Unit) => true,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::List(a), Value::List(b)) => {
                if Rc::ptr_eq(a, b) {
                    return a.borrow().iter().all(|x| x.equals(x));
                }
                let (a, b) = (a.borrow(), b.borrow());
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.equals(y))
            }
            (Value::Map(a), Value::Map(b)) => {
                if Rc::ptr_eq(a, b) {
                    return a.borrow().values.iter().all(|x| x.equals(x));
                }
                let (a, b) = (a.borrow(), b.borrow());
                a.keys.len() == b.keys.len()
                    && a.keys.iter().zip(a.values.iter()).all(|(k, v)| {
                        b.position(k)
                            .and_then(|j| b.values.get(j))
                            .is_some_and(|w| v.equals(w))
                    })
            }
            (Value::Obj(a), Value::Obj(b)) => {
                if a.tag != b.tag {
                    return false;
                }
                if Rc::ptr_eq(a, b) {
                    return a.fields.borrow().iter().all(|x| x.equals(x));
                }
                let (x, y) = (a.fields.borrow(), b.fields.borrow());
                x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| p.equals(q))
            }
            (Value::Error(a), Value::Error(b)) => a == b,
            _ => false,
        }
    }
}
