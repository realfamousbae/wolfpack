use std::collections::HashMap;

pub struct Point {
    pub x: f64,
    pub y: f64,
}

pub struct Counter {
    n: i64,
}

pub fn counter() -> Counter {
    Counter { n: 7 }
}

pub fn int_or(n: Option<i64>, d: i64) -> i64 {
    n.unwrap_or(d)
}

pub fn float_or(x: Option<f64>) -> f64 {
    x.unwrap_or(-1.0)
}

pub fn text_or(s: Option<&str>) -> String {
    s.unwrap_or("none").to_string()
}

pub fn count(l: Option<&[String]>) -> i64 {
    l.map_or(-1, |l| l.len() as i64)
}

pub fn total(m: Option<&HashMap<String, i64>>) -> i64 {
    m.map_or(-1, |m| m.values().sum())
}

pub fn x_of(p: Option<&Point>) -> f64 {
    p.map_or(-1.0, |p| p.x)
}

pub fn ticks(c: Option<&Counter>) -> i64 {
    c.map_or(-1, |c| c.n)
}
