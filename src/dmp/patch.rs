// SPDX-License-Identifier: Apache-2.0
// Copyright 2018 The diff-match-patch Authors. https://github.com/google/diff-match-patch
// Ported to Rust from the JavaScript diff-match-patch 1.0.5 for the jsondiffpatch port.

//! Patches: making them from two texts, their GNU-diff-like text form, and applying them fuzzily.

use super::{Diff, DiffMatchPatch, Op};
use crate::js::number::parse_int;
use crate::js::string::{index_of, last_index_of, substring};
use crate::js::uri::{decode_uri, encode_uri};
use crate::js::{JsError, JsString};

/// One patch: the diffs with their context, and where they apply in the old (1) and new (2) text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Patch {
    /// The diffs of this patch, context included.
    pub diffs: Vec<Diff>,
    /// Start in the old text.
    pub start1: i64,
    /// Start in the new text.
    pub start2: i64,
    /// Length in the old text.
    pub length1: i64,
    /// Length in the new text.
    pub length2: i64,
}

fn js_substring(text: &[u16], start: i64, end: i64) -> Vec<u16> {
    substring(text, start as f64, end as f64).to_vec()
}

fn coords(start: i64, length: i64) -> String {
    match length {
        0 => format!("{start},0"),
        1 => format!("{}", start + 1),
        _ => format!("{},{length}", start + 1),
    }
}

impl Patch {
    /// The patch in GNU diff form, its texts escaped with `encodeURI` (a `URIError` for unpaired surrogates).
    pub fn to_text(&self) -> Result<JsString, JsError> {
        let mut text = JsString::from(format!(
            "@@ -{} +{} @@\n",
            coords(self.start1, self.length1),
            coords(self.start2, self.length2)
        ));
        for (op, data) in &self.diffs {
            text.push_str(match op {
                Op::Insert => "+",
                Op::Delete => "-",
                Op::Equal => " ",
            });
            text.push_js(&encode_uri(data)?);
            text.push_str("\n");
        }
        Ok(text.replace_all("%20", " "))
    }
}

/// Parses `@@ -A[,B] +C[,D] @@` as diff-match-patch's header expression does: the four captures, or `None`.
fn parse_header(line: &[u16]) -> Option<[Vec<u16>; 4]> {
    let mut i = 0;
    let expect = |i: &mut usize, text: &str| -> bool {
        let units: Vec<u16> = text.encode_utf16().collect();
        if line.len() >= *i + units.len() && line[*i..*i + units.len()] == units[..] {
            *i += units.len();
            true
        } else {
            false
        }
    };
    let digits = |i: &mut usize| -> Vec<u16> {
        let start = *i;
        while *i < line.len() && (0x30..=0x39).contains(&line[*i]) {
            *i += 1;
        }
        line[start..*i].to_vec()
    };
    if !expect(&mut i, "@@ -") {
        return None;
    }
    let a = digits(&mut i);
    if a.is_empty() {
        return None;
    }
    expect(&mut i, ",");
    let b = digits(&mut i);
    if !expect(&mut i, " +") {
        return None;
    }
    let c = digits(&mut i);
    if c.is_empty() {
        return None;
    }
    expect(&mut i, ",");
    let d = digits(&mut i);
    if !expect(&mut i, " @@") || i != line.len() {
        return None;
    }
    Some([a, b, c, d])
}

/// A patch-header coordinate. The original reads it as a JavaScript number, so it is capped at
/// `Number.MAX_SAFE_INTEGER`: whatever a request puts in a header, the padding, context and split
/// arithmetic on these `i64` coordinates then stays far from overflow (a NaN still becomes 0).
fn header_number(digits: &[u16]) -> i64 {
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    let value = parse_int(&JsString::from_units(digits.to_vec()));
    (if value > MAX_SAFE_INTEGER {
        MAX_SAFE_INTEGER
    } else {
        value
    }) as i64
}

