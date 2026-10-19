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
