/// Counts.
pub struct Counter {
    count: i64,
}

impl Counter {
    /// A counter at zero.
    pub fn new() -> Counter {
        Counter { count: 0 }
    }

    /// A counter `n` higher.
    pub fn bump(&self, n: i64) -> Counter {
        Counter {
            count: self.count + n,
        }
    }

    /// The count.
    pub fn value(&self) -> i64 {
        self.count
    }
}

/// A counter from its count.
pub fn parse(s: &str) -> Result<Counter, String> {
    s.parse()
        .map(|count| Counter { count })
        .map_err(|_| format!("not a number: {s}"))
}

/// Not called from wolfpack code.
pub fn unused() -> i64 {
    0
}