impl DiffMatchPatch {
    /// `patch_make(text1, text2)`.
    pub fn patch_make(&self, text1: &[u16], text2: &[u16]) -> Result<Vec<Patch>, JsError> {
        let mut diffs = self.diff_main(text1, text2, true)?;
        if diffs.len() > 2 {
            Self::cleanup_semantic(&mut diffs);
            self.cleanup_efficiency(&mut diffs);
        }
        if diffs.is_empty() {
            return Ok(Vec::new());
        }
        let mut patches = Vec::new();
        let mut patch = Patch::default();
        let (mut char_count1, mut char_count2) = (0i64, 0i64);
        let mut prepatch_text: Vec<u16> = text1.to_vec();
        let mut postpatch_text: Vec<u16> = text1.to_vec();
        let margin = self.patch_margin as i64;
        let total = diffs.len();
        for (x, (diff_type, diff_text)) in diffs.into_iter().enumerate() {
            let length = diff_text.len() as i64;
            if patch.diffs.is_empty() && diff_type != Op::Equal {
                patch.start1 = char_count1;
                patch.start2 = char_count2;
            }
            match diff_type {
                Op::Insert => {
                    patch.length2 += length;
                    let at = char_count2 as usize;
                    postpatch_text.splice(at..at, diff_text.iter().copied());
                    patch.diffs.push((diff_type, diff_text));
                }
                Op::Delete => {
                    patch.length1 += length;
                    let at = char_count2 as usize;
                    let end = (at + diff_text.len()).min(postpatch_text.len());
                    postpatch_text.drain(at..end);
                    patch.diffs.push((diff_type, diff_text));
                }
                Op::Equal => {
                    if length <= 2 * margin && !patch.diffs.is_empty() && total != x + 1 {
                        patch.length1 += length;
                        patch.length2 += length;
                        patch.diffs.push((diff_type, diff_text));
                    } else if length >= 2 * margin && !patch.diffs.is_empty() {
                        self.patch_add_context(&mut patch, &prepatch_text);
                        patches.push(std::mem::take(&mut patch));
                        prepatch_text = postpatch_text.clone();
                        char_count1 = char_count2;
                    }
                }
            }
            if diff_type != Op::Insert {
                char_count1 += length;
            }
            if diff_type != Op::Delete {
                char_count2 += length;
            }
        }
        if !patch.diffs.is_empty() {
            self.patch_add_context(&mut patch, &prepatch_text);
            patches.push(patch);
        }
        Ok(patches)
    }

    fn patch_add_context(&self, patch: &mut Patch, text: &[u16]) {
        if text.is_empty() {
            return;
        }
        let margin = self.patch_margin as i64;
        let mut pattern = js_substring(text, patch.start2, patch.start2 + patch.length1);
        let mut padding = 0i64;
        while index_of(text, &pattern, 0) != last_index_of(text, &pattern, text.len())
            && (pattern.len() as i64) < self.match_max_bits as i64 - 2 * margin
        {
            padding += margin;
            pattern = js_substring(text, patch.start2 - padding, patch.start2 + patch.length1 + padding);
        }
        padding += margin;
        let prefix = js_substring(text, patch.start2 - padding, patch.start2);
        if !prefix.is_empty() {
            patch.diffs.insert(0, (Op::Equal, prefix.clone()));
        }
        let suffix = js_substring(
            text,
            patch.start2 + patch.length1,
            patch.start2 + patch.length1 + padding,
        );
        if !suffix.is_empty() {
            patch.diffs.push((Op::Equal, suffix.clone()));
        }
        patch.start1 -= prefix.len() as i64;
        patch.start2 -= prefix.len() as i64;
        patch.length1 += (prefix.len() + suffix.len()) as i64;
        patch.length2 += (prefix.len() + suffix.len()) as i64;
    }

    /// `patch_toText`.
    pub fn patch_to_text(patches: &[Patch]) -> Result<JsString, JsError> {
        let mut text = JsString::new();
        for patch in patches {
            text.push_js(&patch.to_text()?);
        }
        Ok(text)
    }

    /// `patch_fromText` of a string.
    pub fn patch_from_text(textline: &JsString) -> Result<Vec<Patch>, JsError> {
        let mut patches = Vec::new();
        if textline.is_empty() {
            return Ok(patches);
        }
        let text = textline.split_unit(0x0A);
        let mut pointer = 0;
        let mut line: Option<JsString> = None;
        while pointer < text.len() {
            let Some([a, b, c, d]) = parse_header(text[pointer].units()) else {
                let mut message = JsString::from("Invalid patch string: ");
                message.push_js(&text[pointer]);
                return Err(JsError::error(message));
            };
            let mut patch = Patch {
                start1: header_number(&a),
                ..Patch::default()
            };
            if b.is_empty() {
                patch.start1 -= 1;
                patch.length1 = 1;
            } else if b == [0x30] {
                patch.length1 = 0;
            } else {
                patch.start1 -= 1;
                patch.length1 = header_number(&b);
            }
            patch.start2 = header_number(&c);
            if d.is_empty() {
                patch.start2 -= 1;
                patch.length2 = 1;
            } else if d == [0x30] {
                patch.length2 = 0;
            } else {
                patch.start2 -= 1;
                patch.length2 = header_number(&d);
            }
            pointer += 1;

            while pointer < text.len() {
                let current = text[pointer].units();
                let sign = current.first().copied();
                match decode_uri(&current[current.len().min(1)..]) {
                    Ok(decoded) => line = Some(decoded),
                    Err(_) => {
                        let mut message = JsString::from("Illegal escape in patch_fromText: ");
                        match &line {
                            Some(previous) => message.push_js(previous),
                            None => message.push_str("undefined"),
                        }
                        return Err(JsError::error(message));
                    }
                }
                let decoded = line.clone().unwrap_or_default().into_units();
                match sign {
                    Some(0x2D) => patch.diffs.push((Op::Delete, decoded)),
                    Some(0x2B) => patch.diffs.push((Op::Insert, decoded)),
                    Some(0x20) => patch.diffs.push((Op::Equal, decoded)),
                    Some(0x40) => break,
                    None => {}
                    Some(other) => {
                        let mut message = JsString::from("Invalid patch mode \"");
                        message.push_units(&[other]);
                        message.push_str("\" in: ");
                        message.push_units(&decoded);
                        return Err(JsError::error(message));
                    }
                }
                pointer += 1;
            }
            patches.push(patch);
        }
        Ok(patches)
    }

