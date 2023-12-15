// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! JavaScript number conversions: `Number.prototype.toString()`, `parseInt` and `ToNumber`.

use super::string::{is_js_whitespace, JsString};

/// `Number::toString(x)` in radix 10: the shortest decimal that round-trips, in fixed notation for exponents from
/// -7 to 20 and in exponential notation (`1e+21`, `1.5e-7`) otherwise. Negative zero is `"0"`.
pub fn number_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let sign = if value < 0.0 { "-" } else { "" };
    let (digits, exponent) = shortest_digits(value.abs());
    let k = digits.len() as i32;
    // The decimal point sits after `n` digits: value = 0.digits * 10^n.
    let n = exponent + 1;
    let body = if k <= n && n <= 21 {
        let mut s = digits.clone();
        s.extend(std::iter::repeat_n('0', (n - k) as usize));
        s
    } else if 0 < n && n <= 21 {
        let (int, frac) = digits.split_at(n as usize);
        format!("{int}.{frac}")
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let exp = if e >= 0 { format!("e+{e}") } else { format!("e-{}", -e) };
        if k == 1 {
            format!("{digits}{exp}")
        } else {
            let (first, rest) = digits.split_at(1);
            format!("{first}.{rest}{exp}")
        }
    };
    format!("{sign}{body}")
}

/// The shortest round-trip decimal digits of a positive finite number and the exponent of its first digit. When the
/// number lies exactly halfway between two shortest candidates, the even one is chosen, as the specification of
/// `Number::toString` requires.
fn shortest_digits(value: f64) -> (String, i32) {
    let (digits, exponent) = split_exponential(&format!("{value:e}"));
    match even_tie_digits(value, &digits, exponent) {
        Some(even) => (even, exponent),
        None => (digits, exponent),
    }
}

/// The significant digits (trailing zeros removed) and the exponent of a number formatted with `{:e}`.
fn split_exponential(formatted: &str) -> (String, i32) {
    let (mantissa, exponent) = formatted.split_once('e').unwrap_or((formatted, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    (digits.to_string(), exponent.parse().unwrap_or(0))
}

/// The even neighbour of `digits` when `value` is exactly the midpoint between `digits` and that neighbour and the
/// neighbour round-trips too. Ties need more decimal digits than a double has bits of spacing, so only long digit
/// strings are checked.
fn even_tie_digits(value: f64, digits: &str, exponent: i32) -> Option<String> {
    let last = digits.bytes().last()? - b'0';
    if digits.len() < 15 || last.is_multiple_of(2) {
        return None;
    }
    // Every finite double has a terminating decimal expansion of at most 767 significant digits.
    let (exact, exact_exponent) = split_exponential(&format!("{value:.1100e}"));
    if exact_exponent != exponent || exact.len() != digits.len() + 1 || !exact.ends_with('5') {
        return None;
    }
    let low = &exact[..digits.len()];
    let candidate = if low == digits {
        increment(low)?
    } else {
        low.to_string()
    };
    if candidate.len() != digits.len() || !(candidate.as_bytes()[candidate.len() - 1] - b'0').is_multiple_of(2) {
        return None;
    }
    let text = format!("{}.{}e{exponent}", &candidate[..1], &candidate[1..]);
    (text.parse::<f64>() == Ok(value)).then_some(candidate)
}

/// A decimal digit string plus one in its last place, or `None` when that carries into a new digit.
fn increment(digits: &str) -> Option<String> {
    let mut bytes = digits.as_bytes().to_vec();
    for byte in bytes.iter_mut().rev() {
        if *byte == b'9' {
            *byte = b'0';
        } else {
            *byte += 1;
            return String::from_utf8(bytes).ok();
        }
    }
    None
}

/// `parseInt(string, 10)`: leading white space and an optional sign, then the longest run of decimal digits;
/// `NaN` when there is none.
pub fn parse_int(text: &JsString) -> f64 {
    let units = text.units();
    let mut i = 0;
    while i < units.len() && is_js_whitespace(units[i]) {
        i += 1;
    }
    let mut negative = false;
    if i < units.len() && (units[i] == u16::from(b'+') || units[i] == u16::from(b'-')) {
        negative = units[i] == u16::from(b'-');
        i += 1;
    }
    let start = i;
    while i < units.len() && (u16::from(b'0')..=u16::from(b'9')).contains(&units[i]) {
        i += 1;
    }
    if start == i {
        return f64::NAN;
    }
    let digits: String = units[start..i].iter().map(|&u| char::from(u as u8)).collect();
    let magnitude: f64 = digits.parse().unwrap_or(f64::NAN);
    if negative {
        -magnitude
    } else {
        magnitude
    }
}

/// `ToNumber` of a string: white space around a decimal literal, `Infinity`, or a `0x`, `0o` or `0b` integer;
/// the empty string is 0 and anything else `NaN`.
pub fn string_to_number(text: &JsString) -> f64 {
    let units = text.units();
    let start = units.iter().position(|&u| !is_js_whitespace(u));
    let Some(start) = start else {
        return 0.0;
    };
    let end = units
        .iter()
        .rposition(|&u| !is_js_whitespace(u))
        .map_or(start, |e| e + 1);
    let body = &units[start..end];
    if body.iter().any(|&u| u > 0x7F) {
        return f64::NAN;
    }
    let body: String = body.iter().map(|&u| char::from(u as u8)).collect();
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(digits) = body.strip_prefix(prefix) {
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                return f64::NAN;
            }
            return digits.chars().fold(0.0, |acc, c| {
                acc * f64::from(radix) + f64::from(c.to_digit(radix).unwrap_or(0))
            });
        }
    }
    let (sign, unsigned) = match body.as_bytes().first() {
        Some(b'-') => (-1.0, &body[1..]),
        Some(b'+') => (1.0, &body[1..]),
        _ => (1.0, body.as_str()),
    };
    if unsigned == "Infinity" {
        return sign * f64::INFINITY;
    }
    if !is_decimal_literal(unsigned) {
        return f64::NAN;
    }
    unsigned.parse::<f64>().map_or(f64::NAN, |v| sign * v)
}

