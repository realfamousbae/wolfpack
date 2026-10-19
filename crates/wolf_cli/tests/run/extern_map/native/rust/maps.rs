use std::collections::{BTreeMap, HashMap};

pub fn total(m: &HashMap<String, i64>) -> i64 {
    m.values().sum()
}

pub fn counts(words: &[String]) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for w in words {
        *out.entry(w.clone()).or_insert(0) += 1;
    }
    out
}

pub fn halves(m: &HashMap<i64, f64>) -> BTreeMap<i64, f64> {
    m.iter().map(|(k, v)| (k * 2, v / 2.0)).collect()
}

pub fn split(ns: &[i64]) -> HashMap<bool, Vec<i64>> {
    let mut out = HashMap::from([(true, Vec::new()), (false, Vec::new())]);
    for &n in ns {
        out.entry(n % 2 == 0).or_default().push(n);
    }
    out
}

// A vector of pairs, with repeated keys: the last value of a key wins.
pub fn letters(s: &str) -> Vec<(String, u32)> {
    let mut seen: HashMap<char, u32> = HashMap::new();
    s.chars()
        .map(|c| {
            let n = seen.entry(c).or_insert(0);
            *n += 1;
            (c.to_string(), *n)
        })
        .collect()
}

pub fn nest(m: &HashMap<String, HashMap<String, i64>>) -> HashMap<String, HashMap<String, i64>> {
    m.clone()
}

pub fn width(ms: &[HashMap<String, i64>]) -> usize {
    ms.iter().map(HashMap::len).sum()
}
