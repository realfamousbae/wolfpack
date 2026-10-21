use std::collections::{BTreeMap, HashMap};

pub struct Counter {
    n: u32,
}

pub fn slugify(title: &str) -> String {
    title.to_lowercase().replace(' ', "-")
}

pub fn owned(s: String) -> String {
    s + "!"
}

#[allow(clippy::ptr_arg)]
pub fn by_ref_string(s: &String) -> usize {
    s.len()
}

pub fn small(n: u8, m: i32, k: usize) -> u64 {
    n as u64 + m as u64 + k as u64
}

pub fn halve(x: f32) -> f32 {
    x / 2.0
}

pub fn sum_vec(v: Vec<u32>) -> i64 {
    v.iter().map(|&x| x as i64).sum()
}

#[allow(clippy::ptr_arg)]
pub fn sum_ref_vec(v: &Vec<i32>) -> i32 {
    v.iter().sum()
}

pub fn sum_slice(v: &[u16]) -> u32 {
    v.iter().map(|&x| x as u32).sum()
}

pub fn words(v: &[String]) -> Vec<String> {
    v.iter().map(|s| s.to_uppercase()).collect()
}

pub fn keys(m: HashMap<String, i32>) -> BTreeMap<String, u8> {
    m.into_iter().map(|(k, v)| (k, v as u8)).collect()
}

pub fn total(m: &HashMap<String, u64>) -> u64 {
    m.values().sum()
}

pub fn parse(s: &str) -> Result<u16, std::num::ParseIntError> {
    s.parse()
}

pub fn first(v: &[String]) -> Option<String> {
    v.first().cloned()
}

pub fn counter() -> Counter {
    Counter { n: 7 }
}

pub fn count(c: &Counter) -> u32 {
    c.n
}

pub fn each<F: Fn(&str) -> bool>(items: Vec<String>, f: F) -> usize {
    items.iter().filter(|s| f(s)).count()
}

pub fn change(v: &mut Vec<i64>) {
    v.push(1)
}

pub struct Size {
    pub w: u32,
    pub h: f32,
    pub tags: Vec<String>,
    pub note: Option<String>,
}

pub struct Frame {
    pub size: Size,
    pub name: String,
}

pub fn area(s: &Size) -> u64 {
    (s.w as f32 * s.h) as u64
}

pub fn grow(s: Size, by: u32) -> Size {
    Size {
        w: s.w + by,
        note: Some(format!("{} tags", s.tags.len())),
        ..s
    }
}

pub fn frames(n: usize) -> Vec<Frame> {
    (0..n)
        .map(|i| Frame {
            size: Size {
                w: i as u32,
                h: 0.5,
                tags: Vec::new(),
                note: None,
            },
            name: format!("f{i}"),
        })
        .collect()
}

pub fn find_frame(name: &str) -> Option<Frame> {
    frames(3).into_iter().find(|f| f.name == name)
}

pub fn parse_size(s: &str) -> Result<Size, String> {
    let (w, h) = s.split_once('x').ok_or("no x")?;
    Ok(Size {
        w: w.parse().map_err(|_| "bad width")?,
        h: h.parse().map_err(|_| "bad height")?,
        tags: vec![s.to_string()],
        note: None,
    })
}

pub struct Stock {
    pub counts: BTreeMap<String, u16>,
}

pub fn total_area(sizes: &[Size]) -> u64 {
    sizes.iter().map(area).sum()
}

pub fn widest(sizes: &HashMap<String, Size>) -> String {
    let mut best: Vec<(&String, &Size)> = sizes.iter().collect();
    best.sort_by_key(|(k, s)| (s.w, (*k).clone()));
    best.last().map(|(k, _)| (*k).clone()).unwrap_or_default()
}

pub fn restock(s: &Stock, item: &str) -> Stock {
    let mut counts = s.counts.clone();
    *counts.entry(item.to_string()).or_insert(0) += 1;
    Stock { counts }
}
