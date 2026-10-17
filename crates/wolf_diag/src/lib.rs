//! Compiler diagnostics: codes, messages, source ranges, hints, and their output as text or JSON.
//!
//! The format follows spec chapter 9: every diagnostic is an error, has a unique code and at least
//! one label on a source range. Output is sorted by file and position so it is deterministic.

pub mod codes;
pub mod explain;
mod json;
mod render;

use wolf_span::Span;

pub use json::render_json;
pub use render::{render_human, render_human_to_string};

/// A diagnostic code such as `E0101`. The full list is in [`codes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Code {
    id: &'static str,
    title: &'static str,
}

impl Code {
    /// Creates a code. Only [`codes`] should call this.
    pub const fn new(id: &'static str, title: &'static str) -> Code {
        Code { id, title }
    }

    /// The code itself, for example `E0101`.
    pub fn id(self) -> &'static str {
        self.id
    }

    /// A short description of the error class.
    pub fn title(self) -> &'static str {
        self.title
    }
}

/// A source range with a message, attached to a diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    /// The range in the source.
    pub span: Span,
    /// Text shown next to the range. May be empty.
    pub message: String,
    /// `true` for the main range of the diagnostic, `false` for related places.
    pub primary: bool,
}

/// One compiler error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// The error code.
    pub code: Code,
    /// The main message: lower case, no final period (spec chapter 9).
    pub message: String,
    /// Source ranges. The first primary label decides where the diagnostic is sorted.
    pub labels: Vec<Label>,
    /// Suggestions for a fix, shown as `help:` lines.
    pub help: Vec<String>,
    /// Explanations, shown as `note:` lines.
    pub notes: Vec<String>,
}

impl Diagnostic {
    /// Creates a diagnostic without labels.
    pub fn new(code: Code, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            code,
            message: message.into(),
            labels: Vec::new(),
            help: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// Adds the primary label.
    pub fn primary(mut self, span: Span, message: impl Into<String>) -> Diagnostic {
        self.labels.push(Label {
            span,
            message: message.into(),
            primary: true,
        });
        self
    }

    /// Adds a secondary label.
    pub fn secondary(mut self, span: Span, message: impl Into<String>) -> Diagnostic {
        self.labels.push(Label {
            span,
            message: message.into(),
            primary: false,
        });
        self
    }

    /// Adds a `help:` line.
    pub fn help(mut self, text: impl Into<String>) -> Diagnostic {
        self.help.push(text.into());
        self
    }

    /// Adds a `note:` line.
    pub fn note(mut self, text: impl Into<String>) -> Diagnostic {
        self.notes.push(text.into());
        self
    }

    /// The span of the first primary label, if there is one.
    pub fn primary_span(&self) -> Option<Span> {
        self.labels
            .iter()
            .find(|label| label.primary)
            .map(|label| label.span)
    }
}

/// Sorts diagnostics by file, then position, then code (spec chapter 9). Diagnostics without a
/// primary label come first.
pub fn sort(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(|a, b| {
        let key = |d: &Diagnostic| d.primary_span().map(|s| (s.file, s.start, s.end));
        key(a)
            .cmp(&key(b))
            .then_with(|| a.code.id().cmp(b.code.id()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use wolf_span::SourceMap;

    #[test]
    fn sort_orders_by_file_and_position() {
        let mut map = SourceMap::new();
        let a = map.add("a.wolf", "xxxx".to_string());
        let b = map.add("b.wolf", "xxxx".to_string());
        let mut list = vec![
            Diagnostic::new(codes::E0102, "b").primary(Span::new(b, 0, 1), ""),
            Diagnostic::new(codes::E0102, "a2").primary(Span::new(a, 2, 3), ""),
            Diagnostic::new(codes::E0001, "none"),
            Diagnostic::new(codes::E0102, "a1").primary(Span::new(a, 0, 1), ""),
        ];
        sort(&mut list);
        let order: Vec<&str> = list.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(order, ["none", "a1", "a2", "b"]);
    }
}
