//! Slugs for URLs: lower case letters and digits, with one dash between words.

/// The slug of a title.
pub fn slugify(title: &str) -> String {
    title
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

/// The number of runs of letters and digits in a title.
pub fn count_words(title: &str) -> usize {
    title
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .count()
}
