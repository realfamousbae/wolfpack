pub struct Point {
    pub x: f64,
    pub y: f64,
}

pub struct Tag {
    pub name: String,
}

pub fn norm(p: &Point) -> f64 {
    p.x.hypot(p.y)
}

pub fn label() -> Tag {
    Tag {
        name: "a".to_string(),
    }
}
