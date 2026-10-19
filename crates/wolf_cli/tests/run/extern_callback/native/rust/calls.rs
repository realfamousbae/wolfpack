pub fn each(items: &[String], f: impl Fn(&str)) {
    for s in items {
        f(s);
    }
}

pub fn twice(n: i64, f: &dyn Fn(i64) -> i64) -> i64 {
    f(f(n))
}

pub fn mapped<F: Fn(i64) -> String>(xs: &[i64], f: F) -> Vec<String> {
    xs.iter().map(|&x| f(x)).collect()
}

pub fn count(items: &[String], f: impl Fn(&str) -> bool) -> Result<usize, String> {
    if items.is_empty() {
        return Err("nothing to count".to_string());
    }
    Ok(items.iter().filter(|s| f(s)).count())
}
