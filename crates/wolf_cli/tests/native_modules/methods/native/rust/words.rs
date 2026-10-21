use std::cell::RefCell;

pub struct Index {
    items: RefCell<Vec<String>>,
}

impl Index {
    pub fn add(&self, word: &str) {
        self.items.borrow_mut().push(word.to_string());
    }

    pub fn len(&self) -> usize {
        self.items.borrow().len()
    }

    pub fn has(&self, word: &str) -> bool {
        self.items.borrow().iter().any(|w| w == word)
    }

    pub fn joined(&self, sep: &str) -> String {
        self.items.borrow().join(sep)
    }

    pub fn clear(&mut self) {
        self.items.borrow_mut().clear();
    }
}

pub fn new_index() -> Index {
    Index {
        items: RefCell::new(Vec::new()),
    }
}
