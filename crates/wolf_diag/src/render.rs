//! Human-readable output, rendered with `codespan-reporting` in the style of rustc.

use std::io;
use std::ops::Range;

use codespan_reporting::diagnostic as cs;
use codespan_reporting::files::{Error as FilesError, Files};
use codespan_reporting::term::termcolor::WriteColor;
use codespan_reporting::term::{self, Chars, Config};
use wolf_span::{FileId, SourceMap, Span};

use crate::Diagnostic;

/// Writes diagnostics in the human format of spec chapter 9 to `out`.
///
/// Colours are used only if `out` supports them; pass a `NoColor` writer for plain text.
/// Rendering never panics: spans are clamped to the file and to character boundaries first.
pub fn render_human(
    map: &SourceMap,
    diagnostics: &[Diagnostic],
    out: &mut dyn WriteColor,
) -> io::Result<()> {
    let files = MapFiles(map);
    let config = config();
    for diagnostic in diagnostics {
        let converted = convert(map, diagnostic);
        if term::emit_to_write_style(out, &config, &files, &converted).is_err() {
            // The file database cannot fail for spans we sanitized, but never lose a message.
            writeln!(
                out,
                "error[{}]: {}",
                diagnostic.code.id(),
                diagnostic.message
            )?;
        }
    }
    Ok(())
}

/// Renders diagnostics as plain text without colours. Used by tests and by `--color never`.
pub fn render_human_to_string(map: &SourceMap, diagnostics: &[Diagnostic]) -> String {
    let mut buffer = codespan_reporting::term::termcolor::NoColor::new(Vec::new());
    // Writing into a Vec cannot fail.
    let _ = render_human(map, diagnostics, &mut buffer);
    String::from_utf8_lossy(&buffer.into_inner()).into_owned()
}

fn config() -> Config {
    Config {
        chars: Chars::ascii(),
        ..Config::default()
    }
}

fn convert(map: &SourceMap, diagnostic: &Diagnostic) -> cs::Diagnostic<FileId> {
    let labels = diagnostic
        .labels
        .iter()
        .map(|label| {
            let range = sanitize(map, label.span);
            let style = if label.primary {
                cs::LabelStyle::Primary
            } else {
                cs::LabelStyle::Secondary
            };
            cs::Label::new(style, label.span.file, range).with_message(label.message.clone())
        })
        .collect();
    let notes = diagnostic
        .help
        .iter()
        .map(|text| format!("help: {text}"))
        .chain(diagnostic.notes.iter().map(|text| format!("note: {text}")))
        .collect();
    cs::Diagnostic::error()
        .with_code(diagnostic.code.id())
        .with_message(diagnostic.message.clone())
        .with_labels(labels)
        .with_notes(notes)
}

/// Clamps a span to its file and widens it to character boundaries.
pub(crate) fn sanitize(map: &SourceMap, span: Span) -> Range<usize> {
    let Some(file) = map.get(span.file) else {
        return 0..0;
    };
    let text = file.text();
    let mut start = (span.start as usize).min(text.len());
    let mut end = (span.end as usize).min(text.len()).max(start);
    while !text.is_char_boundary(start) {
        start -= 1;
    }
    while !text.is_char_boundary(end) {
        end += 1;
    }
    start..end
}

struct MapFiles<'a>(&'a SourceMap);

impl<'a> Files<'a> for MapFiles<'a> {
    type FileId = FileId;
    type Name = &'a str;
    type Source = &'a str;

    fn name(&'a self, id: FileId) -> Result<&'a str, FilesError> {
        self.0
            .get(id)
            .map(|file| file.name())
            .ok_or(FilesError::FileMissing)
    }

    fn source(&'a self, id: FileId) -> Result<&'a str, FilesError> {
        self.0
            .get(id)
            .map(|file| file.text())
            .ok_or(FilesError::FileMissing)
    }

    fn line_index(&'a self, id: FileId, byte_index: usize) -> Result<usize, FilesError> {
        let file = self.0.get(id).ok_or(FilesError::FileMissing)?;
        let offset = u32::try_from(byte_index).unwrap_or(u32::MAX);
        Ok(file.line_index(offset))
    }

    fn line_range(&'a self, id: FileId, line_index: usize) -> Result<Range<usize>, FilesError> {
        let file = self.0.get(id).ok_or(FilesError::FileMissing)?;
        match file.line_range(line_index) {
            Some(range) => Ok(range),
            None => {
                // codespan asks for the line after the last one when a span ends at EOF.
                let len = file.text().len();
                Ok(len..len)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes;

    #[test]
    fn renders_rustc_style() {
        let mut map = SourceMap::new();
        let id = map.add("main.wolf", "let x = @\n".to_string());
        let d = Diagnostic::new(codes::E0102, "unexpected character `@`")
            .primary(Span::new(id, 8, 9), "not valid here")
            .help("remove it");
        let text = render_human_to_string(&map, &[d]);
        assert!(
            text.starts_with("error[E0102]: unexpected character `@`\n"),
            "{text}"
        );
        assert!(text.contains("--> main.wolf:1:9"), "{text}");
        assert!(text.contains("= help: remove it"), "{text}");
    }

    #[test]
    fn spans_inside_characters_do_not_panic() {
        let mut map = SourceMap::new();
        let id = map.add("main.wolf", "яя".to_string());
        let d = Diagnostic::new(codes::E0102, "x").primary(Span::new(id, 1, 3), "");
        let text = render_human_to_string(&map, &[d]);
        assert!(text.contains("main.wolf:1:1"), "{text}");
        let far = Diagnostic::new(codes::E0102, "x").primary(Span::new(id, 90, 99), "");
        let _ = render_human_to_string(&map, &[far]);
    }
}
