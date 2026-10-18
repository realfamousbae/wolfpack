//! A small pretty printer in the style of Wadler's "prettier printer": a document of text, line
//! breaks and groups, printed within a width. A group is printed on one line if it fits, and with
//! all its line breaks otherwise.

/// Indentation per level.
pub(crate) const INDENT: usize = 4;
/// The width lines are kept within.
pub(crate) const WIDTH: usize = 100;

/// A document.
#[derive(Clone, Debug)]
pub(crate) enum Doc {
    /// Text without newlines.
    Text(String),
    /// A space, or a newline when the group is broken.
    Line,
    /// Nothing, or a newline when the group is broken.
    Soft,
    /// Always a newline; the enclosing groups are broken.
    Hard,
    /// Text printed only when the group is broken (a trailing comma).
    IfBreak(String),
    /// One more level of indentation after the newlines inside.
    Nest(Box<Doc>),
    /// Printed on one line if it fits.
    Group(Box<Doc>),
    /// Always printed on one line.
    Flat(Box<Doc>),
    /// A sequence.
    Cat(Vec<Doc>),
    /// Items separated by a space or a newline: in a broken group each item goes on the current
    /// line if it fits there, else on a new line.
    Fill(Vec<Doc>),
    /// A multiline string: the opening line, the lines of the content relative to the closing
    /// quotes, and the closing quotes. The content and the closing quotes are indented one level
    /// deeper than the current line; the content is printed as it is, trailing spaces included.
    Verbatim(String, Vec<String>, String),
}

pub(crate) fn text(s: impl Into<String>) -> Doc {
    Doc::Text(s.into())
}

pub(crate) fn cat(parts: Vec<Doc>) -> Doc {
    Doc::Cat(parts)
}

pub(crate) fn nest(d: Doc) -> Doc {
    Doc::Nest(Box::new(d))
}

pub(crate) fn group(d: Doc) -> Doc {
    Doc::Group(Box::new(d))
}

pub(crate) fn flat(d: Doc) -> Doc {
    Doc::Flat(Box::new(d))
}

/// Whether the document always breaks.
pub(crate) fn has_hard(d: &Doc) -> bool {
    match d {
        Doc::Hard => true,
        Doc::Nest(x) | Doc::Group(x) => has_hard(x),
        Doc::Flat(_) => false,
        Doc::Cat(xs) | Doc::Fill(xs) => xs.iter().any(has_hard),
        _ => false,
    }
}

