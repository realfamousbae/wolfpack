use std::collections::HashMap;

pub struct Point {
    pub x: f64,
    pub y: f64,
}

pub struct Label {
    pub text: String,
    pub max_size: u32,
    pub tags: Vec<String>,
    pub at: Point,
}

pub fn distance(a: &Point, b: &Point) -> f64 {
    (b.x - a.x).hypot(b.y - a.y)
}

pub fn moved(p: &Point, dx: f64) -> Point {
    Point {
        x: p.x + dx,
        y: p.y,
    }
}

pub fn describe(l: &Label) -> String {
    format!(
        "{} ({}) {} at {},{}",
        l.text,
        l.max_size,
        l.tags.join(","),
        l.at.x,
        l.at.y
    )
}

static GROWN_CALLS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

pub fn grown(l: &Label) -> Label {
    GROWN_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut tags = l.tags.clone();
    tags.push("c".to_string());
    Label {
        text: l.text.to_uppercase(),
        max_size: l.max_size * 2,
        tags,
        at: Point {
            x: l.at.x,
            y: l.at.y,
        },
    }
}

pub fn corners(n: i64) -> Vec<Point> {
    (0..n)
        .map(|i| Point {
            x: i as f64,
            y: (10 - i) as f64,
        })
        .collect()
}

pub fn find(name: &str) -> Option<Point> {
    (name == "origin").then_some(Point { x: 0.0, y: 0.0 })
}

pub fn shifted(m: &HashMap<String, Point>) -> HashMap<String, Point> {
    m.iter()
        .map(|(k, p)| {
            (
                k.clone(),
                Point {
                    x: p.x + 1.0,
                    y: p.y,
                },
            )
        })
        .collect()
}

pub fn calls() -> i64 {
    GROWN_CALLS.load(std::sync::atomic::Ordering::Relaxed)
}
