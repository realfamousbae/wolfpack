//! Source files, positions and ranges.
//!
//! Every file the compiler reads is stored once in a [`SourceMap`] and identified by a [`FileId`].
//! Positions inside a file are byte offsets ([`Span`]). Line and column numbers are computed only
//! when a diagnostic is shown; columns count Unicode scalar values (spec chapter 1, section 1).

use std::ops::Range;

/// Identifies a file inside a [`SourceMap`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(u32);

impl FileId {
    /// The index of the file in its [`SourceMap`].
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A range of bytes in one file. `start` is inclusive, `end` is exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Span {
    /// The file the range belongs to.
    pub file: FileId,
    /// Offset of the first byte.
    pub start: u32,
    /// Offset one past the last byte.
    pub end: u32,
}

impl Span {
    /// Creates a span. If `end < start`, the span is empty at `start`.
    pub fn new(file: FileId, start: u32, end: u32) -> Span {
        Span {
            file,
            start,
            end: end.max(start),
        }
    }

    /// The byte range of the span, for slicing the source text.
    pub fn range(self) -> Range<usize> {
        self.start as usize..self.end as usize
    }

    /// Length in bytes.
    pub fn len(self) -> u32 {
        self.end - self.start
    }

    /// Whether the span covers no bytes.
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// The smallest span covering both `self` and `other`. Both must be in the same file;
    /// otherwise `self` is returned.
    pub fn to(self, other: Span) -> Span {
        if self.file != other.file {
            return self;
        }
        Span::new(
            self.file,
            self.start.min(other.start),
            self.end.max(other.end),
        )
    }
}

/// A one-based line and column. The column counts Unicode scalar values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineCol {
    /// Line number, starting at 1.
    pub line: u32,
    /// Column number, starting at 1, in Unicode scalar values.
    pub column: u32,
}

/// The largest file the compiler accepts, in bytes. Offsets are stored as `u32`.
pub const MAX_FILE_SIZE: usize = u32::MAX as usize;

/// One source file: its display name and its text.
#[derive(Debug)]
pub struct SourceFile {
    name: String,
    text: String,
    line_starts: Vec<u32>,
}

impl SourceFile {
    fn new(name: String, text: String) -> SourceFile {
        let mut line_starts = vec![0];
        for (offset, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(to_u32(offset + 1));
            }
        }
        SourceFile {
            name,
            text,
            line_starts,
        }
    }

    /// The name shown in diagnostics, usually a path relative to the working directory.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The full text of the file.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The zero-based index of the line containing `offset`.
    pub fn line_index(&self, offset: u32) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(next) => next.saturating_sub(1),
        }
    }

    /// The byte range of the zero-based line `index`, including its line terminator.
    /// Returns `None` if there is no such line.
    pub fn line_range(&self, index: usize) -> Option<Range<usize>> {
        let start = *self.line_starts.get(index)? as usize;
        let end = match self.line_starts.get(index + 1) {
            Some(next) => *next as usize,
            None => self.text.len(),
        };
        Some(start..end)
    }

    /// Line and column of `offset`. An offset inside a multi-byte character, or past the end,
    /// is clamped to the nearest valid position before it.
    pub fn line_col(&self, offset: u32) -> LineCol {
        let line = self.line_index(offset);
        let line_start = self.line_starts.get(line).copied().unwrap_or(0) as usize;
        let offset = (offset as usize).min(self.text.len()).max(line_start);
        let prefix = self.text.get(line_start..offset).unwrap_or("");
        LineCol {
            line: to_u32(line + 1),
            column: to_u32(prefix.chars().count() + 1),
        }
    }
}

/// All source files of one compilation.
#[derive(Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    /// Creates an empty map.
    pub fn new() -> SourceMap {
        SourceMap::default()
    }

    /// Adds a file and returns its id. The caller must ensure `text.len() <= MAX_FILE_SIZE`;
    /// longer text is truncated at a character boundary.
    pub fn add(&mut self, name: impl Into<String>, mut text: String) -> FileId {
        if text.len() > MAX_FILE_SIZE {
            let mut cut = MAX_FILE_SIZE;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
        }
        let id = FileId(to_u32(self.files.len()));
        self.files.push(SourceFile::new(name.into(), text));
        id
    }

    /// The file with the given id, if it exists.
    pub fn get(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(id.index())
    }

    /// Iterates over all files with their ids.
    pub fn iter(&self) -> impl Iterator<Item = (FileId, &SourceFile)> {
        self.files
            .iter()
            .enumerate()
            .map(|(index, file)| (FileId(to_u32(index)), file))
    }
}

/// Converts a length or offset to `u32`, saturating. Files are limited to [`MAX_FILE_SIZE`],
/// so saturation never happens for valid offsets.
fn to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> SourceFile {
        SourceFile::new("test.wolf".to_string(), text.to_string())
    }

    #[test]
    fn line_col_counts_scalar_values() {
        let f = file("ab\nпривет x\n");
        assert_eq!(f.line_col(0), LineCol { line: 1, column: 1 });
        assert_eq!(f.line_col(3), LineCol { line: 2, column: 1 });
        // "привет " is 13 bytes, 7 scalar values
        assert_eq!(f.line_col(3 + 13), LineCol { line: 2, column: 8 });
    }

    #[test]
    fn line_col_at_end_of_file() {
        let f = file("a\nb");
        assert_eq!(f.line_col(3), LineCol { line: 2, column: 2 });
        assert_eq!(f.line_col(100), LineCol { line: 2, column: 2 });
    }

    #[test]
    fn line_col_inside_a_character_is_clamped() {
        let f = file("я");
        assert_eq!(f.line_col(1).line, 1);
    }

    #[test]
    fn line_ranges_include_terminators() {
        let f = file("a\r\nbc\n");
        assert_eq!(f.line_range(0), Some(0..3));
        assert_eq!(f.line_range(1), Some(3..6));
        assert_eq!(f.line_range(2), Some(6..6));
        assert_eq!(f.line_range(3), None);
    }

    #[test]
    fn span_to_merges() {
        let mut map = SourceMap::new();
        let id = map.add("a", "hello".to_string());
        let a = Span::new(id, 1, 2);
        let b = Span::new(id, 3, 5);
        assert_eq!(a.to(b), Span::new(id, 1, 5));
        assert_eq!(Span::new(id, 4, 2).len(), 0);
    }
}
