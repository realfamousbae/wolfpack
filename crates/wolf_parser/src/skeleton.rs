//! Checks of the file skeleton that need the whole file (spec chapter 2): section order,
//! duplicate sections, and the target blocks of `extern`.

use std::collections::HashMap;

use wolf_ast::{File, ItemKind, SectionKind};
use wolf_diag::{Diagnostic, codes};
use wolf_span::{FileId, Span};

pub(crate) fn check(_file: FileId, tree: &File) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut first_seen: HashMap<SectionKind, Span> = HashMap::new();
    // The latest section so far, in the required order, with its name span.
    let mut furthest: Option<(SectionKind, Span)> = None;
    for section in &tree.sections {
        if let Some(first) = first_seen.get(&section.kind) {
            out.push(
                Diagnostic::new(
                    codes::E0203,
                    format!("duplicate section `{}`", section.kind.name()),
                )
                .primary(section.name_span, "second section with this name")
                .secondary(*first, "first section")
                .help(format!(
                    "merge the entries into the first `{}` section",
                    section.kind.name()
                )),
            );
            continue;
        }
        first_seen.insert(section.kind, section.name_span);
        match furthest {
            Some((later, later_span)) if later > section.kind => {
                // Find the first section written so far that must come after this one.
                let before = tree
                    .sections
                    .iter()
                    .find(|s| s.kind > section.kind)
                    .map(|s| (s.kind, s.name_span))
                    .unwrap_or((later, later_span));
                out.push(
                    Diagnostic::new(
                        codes::E0202,
                        format!("section `{}` is out of order", section.kind.name()),
                    )
                    .primary(section.name_span, "this section comes too late")
                    .secondary(before.1, format!("`{}` must come after it", before.0.name()))
                    .help(format!(
                        "move `{}` before `{}`",
                        section.kind.name(),
                        before.0.name()
                    ))
                    .note(
                        "the order is: imports, extern, config, types, interfaces, impl, functions, tests",
                    ),
                );
            }
            _ => furthest = Some((section.kind, section.name_span)),
        }
        if section.kind == SectionKind::Extern {
            check_extern(section, &mut out);
        }
    }
    out
}

fn check_extern(section: &wolf_ast::Section, out: &mut Vec<Diagnostic>) {
    let mut seen: HashMap<String, Span> = HashMap::new();
    for item in &section.items {
        let ItemKind::ExternTarget(target) = &item.kind else {
            continue;
        };
        let name = target.target.name.as_str();
        if !matches!(name, "ts" | "go" | "rust") {
            out.push(
                Diagnostic::new(codes::E0209, format!("unknown extern target `{name}`"))
                    .primary(target.target.span, "not a target")
                    .help("targets are `ts`, `go` and `rust`"),
            );
        } else if let Some(first) = seen.get(name) {
            out.push(
                Diagnostic::new(codes::E0209, format!("duplicate extern block `{name}`"))
                    .primary(target.target.span, "second block for this target")
                    .secondary(*first, "first block"),
            );
        } else {
            seen.insert(name.to_string(), target.target.span);
        }
        if target.decls.is_empty() {
            out.push(
                Diagnostic::new(codes::E0204, format!("empty extern block `{name}`"))
                    .primary(target.target.span, "this block has no declarations")
                    .help("remove the block"),
            );
        }
    }
}