    /// `patch_addPadding`: pads the patches for matching at the edges; returns the padding.
    fn patch_add_padding(&self, patches: &mut [Patch]) -> Vec<u16> {
        let padding_length = self.patch_margin;
        let null_padding: Vec<u16> = (1..=padding_length as u16).collect();
        let pad = padding_length as i64;
        for patch in patches.iter_mut() {
            patch.start1 += pad;
            patch.start2 += pad;
        }
        let Some(first) = patches.first_mut() else {
            return null_padding;
        };
        if first.diffs.first().is_none_or(|d| d.0 != Op::Equal) {
            first.diffs.insert(0, (Op::Equal, null_padding.clone()));
            first.start1 -= pad;
            first.start2 -= pad;
            first.length1 += pad;
            first.length2 += pad;
        } else if padding_length > first.diffs[0].1.len() {
            let current = first.diffs[0].1.len();
            let extra = (padding_length - current) as i64;
            let mut grown = null_padding[current..].to_vec();
            grown.extend_from_slice(&first.diffs[0].1);
            first.diffs[0].1 = grown;
            first.start1 -= extra;
            first.start2 -= extra;
            first.length1 += extra;
            first.length2 += extra;
        }
        let Some(last) = patches.last_mut() else {
            return null_padding;
        };
        if last.diffs.last().is_none_or(|d| d.0 != Op::Equal) {
            last.diffs.push((Op::Equal, null_padding.clone()));
            last.length1 += pad;
            last.length2 += pad;
        } else {
            let n = last.diffs.len();
            let current = last.diffs[n - 1].1.len();
            if padding_length > current {
                let extra = padding_length - current;
                last.diffs[n - 1].1.extend_from_slice(&null_padding[..extra]);
                last.length1 += extra as i64;
                last.length2 += extra as i64;
            }
        }
        null_padding
    }

    /// `patch_splitMax`: splits patches longer than the match algorithm can handle.
    fn patch_split_max(&self, patches: &mut Vec<Patch>) {
        let patch_size = self.match_max_bits as i64;
        let margin = self.patch_margin as i64;
        let mut x: isize = 0;
        while (x as usize) < patches.len() {
            if patches[x as usize].length1 <= patch_size {
                x += 1;
                continue;
            }
            let mut bigpatch = patches.remove(x as usize);
            x -= 1;
            let mut start1 = bigpatch.start1;
            let mut start2 = bigpatch.start2;
            let mut precontext: Vec<u16> = Vec::new();
            while !bigpatch.diffs.is_empty() {
                let mut patch = Patch::default();
                let mut empty = true;
                patch.start1 = start1 - precontext.len() as i64;
                patch.start2 = start2 - precontext.len() as i64;
                if !precontext.is_empty() {
                    patch.length1 = precontext.len() as i64;
                    patch.length2 = precontext.len() as i64;
                    patch.diffs.push((Op::Equal, precontext.clone()));
                }
                while !bigpatch.diffs.is_empty() && patch.length1 < patch_size - margin {
                    let diff_type = bigpatch.diffs[0].0;
                    let diff_length = bigpatch.diffs[0].1.len() as i64;
                    if diff_type == Op::Insert {
                        patch.length2 += diff_length;
                        start2 += diff_length;
                        patch.diffs.push(bigpatch.diffs.remove(0));
                        empty = false;
                    } else if diff_type == Op::Delete
                        && patch.diffs.len() == 1
                        && patch.diffs[0].0 == Op::Equal
                        && diff_length > 2 * patch_size
                    {
                        patch.length1 += diff_length;
                        start1 += diff_length;
                        empty = false;
                        patch.diffs.push(bigpatch.diffs.remove(0));
                    } else {
                        let take = (patch_size - patch.length1 - margin).max(0) as usize;
                        let diff_text: Vec<u16> = bigpatch.diffs[0].1[..take.min(bigpatch.diffs[0].1.len())].to_vec();
                        let taken = diff_text.len() as i64;
                        patch.length1 += taken;
                        start1 += taken;
                        if diff_type == Op::Equal {
                            patch.length2 += taken;
                            start2 += taken;
                        } else {
                            empty = false;
                        }
                        if diff_text == bigpatch.diffs[0].1 {
                            bigpatch.diffs.remove(0);
                        } else {
                            bigpatch.diffs[0].1.drain(..diff_text.len());
                        }
                        patch.diffs.push((diff_type, diff_text));
                    }
                }
                let text2 = Self::text2(&patch.diffs);
                precontext = text2[text2.len().saturating_sub(margin as usize)..].to_vec();
                let text1 = Self::text1(&bigpatch.diffs);
                let postcontext: Vec<u16> = text1[..text1.len().min(margin as usize)].to_vec();
                if !postcontext.is_empty() {
                    patch.length1 += postcontext.len() as i64;
                    patch.length2 += postcontext.len() as i64;
                    match patch.diffs.last_mut() {
                        Some(last) if last.0 == Op::Equal => last.1.extend_from_slice(&postcontext),
                        _ => patch.diffs.push((Op::Equal, postcontext)),
                    }
                }
                if !empty {
                    x += 1;
                    patches.insert(x as usize, patch);
                }
            }
            x += 1;
        }
    }

