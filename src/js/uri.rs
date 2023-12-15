// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! `encodeURI` and `decodeURI`, which diff-match-patch uses to escape the text of its patches.

use super::error::JsError;
use super::string::JsString;

/// The characters `encodeURI` leaves alone besides ASCII letters and digits.
const URI_UNESCAPED: &[u8] = b";,/?:@&=+$-_.!~*'()#";
/// The characters whose escapes `decodeURI` keeps as they are.
const URI_RESERVED: &[u8] = b";/?:@&=+$,#";

fn uri_malformed() -> JsError {
    JsError::new("URIError", "URI malformed")
}

/// `encodeURI(text)`: every code point outside the unescaped set as `%XX` escapes of its UTF-8 bytes. An unpaired
/// surrogate is a `URIError`.
pub fn encode_uri(text: &[u16]) -> Result<JsString, JsError> {
    let mut out = JsString::new();
    let mut i = 0;
    while i < text.len() {
        let unit = text[i];
        if unit < 0x80 && (char::from(unit as u8).is_ascii_alphanumeric() || URI_UNESCAPED.contains(&(unit as u8))) {
            out.push_units(&[unit]);
            i += 1;
            continue;
        }
        let code_point = if (0xD800..0xDC00).contains(&unit) {
            match text.get(i + 1) {
                Some(&low) if (0xDC00..0xE000).contains(&low) => {
                    i += 1;
                    0x10000 + ((u32::from(unit) - 0xD800) << 10) + (u32::from(low) - 0xDC00)
                }
                _ => return Err(uri_malformed()),
            }
        } else if (0xDC00..0xE000).contains(&unit) {
            return Err(uri_malformed());
        } else {
            u32::from(unit)
        };
        let ch = char::from_u32(code_point).ok_or_else(uri_malformed)?;
        let mut buf = [0u8; 4];
        for byte in ch.encode_utf8(&mut buf).bytes() {
            out.push_str(&format!("%{byte:02X}"));
        }
        i += 1;
    }
    Ok(out)
}

fn hex_value(unit: u16) -> Option<u8> {
    char::from_u32(u32::from(unit))
        .and_then(|c| c.to_digit(16))
        .map(|d| d as u8)
}

/// The byte of the `%XX` escape at `at`, if there is one.
fn escaped_byte(text: &[u16], at: usize) -> Option<u8> {
    if text.get(at) != Some(&u16::from(b'%')) {
        return None;
    }
    let high = hex_value(*text.get(at + 1)?)?;
    let low = hex_value(*text.get(at + 2)?)?;
    Some(high << 4 | low)
}

/// `decodeURI(text)`: `%XX` escapes of UTF-8 sequences decoded, except escapes of reserved characters and `#`,
/// which stay as written. A malformed escape or byte sequence is a `URIError`.
pub fn decode_uri(text: &[u16]) -> Result<JsString, JsError> {
    let mut out = JsString::new();
    let mut i = 0;
    while i < text.len() {
        if text[i] != u16::from(b'%') {
            out.push_units(&[text[i]]);
            i += 1;
            continue;
        }
        let start = i;
        let first = escaped_byte(text, i).ok_or_else(uri_malformed)?;
        i += 3;
        if first < 0x80 {
            if URI_RESERVED.contains(&first) {
                out.push_units(&text[start..i]);
            } else {
                out.push_units(&[u16::from(first)]);
            }
            continue;
        }
        let (count, min, initial) = match first {
            0xC0..=0xDF => (1, 0x80, u32::from(first & 0x1F)),
            0xE0..=0xEF => (2, 0x800, u32::from(first & 0x0F)),
            0xF0..=0xF7 => (3, 0x10000, u32::from(first & 0x07)),
            _ => return Err(uri_malformed()),
        };
        let mut code_point = initial;
        for _ in 0..count {
            let byte = escaped_byte(text, i).ok_or_else(uri_malformed)?;
            if byte & 0xC0 != 0x80 {
                return Err(uri_malformed());
            }
            code_point = code_point << 6 | u32::from(byte & 0x3F);
            i += 3;
        }
        if code_point < min || (0xD800..0xE000).contains(&code_point) || code_point > 0x10FFFF {
            return Err(uri_malformed());
        }
        let ch = char::from_u32(code_point).ok_or_else(uri_malformed)?;
        let mut buf = [0u16; 2];
        out.push_units(ch.encode_utf16(&mut buf));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn encodes_like_javascript() {
        let encoded = encode_uri(&units("a b%é😀#")).map(|s| s.to_string_lossy());
        assert_eq!(encoded.ok().as_deref(), Some("a%20b%25%C3%A9%F0%9F%98%80#"));
        assert!(encode_uri(&[0xD83D]).is_err());
    }

    #[test]
    fn decodes_like_javascript() {
        let decoded = decode_uri(&units("a%20b%23%3B%C3%A9")).map(|s| s.to_string_lossy());
        assert_eq!(decoded.ok().as_deref(), Some("a b%23%3Bé"));
        assert!(decode_uri(&units("%E0%A4%A")).is_err());
        assert!(decode_uri(&units("%C0%80")).is_err());
        assert!(decode_uri(&units("%ED%A0%80")).is_err());
    }
}
