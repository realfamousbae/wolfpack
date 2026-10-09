//! Naming style (spec chapter 1, section 3) and "did you mean" suggestions.

/// `[a-z][a-z0-9]*(_[a-z0-9]+)*`
pub fn is_snake_case(name: &str) -> bool {
    wolf_project::is_snake_case(name)
}

/// `[A-Z][A-Za-z0-9]*`
pub fn is_pascal_case(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase())
        && name.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Converts a name to `snake_case`, for hints.
pub(crate) fn to_snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 && !out.ends_with('_') {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else if c == '_' || c == '-' {
            if !out.is_empty() && !out.ends_with('_') {
                out.push('_');
            }
        } else {
            out.push(c);
        }
    }
    out.trim_matches('_').to_string()
}

/// Converts a name to `PascalCase`, for hints.
pub(crate) fn to_pascal(name: &str) -> String {
    name.split(['_', '-'])
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut chars = p.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// The closest candidate to `name`, if it is close enough. Ties go to the alphabetically first.
/// The candidate closest to `name`, if one is close enough to be a likely typo.
pub fn suggest<'a>(name: &str, candidates: impl Iterator<Item = &'a str>) -> Option<String> {
    let limit = (name.chars().count() / 3).max(1);
    let mut best: Option<(usize, &str)> = None;
    for candidate in candidates {
        if candidate == name {
            continue;
        }
        let d = distance(name, candidate);
        if d > limit {
            continue;
        }
        best = match best {
            Some((bd, bc)) if (bd, bc) <= (d, candidate) => Some((bd, bc)),
            _ => Some((d, candidate)),
        };
    }
    best.map(|(_, c)| c.to_string())
}

/// Levenshtein distance on characters.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row.first().copied().unwrap_or(0);
        if let Some(first) = row.first_mut() {
            *first = i + 1;
        }
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            let above = row.get(j + 1).copied().unwrap_or(0);
            let left = row.get(j).copied().unwrap_or(0);
            let value = (above + 1).min(left + 1).min(prev + cost);
            prev = above;
            if let Some(cell) = row.get_mut(j + 1) {
                *cell = value;
            }
        }
    }
    row.last().copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles() {
        assert!(is_pascal_case("FileStorage"));
        assert!(!is_pascal_case("fileStorage"));
        assert!(!is_pascal_case("File_Storage"));
        assert_eq!(to_snake("findUser"), "find_user");
        assert_eq!(to_snake("FileStorage"), "file_storage");
        assert_eq!(to_pascal("file_storage"), "FileStorage");
    }

    #[test]
    fn suggestions() {
        let names = ["print", "panic", "price"];
        assert_eq!(
            suggest("prnt", names.iter().copied()).as_deref(),
            Some("print")
        );
        assert_eq!(suggest("zzz", names.iter().copied()), None);
    }
}