/// The width of the document on one line, or `None` if it cannot be on one line.
pub(crate) fn flat_width(d: &Doc) -> Option<usize> {
    match d {
        Doc::Text(s) | Doc::Verbatim(s, _, _) => Some(s.chars().count()),
        Doc::Line => Some(1),
        Doc::Soft | Doc::IfBreak(_) => Some(0),
        Doc::Hard => None,
        Doc::Nest(x) | Doc::Group(x) | Doc::Flat(x) => flat_width(x),
        Doc::Cat(xs) => xs.iter().map(flat_width).sum(),
        Doc::Fill(xs) => {
            let w: Option<usize> = xs.iter().map(flat_width).sum();
            w.map(|w| w + xs.len().saturating_sub(1))
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Flat,
    Break,
}

/// What the printer does next: a document, or the rest of a fill from an item on.
enum Cmd<'d> {
    Doc(usize, Mode, &'d Doc),
    Fill(usize, &'d [Doc]),
}

/// Prints the document.
pub(crate) fn print(d: &Doc) -> String {
    let mut out = String::new();
    let mut col = 0usize;
    let mut stack: Vec<Cmd<'_>> = vec![Cmd::Doc(0, Mode::Break, d)];
    while let Some(cmd) = stack.pop() {
        let (ind, mode, d) = match cmd {
            Cmd::Doc(i, m, d) => (i, m, d),
            Cmd::Fill(ind, items) => {
                // The separator before the first remaining item.
                let Some((first, rest)) = items.split_first() else {
                    continue;
                };
                let fits_here = flat_width(first).is_some_and(|w| col + 1 + w <= WIDTH);
                if fits_here {
                    out.push(' ');
                    col += 1;
                    stack.push(Cmd::Fill(ind, rest));
                    stack.push(Cmd::Doc(ind, Mode::Flat, first));
                } else {
                    newline(&mut out, &mut col, ind);
                    stack.push(Cmd::Fill(ind, rest));
                    stack.push(Cmd::Doc(ind, Mode::Break, first));
                }
                continue;
            }
        };
        match d {
            Doc::Text(s) => {
                out.push_str(s);
                col += s.chars().count();
            }
            Doc::Verbatim(first, lines, close) => {
                // One level deeper than the line the string starts on.
                let line = out.rsplit('\n').next().unwrap_or("");
                let line_indent = line.len() - line.trim_start_matches(' ').len();
                out.push_str(first);
                let inner = " ".repeat(line_indent + INDENT);
                for line in lines {
                    out.push('\n');
                    if !line.is_empty() {
                        out.push_str(&inner);
                        out.push_str(line);
                    }
                }
                out.push('\n');
                out.push_str(&inner);
                out.push_str(close);
                col = line_indent + INDENT + close.chars().count();
            }
            Doc::Line | Doc::Soft if mode == Mode::Flat => {
                if matches!(d, Doc::Line) {
                    out.push(' ');
                    col += 1;
                }
            }
            Doc::Line | Doc::Soft | Doc::Hard => newline(&mut out, &mut col, ind),
            Doc::IfBreak(s) => {
                if mode == Mode::Break {
                    out.push_str(s);
                    col += s.chars().count();
                }
            }
            Doc::Nest(x) => stack.push(Cmd::Doc(ind + INDENT, mode, x)),
            Doc::Flat(x) => stack.push(Cmd::Doc(ind, Mode::Flat, x)),
            Doc::Group(x) => {
                let m = if mode == Mode::Flat
                    || (!has_hard(x) && fits(WIDTH.saturating_sub(col), x, &stack))
                {
                    Mode::Flat
                } else {
                    Mode::Break
                };
                stack.push(Cmd::Doc(ind, m, x));
            }
            Doc::Cat(xs) => {
                for x in xs.iter().rev() {
                    stack.push(Cmd::Doc(ind, mode, x));
                }
            }
            Doc::Fill(xs) => {
                if mode == Mode::Flat {
                    for (i, x) in xs.iter().enumerate().rev() {
                        stack.push(Cmd::Doc(ind, Mode::Flat, x));
                        if i > 0 {
                            stack.push(Cmd::Doc(ind, Mode::Flat, &Doc::Line));
                        }
                    }
                } else if let Some((first, rest)) = xs.split_first() {
                    stack.push(Cmd::Fill(ind, rest));
                    stack.push(Cmd::Doc(ind, Mode::Break, first));
                }
            }
        }
    }
    out
}

fn newline(out: &mut String, col: &mut usize, ind: usize) {
    while out.ends_with(' ') {
        out.pop();
    }
    out.push('\n');
    out.push_str(&" ".repeat(ind));
    *col = ind;
}

/// Whether `d` on one line, followed by the rest up to its next possible line break, fits in
/// `room` columns.
fn fits(room: usize, d: &Doc, rest: &[Cmd<'_>]) -> bool {
    let mut room = room as isize;
    let mut items: Vec<(Mode, &Doc)> = vec![(Mode::Flat, d)];
    let mut rest = rest.iter().rev();
    loop {
        let next = items.pop().or_else(|| match rest.next() {
            Some(Cmd::Doc(_, m, d)) => Some((*m, *d)),
            // A fill may break before its next item.
            Some(Cmd::Fill(..)) | None => None,
        });
        let Some((mode, d)) = next else {
            return room >= 0;
        };
        match d {
            Doc::Text(s) | Doc::Verbatim(s, _, _) => room -= s.chars().count() as isize,
            Doc::Line | Doc::Soft | Doc::Hard if mode == Mode::Break => return room >= 0,
            Doc::Hard => return false,
            Doc::Line => room -= 1,
            Doc::Soft | Doc::IfBreak(_) => {}
            Doc::Nest(x) | Doc::Group(x) => items.push((mode, x)),
            Doc::Flat(x) => items.push((Mode::Flat, x)),
            Doc::Cat(xs) => {
                for x in xs.iter().rev() {
                    items.push((mode, x));
                }
            }
            Doc::Fill(xs) => {
                for (i, x) in xs.iter().enumerate().rev() {
                    items.push((mode, x));
                    if i > 0 {
                        items.push((mode, &Doc::Line));
                    }
                }
            }
        }
        if room < 0 {
            return false;
        }
    }
}
