//! Text of std: the canonical `Float` format, parsing of numbers and the string functions. These
//! follow the runtime of the Rust target (`wolf_rs::runtime`) exactly, so that the VM prints
//! what the targets print.

/// `Float.to_string`: the canonical format of spec chapter 7, section 4.1, which is the format
/// of JavaScript with `inf` and a `.0` for whole numbers.
pub fn float_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0" } else { "0.0" }.to_string();
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let e = format!("{:e}", x.abs());
    let (mantissa, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let digits = mantissa.replace('.', "");
    let n = exp.parse::<i64>().unwrap_or(0) + 1;
    let k = i64::try_from(digits.len()).unwrap_or(0);
    let zeros = |count: i64| "0".repeat(usize::try_from(count).unwrap_or(0));
    let text = if k <= n && n <= 21 {
        format!("{digits}{}.0", zeros(n - k))
    } else if 0 < n && n <= 21 {
        let (a, b) = split(&digits, usize::try_from(n).unwrap_or(0));
        format!("{a}.{b}")
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", zeros(-n))
    } else {
        let (a, b) = split(&digits, 1);
        if b.is_empty() {
            format!("{a}e{}", n - 1)
        } else {
            format!("{a}.{b}e{}", n - 1)
        }
    };
    format!("{sign}{text}")
}

fn split(s: &str, at: usize) -> (&str, &str) {
    if s.is_char_boundary(at) && at <= s.len() {
        s.split_at(at)
    } else {
        (s, "")
    }
}

fn digits(text: &str, underscores: bool) -> Option<&str> {
    let bytes = text.as_bytes();
    if !bytes.first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let mut i = 0;
    while let Some(b) = bytes.get(i) {
        if b.is_ascii_digit() {
            i += 1;
        } else if underscores && *b == b'_' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
            i += 2;
        } else {
            break;
        }
    }
    text.get(i..)
}

/// `String.parse_int`: an optional `-` and decimal digits.
pub fn parse_int(text: &str) -> Option<i64> {
    match digits(text.strip_prefix('-').unwrap_or(text), false) {
        Some("") => text.parse::<i64>().ok(),
        _ => None,
    }
}

/// `String.parse_float`: the float literal syntax with an optional `-`, `NaN` or `inf`.
pub fn parse_float(text: &str) -> Option<f64> {
    match text {
        "NaN" => return Some(f64::NAN),
        "inf" => return Some(f64::INFINITY),
        "-inf" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    let mut rest = digits(text.strip_prefix('-').unwrap_or(text), true);
    if let Some(r) = rest.and_then(|r| r.strip_prefix('.')) {
        rest = digits(r, true);
    }
    if let Some(r) = rest.and_then(|r| r.strip_prefix(['e', 'E'])) {
        rest = digits(r.strip_prefix(['+', '-']).unwrap_or(r), true);
    }
    match rest {
        Some("") => text.replace('_', "").parse::<f64>().ok(),
        _ => None,
    }
}

/// `Float.to_int`; `None` with the panic message.
pub fn float_to_int(x: f64) -> Result<i64, &'static str> {
    if !x.is_finite() {
        return Err("float to int conversion of a non-finite value");
    }
    let t = x.trunc();
    if !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&t) {
        return Err("integer overflow");
    }
    // The range check above makes the conversion exact.
    #[allow(clippy::cast_possible_truncation)]
    Ok(t as i64)
}

/// The length of a string in Unicode scalar values.
pub fn len(text: &str) -> i64 {
    i64::try_from(text.chars().count()).unwrap_or(i64::MAX)
}

/// `String.find`: the position of the first occurrence, in scalar values.
pub fn find(text: &str, part: &str) -> Option<i64> {
    text.find(part).map(|i| len(text.get(..i).unwrap_or("")))
}

/// `String.trim`: ASCII whitespace at both ends is removed.
pub fn trim(text: &str) -> &str {
    text.trim_matches([' ', '\t', '\n', '\r', '\x0c', '\x0b'])
}

/// The scalar values `start..end` of a string; `None` if the bounds are out of range.
pub fn slice(text: &str, start: i64, end: i64) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let (a, b) = bounds(start, end, chars.len())?;
    chars.get(a..b).map(|c| c.iter().collect())
}

/// Checked bounds of a slice.
pub fn bounds(start: i64, end: i64, length: usize) -> Option<(usize, usize)> {
    match (usize::try_from(start), usize::try_from(end)) {
        (Ok(a), Ok(b)) if a <= b && b <= length => Some((a, b)),
        _ => None,
    }
}

/// `x` with `digits` digits after the point, rounded to the nearest with ties to even on its
/// exact value; `NaN` and the infinities as without a precision (spec chapter 7, section 4.4).
pub fn fixed(x: f64, digits: usize) -> String {
    if x.is_finite() {
        format!("{x:.digits$}")
    } else {
        float_to_string(x)
    }
}

/// `text` padded with `fill` to `width` Unicode scalar values: `align` 0 pads on the right, 1 on
/// both sides with the extra character on the right, 2 on the left. `zero` pads a number with
/// zeros after its sign, and `NaN` and the infinities with spaces on the left.
pub fn pad(text: &str, width: usize, fill: char, align: i64, zero: bool) -> String {
    let n = text.chars().count();
    if n >= width {
        return text.to_string();
    }
    let missing = width - n;
    let repeat = |c: char, k: usize| std::iter::repeat_n(c, k).collect::<String>();
    if zero {
        if matches!(text, "NaN" | "inf" | "-inf") {
            return format!("{}{text}", repeat(' ', missing));
        }
        let (sign, digits) = match text.strip_prefix('-') {
            Some(rest) => ("-", rest),
            None => ("", text),
        };
        return format!("{sign}{}{digits}", repeat('0', missing));
    }
    match align {
        0 => format!("{text}{}", repeat(fill, missing)),
        1 => {
            let left = missing / 2;
            format!(
                "{}{text}{}",
                repeat(fill, left),
                repeat(fill, missing - left)
            )
        }
        _ => format!("{}{text}", repeat(fill, missing)),
    }
}

/// A `String` inside a collection: in quotes, with `\"`, `\\`, `\n`, `\r`, `\t`, `\0` and the
/// other control characters as `\u{1b}` (spec chapter 7, section 4.1, rule 1).
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if c.is_ascii_control() => out.push_str(&format!("\\u{{{:x}}}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_print_like_the_targets() {
        assert_eq!(float_to_string(1.0), "1.0");
        assert_eq!(float_to_string(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(float_to_string(1e21), "1e21");
        assert_eq!(float_to_string(1.5e-7), "1.5e-7");
        assert_eq!(float_to_string(-0.0), "-0.0");
        assert_eq!(float_to_string(f64::INFINITY), "inf");
        assert_eq!(float_to_string(123456.789), "123456.789");
    }

    #[test]
    fn numbers_parse_like_the_targets() {
        assert_eq!(parse_int("-42"), Some(-42));
        assert_eq!(parse_int("+1"), None);
        assert_eq!(parse_int("1_000"), None);
        assert_eq!(parse_float("1_000.5"), Some(1000.5));
        assert_eq!(parse_float(".5"), None);
        assert_eq!(float_to_int(2.9), Ok(2));
    }
}