    /// `patch_apply(patches, text)`: the patched text and whether each patch applied.
    pub fn patch_apply(&self, patches: &[Patch], text: &JsString) -> Result<(JsString, Vec<bool>), JsError> {
        if patches.is_empty() {
            return Ok((text.clone(), Vec::new()));
        }
        let mut patches = patches.to_vec();
        let null_padding = self.patch_add_padding(&mut patches);
        let mut text: Vec<u16> = [&null_padding[..], text.units(), &null_padding[..]].concat();
        self.patch_split_max(&mut patches);
        let max_bits = self.match_max_bits;
        let mut delta: i64 = 0;
        let mut results = Vec::with_capacity(patches.len());
        for patch in &patches {
            let expected_loc = patch.start2.saturating_add(delta);
            let text1 = Self::text1(&patch.diffs);
            let mut end_loc: Option<usize> = None;
            let start_loc = if text1.len() > max_bits {
                let start = self.match_main(&text, &text1[..max_bits], expected_loc)?;
                match start {
                    Some(start) => {
                        let end = self.match_main(
                            &text,
                            &text1[text1.len() - max_bits..],
                            expected_loc.saturating_add(text1.len() as i64 - max_bits as i64),
                        )?;
                        end_loc = end;
                        match end {
                            Some(end) if start < end => Some(start),
                            _ => None,
                        }
                    }
                    None => None,
                }
            } else {
                self.match_main(&text, &text1, expected_loc)?
            };
            let Some(start_loc) = start_loc else {
                results.push(false);
                delta = delta.saturating_sub(patch.length2.saturating_sub(patch.length1));
                continue;
            };
            results.push(true);
            delta = (start_loc as i64).saturating_sub(expected_loc);
            let start = start_loc as i64;
            let text2 = match end_loc {
                None => js_substring(&text, start, start + text1.len() as i64),
                Some(end) => js_substring(&text, start, end as i64 + max_bits as i64),
            };
            if text1 == text2 {
                let replacement = Self::text2(&patch.diffs);
                text = [
                    &js_substring(&text, 0, start)[..],
                    &replacement[..],
                    &js_substring(&text, start + text1.len() as i64, text.len() as i64)[..],
                ]
                .concat();
                continue;
            }
            let mut diffs = self.diff_main(&text1, &text2, false)?;
            if text1.len() > max_bits
                && Self::levenshtein(&diffs) as f64 / text1.len() as f64 > self.patch_delete_threshold
            {
                if let Some(last) = results.last_mut() {
                    *last = false;
                }
                continue;
            }
            Self::cleanup_semantic_lossless(&mut diffs);
            let mut index1 = 0usize;
            let mut index2 = 0usize;
            for (op, data) in &patch.diffs {
                if *op != Op::Equal {
                    index2 = Self::x_index(&diffs, index1);
                }
                let at = start + index2 as i64;
                if *op == Op::Insert {
                    text = [
                        &js_substring(&text, 0, at)[..],
                        &data[..],
                        &js_substring(&text, at, text.len() as i64)[..],
                    ]
                    .concat();
                } else if *op == Op::Delete {
                    let end = start + Self::x_index(&diffs, index1 + data.len()) as i64;
                    text = [
                        &js_substring(&text, 0, at)[..],
                        &js_substring(&text, end, text.len() as i64)[..],
                    ]
                    .concat();
                }
                if *op != Op::Delete {
                    index1 += data.len();
                }
            }
        }
        let pad = null_padding.len() as i64;
        let stripped = js_substring(&text, pad, text.len() as i64 - pad);
        Ok((JsString::from_units(stripped), results))
    }
}