/// Whether text is a StrUnsignedDecimalLiteral: digits with an optional fraction and exponent.
fn is_decimal_literal(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    let int_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i - int_start;
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        digits += i - frac_start;
    }
    if digits == 0 {
        return false;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        let exp_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_start {
            return false;
        }
    }
    i == bytes.len()
}

/// `ToIntegerOrInfinity`.
pub fn to_integer_or_infinity(value: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else {
        value.trunc() + 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_javascript() {
        let cases = [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.5, "-1.5"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (123_456_789_012_345_680_000.0, "123456789012345680000"),
            (0.000001, "0.000001"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (5e-324, "5e-324"),
            (1.797_693_134_862_315_7e308, "1.7976931348623157e+308"),
            (0.1 + 0.2, "0.30000000000000004"),
            (9_007_199_254_740_992.0, "9007199254740992"),
            (982_449_082_450_741.2, "982449082450741.2"),
            (982_449_082_450_741.4, "982449082450741.4"),
        ];
        for (value, expected) in cases {
            assert_eq!(number_to_string(value), expected, "{value:e}");
        }
    }

    #[test]
    fn parses_integers_like_parse_int() {
        assert_eq!(parse_int(&JsString::from(" 12")), 12.0);
        assert_eq!(parse_int(&JsString::from("1e3")), 1.0);
        assert_eq!(parse_int(&JsString::from("-5")), -5.0);
        assert_eq!(parse_int(&JsString::from("\u{a0}7x")), 7.0);
        assert!(parse_int(&JsString::from("")).is_nan());
        assert!(parse_int(&JsString::from("_3")).is_nan());
    }

    #[test]
    fn converts_strings_to_numbers() {
        assert_eq!(string_to_number(&JsString::from(" 10 ")), 10.0);
        assert_eq!(string_to_number(&JsString::from("")), 0.0);
        assert_eq!(string_to_number(&JsString::from("0x10")), 16.0);
        assert_eq!(string_to_number(&JsString::from("-Infinity")), f64::NEG_INFINITY);
        assert!(string_to_number(&JsString::from("1.2.3")).is_nan());
        assert!(string_to_number(&JsString::from(".")).is_nan());
        assert_eq!(string_to_number(&JsString::from(".5e1")), 5.0);
    }
}
