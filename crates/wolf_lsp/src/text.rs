//! Positions of the protocol: lines from 0 and columns in UTF-16 code units, as the Language
//! Server Protocol counts them by default.

use lsp_types::{Position, Range};

/// The position of a byte offset of `text`.
pub(crate) fn position(text: &str, offset: usize) -> Position {
    let offset = offset.min(text.len());
    let before = text.get(..offset).unwrap_or(text);
    let line = before.matches('\n').count();
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    let column: usize = before
        .get(start..)
        .unwrap_or("")
        .chars()
        .map(char::len_utf16)
        .sum();
    Position::new(line as u32, column as u32)
}

/// The range of a byte range of `text`.
pub(crate) fn range(text: &str, start: usize, end: usize) -> Range {
    Range::new(position(text, start), position(text, end))
}

/// The byte offset of a position of `text`; a position past the end of a line is its end.
pub(crate) fn offset(text: &str, position: Position) -> usize {
    let mut line_start = 0;
    for _ in 0..position.line {
        match text.get(line_start..).and_then(|rest| rest.find('\n')) {
            Some(i) => line_start += i + 1,
            None => return text.len(),
        }
    }
    let line = text.get(line_start..).unwrap_or("");
    let line = line.split('\n').next().unwrap_or("");
    let mut units = 0;
    for (i, c) in line.char_indices() {
        if units >= position.character as usize {
            return line_start + i;
        }
        units += c.len_utf16();
    }
    line_start + line.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_count_utf16_units() {
        let text = "ab\nя😀x\n";
        assert_eq!(position(text, 0), Position::new(0, 0));
        assert_eq!(position(text, 3), Position::new(1, 0));
        // `я` is one unit, `😀` two.
        let x = text.find('x').unwrap_or(0);
        assert_eq!(position(text, x), Position::new(1, 3));
        assert_eq!(offset(text, Position::new(1, 3)), x);
        assert_eq!(offset(text, Position::new(0, 99)), 2);
        assert_eq!(offset(text, Position::new(9, 0)), text.len());
    }
}
