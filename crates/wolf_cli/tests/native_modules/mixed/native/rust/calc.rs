pub struct Point {
    pub x: f64,
    pub count: u32,
}

pub fn scale(n: i64, by: f64) -> f64 {
    n as f64 * by
}

pub fn mean(v: &[i64]) -> f64 {
    v.iter().sum::<i64>() as f64 / v.len() as f64
}

pub fn bump(p: &Point) -> Point {
    Point {
        x: p.x,
        count: p.count + 1,
    }
}

pub fn label(n: u8) -> String {
    format!("#{n}")
}
