//! Long descriptions of the codes, shown by `wolf explain` (spec chapter 9, section 2, rule 8).
//!
//! Each text is a Markdown file `explain/<code>.md` of this crate, embedded at compile time, so
//! the compiler needs no files at run time. A text has a heading, the rule and the usual fix, and
//! a wrong and a fixed example. Examples in ```` ```wolf wrong ```` and ```` ```wolf fixed ````
//! blocks are checked by the tests of `wolf_cli`.

use crate::Code;
use crate::codes::{ALL, RETIRED};

/// What `wolf explain` finds for a code written by the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Explanation {
    /// A code in use and its text.
    Text(Code, &'static str),
    /// A code that is no longer used.
    Retired(&'static str),
    /// No such code.
    Unknown,
}

/// Looks up a code as the user writes it: `E0501`, or `e0501`.
pub fn explain(input: &str) -> Explanation {
    let id = match input.strip_prefix('e') {
        Some(rest) => format!("E{rest}"),
        None => input.to_string(),
    };
    if let Some(code) = ALL.iter().find(|c| c.id() == id) {
        return match text(code.id()) {
            Some(text) => Explanation::Text(*code, text),
            None => Explanation::Unknown,
        };
    }
    match RETIRED.iter().find(|r| **r == id) {
        Some(retired) => Explanation::Retired(retired),
        None => Explanation::Unknown,
    }
}

/// The text of a code in use, if it has one.
pub fn text(id: &str) -> Option<&'static str> {
    TEXTS.iter().find(|(c, _)| *c == id).map(|(_, t)| *t)
}

/// Every code that has a text, with the text.
pub fn texts() -> &'static [(&'static str, &'static str)] {
    TEXTS
}

macro_rules! texts {
    ($($id:literal),* $(,)?) => {
        &[$(($id, include_str!(concat!("../explain/", $id, ".md")))),*]
    };
}

const TEXTS: &[(&str, &str)] = texts![
    "E0001", "E0002", "E0003", "E0004", "E0101", "E0102", "E0103", "E0104", "E0105", "E0106",
    "E0107", "E0108", "E0109", "E0110", "E0111", "E0112", "E0113", "E0201", "E0202", "E0203",
    "E0204", "E0205", "E0206", "E0207", "E0208", "E0209", "E0210", "E0211", "E0212", "E0213",
    "E0301", "E0302", "E0303", "E0304", "E0305", "E0306", "E0307", "E0308", "E0309", "E0310",
    "E0311", "E0312", "E0313", "E0314", "E0420", "E0501",
];
