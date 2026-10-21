use std::cell::RefCell;

pub struct Index {
    words: RefCell<Vec<String>>,
}

impl Index {
    pub fn new() -> Index {
        Index { words: RefCell::new(Vec::new()) }
    }
    pub fn add(&self, w: &str) {
        self.words.borrow_mut().push(w.to_string());
    }
    pub fn len(&self) -> usize {
        self.words.borrow().len()
    }
}
