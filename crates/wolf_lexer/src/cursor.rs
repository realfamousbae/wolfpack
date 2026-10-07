//! A cursor over the characters of the source, with byte offsets.
//!
//! All access goes through `Option`, so the lexer cannot index out of bounds.

pub(crate) struct Cursor {
    /// Every character with the byte offset where it starts.
    chars: Vec<(u32, char)>,
    /// Index into `chars` of the next character.
    index: usize,
    /// Byte length of the text.
    len: u32,
}

impl Cursor {
    pub(crate) fn new(text: &str) -> Cursor {
        let chars = text
            .char_indices()
            .map(|(offset, c)| (u32::try_from(offset).unwrap_or(u32::MAX), c))
            .collect();
        Cursor {
            chars,
            index: 0,
            len: u32::try_from(text.len()).unwrap_or(u32::MAX),
        }
    }

    /// The character `n` positions ahead (0 is the next one).
    pub(crate) fn peek(&self, n: usize) -> Option<char> {
        self.chars.get(self.index + n).map(|&(_, c)| c)
    }

    /// The character just before the cursor.
    pub(crate) fn prev(&self) -> Option<char> {
        let index = self.index.checked_sub(1)?;
        self.chars.get(index).map(|&(_, c)| c)
    }

    /// Byte offset of the next character, or the text length at the end.
    pub(crate) fn offset(&self) -> u32 {
        self.chars
            .get(self.index)
            .map(|&(offset, _)| offset)
            .unwrap_or(self.len)
    }

    /// Consumes and returns the next character.
    pub(crate) fn bump(&mut self) -> Option<char> {
        let c = self.peek(0)?;
        self.index += 1;
        Some(c)
    }

    /// Consumes the next character if it is `expected`.
    pub(crate) fn eat(&mut self, expected: char) -> bool {
        if self.peek(0) == Some(expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }
}
