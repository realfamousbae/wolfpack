//! Machine-readable output: one JSON object per diagnostic and line (spec chapter 9).
//!
//! Written by hand to avoid a JSON dependency; only output is needed.

use std::fmt::Write;

use wolf_span::SourceMap;

use crate::Diagnostic;
use crate::render::sanitize;

/// Renders diagnostics as JSON lines. The schema is described in spec chapter 9, section 3.
pub fn render_json(map: &SourceMap, diagnostics: &[Diagnostic]) -> String {
    let mut out = String::new();
    for diagnostic in diagnostics {
        out.push_str("{\"code\":");
        string(&mut out, diagnostic.code.id());
        out.push_str(",\"severity\":\"error\",\"message\":");
        string(&mut out, &diagnostic.message);
        out.push_str(",\"labels\":[");
        for (index, label) in diagnostic.labels.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let range = sanitize(map, label.span);
            let file = map.get(label.span.file);
            let name = file.map(|f| f.name()).unwrap_or("");
            let start = file.map(|f| f.line_col(offset(range.start)));
            let end = file.map(|f| f.line_col(offset(range.end)));
            out.push_str("{\"file\":");
            string(&mut out, name);
            let _ = write!(out, ",\"start\":{},\"end\":{}", range.start, range.end);
            if let (Some(start), Some(end)) = (start, end) {
                let _ = write!(
                    out,
                    ",\"line_start\":{},\"column_start\":{},\"line_end\":{},\"column_end\":{}",
                    start.line, start.column, end.line, end.column
                );
            }
            let _ = write!(out, ",\"primary\":{},\"message\":", label.primary);
            string(&mut out, &label.message);
            out.push('}');
        }
        out.push_str("],\"help\":");
        list(&mut out, &diagnostic.help);
        out.push_str(",\"notes\":");
        list(&mut out, &diagnostic.notes);
        out.push_str("}\n");
    }
    out
}

fn offset(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn list(out: &mut String, items: &[String]) {
    out.push('[');
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        string(out, item);
    }
    out.push(']');
}

/// Writes a JSON string literal with all required escapes.
fn string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes;
    use wolf_span::Span;

    #[test]
    fn escapes_strings() {
        let mut out = String::new();
        string(&mut out, "a\"b\\c\nd\u{1}é");
        assert_eq!(out, "\"a\\\"b\\\\c\\nd\\u0001é\"");
    }

    #[test]
    fn renders_one_line_per_diagnostic() {
        let mut map = SourceMap::new();
        let id = map.add("m.wolf", "ab\n\"\\q\"".to_string());
        let d = Diagnostic::new(codes::E0104, "unknown escape `\\q`")
            .primary(Span::new(id, 4, 6), "here")
            .help("use `\\\\`");
        let json = render_json(&map, &[d.clone(), d]);
        assert_eq!(json.lines().count(), 2);
        let first = json.lines().next().unwrap_or("");
        assert_eq!(
            first,
            "{\"code\":\"E0104\",\"severity\":\"error\",\"message\":\"unknown escape `\\\\q`\",\
             \"labels\":[{\"file\":\"m.wolf\",\"start\":4,\"end\":6,\"line_start\":2,\
             \"column_start\":2,\"line_end\":2,\"column_end\":4,\"primary\":true,\
             \"message\":\"here\"}],\"help\":[\"use `\\\\\\\\`\"],\"notes\":[]}"
        );
    }
}
