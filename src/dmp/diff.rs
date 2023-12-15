// SPDX-License-Identifier: Apache-2.0
// Copyright 2018 The diff-match-patch Authors. https://github.com/google/diff-match-patch
// Ported to Rust from the JavaScript diff-match-patch 1.0.5 for the jsondiffpatch port.

//! Diffs: the Myers bisection with the half-match and line-mode speedups, and the semantic, lossless and
//! efficiency cleanups.

use std::collections::HashMap;

use super::{Diff, DiffMatchPatch, Op};
use crate::js::string::{index_of, is_js_whitespace};
use crate::js::JsError;

fn is_alphanumeric(unit: u16) -> bool {
    matches!(unit, 0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A)
}

fn concat(parts: &[&[u16]]) -> Vec<u16> {
    let mut out = Vec::with_capacity(parts.iter().map(|p| p.len()).sum());
    for part in parts {
        out.extend_from_slice(part);
    }
    out
}

/// A half match: the prefix and suffix of text1, the prefix and suffix of text2, and the common middle.
type HalfMatch = [Vec<u16>; 5];

/// Two texts with every line replaced by one code unit, and the lines those units stand for.
struct LineEncoding {
    chars1: Vec<u16>,
    chars2: Vec<u16>,
    line_array: Vec<Vec<u16>>,
}

impl DiffMatchPatch {
    /// `diff_main(text1, text2, checklines)`.
    pub fn diff_main(&self, text1: &[u16], text2: &[u16], checklines: bool) -> Result<Vec<Diff>, JsError> {
        if text1 == text2 {
            if !text1.is_empty() {
                return Ok(vec![(Op::Equal, text1.to_vec())]);
            }
            return Ok(Vec::new());
        }
        let common = Self::common_prefix(text1, text2);
        let prefix = &text1[..common];
        let (text1, text2) = (&text1[common..], &text2[common..]);
        let common = Self::common_suffix(text1, text2);
        let suffix = &text1[text1.len() - common..];
        let (text1, text2) = (&text1[..text1.len() - common], &text2[..text2.len() - common]);

        let mut diffs = self.diff_compute(text1, text2, checklines)?;
        if !prefix.is_empty() {
            diffs.insert(0, (Op::Equal, prefix.to_vec()));
        }
        if !suffix.is_empty() {
            diffs.push((Op::Equal, suffix.to_vec()));
        }
        Self::cleanup_merge(&mut diffs);
        Ok(diffs)
    }

    fn diff_compute(&self, text1: &[u16], text2: &[u16], checklines: bool) -> Result<Vec<Diff>, JsError> {
        if text1.is_empty() {
            return Ok(vec![(Op::Insert, text2.to_vec())]);
        }
        if text2.is_empty() {
            return Ok(vec![(Op::Delete, text1.to_vec())]);
        }
        let (longtext, shorttext) = if text1.len() > text2.len() {
            (text1, text2)
        } else {
            (text2, text1)
        };
        if let Some(i) = index_of(longtext, shorttext, 0) {
            let op = if text1.len() > text2.len() {
                Op::Delete
            } else {
                Op::Insert
            };
            return Ok(vec![
                (op, longtext[..i].to_vec()),
                (Op::Equal, shorttext.to_vec()),
                (op, longtext[i + shorttext.len()..].to_vec()),
            ]);
        }
        if shorttext.len() == 1 {
            return Ok(vec![(Op::Delete, text1.to_vec()), (Op::Insert, text2.to_vec())]);
        }
        if let Some([text1_a, text1_b, text2_a, text2_b, mid_common]) = self.half_match(text1, text2) {
            let mut diffs = self.diff_main(&text1_a, &text2_a, checklines)?;
            diffs.push((Op::Equal, mid_common));
            diffs.extend(self.diff_main(&text1_b, &text2_b, checklines)?);
            return Ok(diffs);
        }
        if checklines && text1.len() > 100 && text2.len() > 100 {
            return self.line_mode(text1, text2);
        }
        Ok(self.bisect(text1, text2))
    }

    fn line_mode(&self, text1: &[u16], text2: &[u16]) -> Result<Vec<Diff>, JsError> {
        let LineEncoding {
            chars1,
            chars2,
            line_array,
        } = Self::lines_to_chars(text1, text2)?;
        let mut diffs = self.diff_main(&chars1, &chars2, false)?;
        for diff in diffs.iter_mut() {
            let mut text = Vec::new();
            for &unit in &diff.1 {
                if let Some(line) = line_array.get(usize::from(unit)) {
                    text.extend_from_slice(line);
                }
            }
            diff.1 = text;
        }
        Self::cleanup_semantic(&mut diffs);

        diffs.push((Op::Equal, Vec::new()));
        let mut pointer = 0;
        let (mut count_delete, mut count_insert) = (0, 0);
        let (mut text_delete, mut text_insert) = (Vec::new(), Vec::new());
        while pointer < diffs.len() {
            match diffs[pointer].0 {
                Op::Insert => {
                    count_insert += 1;
                    text_insert.extend_from_slice(&diffs[pointer].1);
                }
                Op::Delete => {
                    count_delete += 1;
                    text_delete.extend_from_slice(&diffs[pointer].1);
                }
                Op::Equal => {
                    if count_delete >= 1 && count_insert >= 1 {
                        let start = pointer - count_delete - count_insert;
                        diffs.drain(start..pointer);
                        pointer = start;
                        let sub = self.diff_main(&text_delete, &text_insert, false)?;
                        let sub_len = sub.len();
                        diffs.splice(pointer..pointer, sub);
                        pointer += sub_len;
                    }
                    count_insert = 0;
                    count_delete = 0;
                    text_delete.clear();
                    text_insert.clear();
                }
            }
            pointer += 1;
        }
        diffs.pop();
        Ok(diffs)
    }

    fn bisect(&self, text1: &[u16], text2: &[u16]) -> Vec<Diff> {
        let text1_length = text1.len() as isize;
        let text2_length = text2.len() as isize;
        let max_d = (text1_length + text2_length + 1) / 2;
        let v_offset = max_d;
        let v_length = 2 * max_d;
        let mut v1 = vec![-1isize; v_length as usize];
        let mut v2 = vec![-1isize; v_length as usize];
        v1[(v_offset + 1) as usize] = 0;
        v2[(v_offset + 1) as usize] = 0;
        let delta = text1_length - text2_length;
        let front = delta % 2 != 0;
        let (mut k1start, mut k1end, mut k2start, mut k2end) = (0isize, 0isize, 0isize, 0isize);
        let at = |v: &Vec<isize>, i: isize| -> isize {
            if i >= 0 && (i as usize) < v.len() {
                v[i as usize]
            } else {
                // Reading past the JavaScript array yields undefined, which compares false.
                isize::MIN
            }
        };
        for d in 0..max_d {
            let mut k1 = -d + k1start;
            while k1 <= d - k1end {
                let k1_offset = v_offset + k1;
                let mut x1 = if k1 == -d || (k1 != d && lt(at(&v1, k1_offset - 1), at(&v1, k1_offset + 1))) {
                    at(&v1, k1_offset + 1)
                } else {
                    at(&v1, k1_offset - 1) + 1
                };
                let mut y1 = x1 - k1;
                while x1 < text1_length && y1 < text2_length && text1[x1 as usize] == text2[y1 as usize] {
                    x1 += 1;
                    y1 += 1;
                }
                v1[k1_offset as usize] = x1;
                if x1 > text1_length {
                    k1end += 2;
                } else if y1 > text2_length {
                    k1start += 2;
                } else if front {
                    let k2_offset = v_offset + delta - k1;
                    if k2_offset >= 0 && k2_offset < v_length && v2[k2_offset as usize] != -1 {
                        let x2 = text1_length - v2[k2_offset as usize];
                        if x1 >= x2 {
                            return self.bisect_split(text1, text2, x1 as usize, y1 as usize);
                        }
                    }
                }
                k1 += 2;
            }
            let mut k2 = -d + k2start;
            while k2 <= d - k2end {
                let k2_offset = v_offset + k2;
                let mut x2 = if k2 == -d || (k2 != d && lt(at(&v2, k2_offset - 1), at(&v2, k2_offset + 1))) {
                    at(&v2, k2_offset + 1)
                } else {
                    at(&v2, k2_offset - 1) + 1
                };
                let mut y2 = x2 - k2;
                while x2 < text1_length
                    && y2 < text2_length
                    && text1[(text1_length - x2 - 1) as usize] == text2[(text2_length - y2 - 1) as usize]
                {
                    x2 += 1;
                    y2 += 1;
                }
                v2[k2_offset as usize] = x2;
                if x2 > text1_length {
                    k2end += 2;
                } else if y2 > text2_length {
                    k2start += 2;
                } else if !front {
                    let k1_offset = v_offset + delta - k2;
                    if k1_offset >= 0 && k1_offset < v_length && v1[k1_offset as usize] != -1 {
                        let x1 = v1[k1_offset as usize];
                        let y1 = v_offset + x1 - k1_offset;
                        let x2 = text1_length - x2;
                        if x1 >= x2 {
                            return self.bisect_split(text1, text2, x1 as usize, y1 as usize);
                        }
                    }
                }
                k2 += 2;
            }
        }
        vec![(Op::Delete, text1.to_vec()), (Op::Insert, text2.to_vec())]
    }

    fn bisect_split(&self, text1: &[u16], text2: &[u16], x: usize, y: usize) -> Vec<Diff> {
        // Bisection never reaches line mode, the only part of a diff that can fail.
        let mut diffs = self.diff_main(&text1[..x], &text2[..y], false).unwrap_or_default();
        diffs.extend(self.diff_main(&text1[x..], &text2[y..], false).unwrap_or_default());
        diffs
    }

    /// Encodes every line of both texts as one code unit, numbering distinct lines from 1. The line table is a
    /// plain JavaScript object: a line `__proto__` never matches, and once a last line `hasOwnProperty` has been
    /// stored, looking up the next line fails.
    fn lines_to_chars(text1: &[u16], text2: &[u16]) -> Result<LineEncoding, JsError> {
        let mut line_array: Vec<Vec<u16>> = vec![Vec::new()];
        let mut line_hash: HashMap<Vec<u16>, usize> = HashMap::new();
        let chars1 = Self::lines_munge(text1, &mut line_array, &mut line_hash, 40000)?;
        let chars2 = Self::lines_munge(text2, &mut line_array, &mut line_hash, 65535)?;
        Ok(LineEncoding {
            chars1,
            chars2,
            line_array,
        })
    }

    fn lines_munge(
        text: &[u16],
        line_array: &mut Vec<Vec<u16>>,
        line_hash: &mut HashMap<Vec<u16>, usize>,
        max_lines: usize,
    ) -> Result<Vec<u16>, JsError> {
        let shadowed: Vec<u16> = "hasOwnProperty".encode_utf16().collect();
        let proto: Vec<u16> = "__proto__".encode_utf16().collect();
        let mut chars = Vec::new();
        let mut line_start = 0usize;
        let mut line_end: isize = -1;
        while line_end < text.len() as isize - 1 {
            line_end = match index_of(text, &[0x0A], line_start) {
                Some(i) => i as isize,
                None => text.len() as isize - 1,
            };
            let mut line = text[line_start..(line_end + 1) as usize].to_vec();
            if line_hash.contains_key(&shadowed) {
                return Err(JsError::type_error("lineHash.hasOwnProperty is not a function"));
            }
            if let Some(&n) = line_hash.get(&line) {
                chars.push(n as u16);
            } else {
                if line_array.len() == max_lines {
                    line = text[line_start..].to_vec();
                    line_end = text.len() as isize;
                }
                chars.push(line_array.len() as u16);
                if line != proto {
                    line_hash.insert(line.clone(), line_array.len());
                }
                line_array.push(line);
            }
            line_start = (line_end + 1) as usize;
        }
        Ok(chars)
    }

    /// The length of the common prefix of two texts.
    pub fn common_prefix(text1: &[u16], text2: &[u16]) -> usize {
        text1.iter().zip(text2).take_while(|(a, b)| a == b).count()
    }

    /// The length of the common suffix of two texts.
    pub fn common_suffix(text1: &[u16], text2: &[u16]) -> usize {
        text1
            .iter()
            .rev()
            .zip(text2.iter().rev())
            .take_while(|(a, b)| a == b)
            .count()
    }

    /// The length of the longest suffix of text1 that is a prefix of text2.
    fn common_overlap(text1: &[u16], text2: &[u16]) -> usize {
        let (text1_length, text2_length) = (text1.len(), text2.len());
        if text1_length == 0 || text2_length == 0 {
            return 0;
        }
        let text1 = if text1_length > text2_length {
            &text1[text1_length - text2_length..]
        } else {
            text1
        };
        let text2 = if text1_length < text2_length {
            &text2[..text1_length]
        } else {
            text2
        };
        let text_length = text1_length.min(text2_length);
        if text1 == text2 {
            return text_length;
        }
        let mut best = 0;
        let mut length = 1;
        loop {
            if length > text_length {
                return best;
            }
            let pattern = &text1[text_length - length..];
            let Some(found) = index_of(text2, pattern, 0) else {
                return best;
            };
            length += found;
            if found == 0 || (length <= text_length && text1[text_length - length..] == text2[..length]) {
                best = length;
                length += 1;
            }
        }
    }

    fn half_match(&self, text1: &[u16], text2: &[u16]) -> Option<HalfMatch> {
        let (longtext, shorttext) = if text1.len() > text2.len() {
            (text1, text2)
        } else {
            (text2, text1)
        };
        if longtext.len() < 4 || shorttext.len() * 2 < longtext.len() {
            return None;
        }
        let hm1 = Self::half_match_i(longtext, shorttext, longtext.len().div_ceil(4));
        let hm2 = Self::half_match_i(longtext, shorttext, longtext.len().div_ceil(2));
        let hm = match (hm1, hm2) {
            (None, None) => return None,
            (Some(hm1), None) => hm1,
            (None, Some(hm2)) => hm2,
            (Some(hm1), Some(hm2)) => {
                if hm1[4].len() > hm2[4].len() {
                    hm1
                } else {
                    hm2
                }
            }
        };
        let [a, b, c, d, mid] = hm;
        if text1.len() > text2.len() {
            Some([a, b, c, d, mid])
        } else {
            Some([c, d, a, b, mid])
        }
    }

    fn half_match_i(longtext: &[u16], shorttext: &[u16], i: usize) -> Option<HalfMatch> {
        let seed_end = (i + longtext.len() / 4).min(longtext.len());
        let seed = &longtext[i.min(longtext.len())..seed_end];
        let mut best_common: Vec<u16> = Vec::new();
        let mut best: Option<[Vec<u16>; 4]> = None;
        let mut from = 0usize;
        while let Some(j) = index_of(shorttext, seed, from) {
            let prefix_length = Self::common_prefix(&longtext[i..], &shorttext[j..]);
            let suffix_length = Self::common_suffix(&longtext[..i], &shorttext[..j]);
            if best_common.len() < suffix_length + prefix_length {
                best_common = concat(&[&shorttext[j - suffix_length..j], &shorttext[j..j + prefix_length]]);
                best = Some([
                    longtext[..i - suffix_length].to_vec(),
                    longtext[i + prefix_length..].to_vec(),
                    shorttext[..j - suffix_length].to_vec(),
                    shorttext[j + prefix_length..].to_vec(),
                ]);
            }
            from = j + 1;
            if from > shorttext.len() {
                break;
            }
        }
        if best_common.len() * 2 >= longtext.len() {
            let [a, b, c, d] = best.unwrap_or_default();
            Some([a, b, c, d, best_common])
        } else {
            None
        }
    }

    /// `diff_cleanupSemantic`.
    pub fn cleanup_semantic(diffs: &mut Vec<Diff>) {
        let mut changes = false;
        let mut equalities: Vec<usize> = Vec::new();
        let mut last_equality: Option<Vec<u16>> = None;
        let mut pointer: isize = 0;
        let (mut length_insertions1, mut length_deletions1) = (0usize, 0usize);
        let (mut length_insertions2, mut length_deletions2) = (0usize, 0usize);
        while (pointer as usize) < diffs.len() {
            let p = pointer as usize;
            if diffs[p].0 == Op::Equal {
                equalities.push(p);
                length_insertions1 = length_insertions2;
                length_deletions1 = length_deletions2;
                length_insertions2 = 0;
                length_deletions2 = 0;
                last_equality = Some(diffs[p].1.clone());
            } else {
                if diffs[p].0 == Op::Insert {
                    length_insertions2 += diffs[p].1.len();
                } else {
                    length_deletions2 += diffs[p].1.len();
                }
                if let Some(equality) = last_equality.as_ref().filter(|e| !e.is_empty()) {
                    if equality.len() <= length_insertions1.max(length_deletions1)
                        && equality.len() <= length_insertions2.max(length_deletions2)
                    {
                        let at = *equalities.last().unwrap_or(&0);
                        diffs.insert(at, (Op::Delete, equality.clone()));
                        diffs[at + 1].0 = Op::Insert;
                        equalities.pop();
                        equalities.pop();
                        pointer = equalities.last().map_or(-1, |&e| e as isize);
                        length_insertions1 = 0;
                        length_deletions1 = 0;
                        length_insertions2 = 0;
                        length_deletions2 = 0;
                        last_equality = None;
                        changes = true;
                    }
                }
            }
            pointer += 1;
        }

        if changes {
            Self::cleanup_merge(diffs);
        }
        Self::cleanup_semantic_lossless(diffs);

        let mut pointer = 1;
        while pointer < diffs.len() {
            if diffs[pointer - 1].0 == Op::Delete && diffs[pointer].0 == Op::Insert {
                let deletion = diffs[pointer - 1].1.clone();
                let insertion = diffs[pointer].1.clone();
                let overlap_length1 = Self::common_overlap(&deletion, &insertion);
                let overlap_length2 = Self::common_overlap(&insertion, &deletion);
                if overlap_length1 >= overlap_length2 {
                    if overlap_length1 as f64 >= deletion.len() as f64 / 2.0
                        || overlap_length1 as f64 >= insertion.len() as f64 / 2.0
                    {
                        diffs.insert(pointer, (Op::Equal, insertion[..overlap_length1].to_vec()));
                        diffs[pointer - 1].1 = deletion[..deletion.len() - overlap_length1].to_vec();
                        diffs[pointer + 1].1 = insertion[overlap_length1..].to_vec();
                        pointer += 1;
                    }
                } else if overlap_length2 as f64 >= deletion.len() as f64 / 2.0
                    || overlap_length2 as f64 >= insertion.len() as f64 / 2.0
                {
                    diffs.insert(pointer, (Op::Equal, deletion[..overlap_length2].to_vec()));
                    diffs[pointer - 1] = (Op::Insert, insertion[..insertion.len() - overlap_length2].to_vec());
                    diffs[pointer + 1] = (Op::Delete, deletion[overlap_length2..].to_vec());
                    pointer += 1;
                }
                pointer += 1;
            }
            pointer += 1;
        }
    }

    fn semantic_score(one: &[u16], two: &[u16]) -> u32 {
        let (Some(&char1), Some(&char2)) = (one.last(), two.first()) else {
            return 6;
        };
        let non_alphanumeric1 = !is_alphanumeric(char1);
        let non_alphanumeric2 = !is_alphanumeric(char2);
        let whitespace1 = non_alphanumeric1 && is_js_whitespace(char1);
        let whitespace2 = non_alphanumeric2 && is_js_whitespace(char2);
        let line_break1 = whitespace1 && (char1 == 0x0D || char1 == 0x0A);
        let line_break2 = whitespace2 && (char2 == 0x0D || char2 == 0x0A);
        let blank_line1 = line_break1 && ends_with_blank_line(one);
        let blank_line2 = line_break2 && starts_with_blank_line(two);
        if blank_line1 || blank_line2 {
            5
        } else if line_break1 || line_break2 {
            4
        } else if non_alphanumeric1 && !whitespace1 && whitespace2 {
            3
        } else if whitespace1 || whitespace2 {
            2
        } else if non_alphanumeric1 || non_alphanumeric2 {
            1
        } else {
            0
        }
    }

    /// `diff_cleanupSemanticLossless`.
    pub fn cleanup_semantic_lossless(diffs: &mut Vec<Diff>) {
        let mut pointer = 1;
        while pointer + 1 < diffs.len() {
            if diffs[pointer - 1].0 == Op::Equal && diffs[pointer + 1].0 == Op::Equal {
                let mut equality1 = diffs[pointer - 1].1.clone();
                let mut edit = diffs[pointer].1.clone();
                let mut equality2 = diffs[pointer + 1].1.clone();

                let common_offset = Self::common_suffix(&equality1, &edit);
                if common_offset > 0 {
                    let common = edit[edit.len() - common_offset..].to_vec();
                    equality1.truncate(equality1.len() - common_offset);
                    edit = concat(&[&common, &edit[..edit.len() - common_offset]]);
                    equality2 = concat(&[&common, &equality2]);
                }

                let mut best_equality1 = equality1.clone();
                let mut best_edit = edit.clone();
                let mut best_equality2 = equality2.clone();
                let mut best_score = Self::semantic_score(&equality1, &edit) + Self::semantic_score(&edit, &equality2);
                while !edit.is_empty() && edit.first() == equality2.first() {
                    let unit = edit[0];
                    equality1.push(unit);
                    edit.remove(0);
                    edit.push(equality2[0]);
                    equality2.remove(0);
                    let score = Self::semantic_score(&equality1, &edit) + Self::semantic_score(&edit, &equality2);
                    if score >= best_score {
                        best_score = score;
                        best_equality1 = equality1.clone();
                        best_edit = edit.clone();
                        best_equality2 = equality2.clone();
                    }
                }

                if diffs[pointer - 1].1 != best_equality1 {
                    if !best_equality1.is_empty() {
                        diffs[pointer - 1].1 = best_equality1;
                    } else {
                        diffs.remove(pointer - 1);
                        pointer -= 1;
                    }
                    diffs[pointer].1 = best_edit;
                    if !best_equality2.is_empty() {
                        diffs[pointer + 1].1 = best_equality2;
                    } else {
                        diffs.remove(pointer + 1);
                        pointer = pointer.wrapping_sub(1);
                    }
                }
            }
            pointer = pointer.wrapping_add(1);
        }
    }

    /// `diff_cleanupEfficiency`.
    pub fn cleanup_efficiency(&self, diffs: &mut Vec<Diff>) {
        let mut changes = false;
        let mut equalities: Vec<usize> = Vec::new();
        let mut last_equality: Option<Vec<u16>> = None;
        let mut pointer: isize = 0;
        let (mut pre_ins, mut pre_del, mut post_ins, mut post_del) = (false, false, false, false);
        let edit_cost = self.diff_edit_cost;
        while (pointer as usize) < diffs.len() {
            let p = pointer as usize;
            if diffs[p].0 == Op::Equal {
                if diffs[p].1.len() < edit_cost && (post_ins || post_del) {
                    equalities.push(p);
                    pre_ins = post_ins;
                    pre_del = post_del;
                    last_equality = Some(diffs[p].1.clone());
                } else {
                    equalities.clear();
                    last_equality = None;
                }
                post_ins = false;
                post_del = false;
            } else {
                if diffs[p].0 == Op::Delete {
                    post_del = true;
                } else {
                    post_ins = true;
                }
                let flags = [pre_ins, pre_del, post_ins, post_del].iter().filter(|&&f| f).count();
                if let Some(equality) = last_equality.as_ref().filter(|e| !e.is_empty()) {
                    if (pre_ins && pre_del && post_ins && post_del)
                        || ((equality.len() as f64) < edit_cost as f64 / 2.0 && flags == 3)
                    {
                        let at = *equalities.last().unwrap_or(&0);
                        diffs.insert(at, (Op::Delete, equality.clone()));
                        diffs[at + 1].0 = Op::Insert;
                        equalities.pop();
                        last_equality = None;
                        if pre_ins && pre_del {
                            post_ins = true;
                            post_del = true;
                            equalities.clear();
                        } else {
                            equalities.pop();
                            pointer = equalities.last().map_or(-1, |&e| e as isize);
                            post_ins = false;
                            post_del = false;
                        }
                        changes = true;
                    }
                }
            }
            pointer += 1;
        }
        if changes {
            Self::cleanup_merge(diffs);
        }
    }

    /// `diff_cleanupMerge`.
    pub fn cleanup_merge(diffs: &mut Vec<Diff>) {
        diffs.push((Op::Equal, Vec::new()));
        let mut pointer = 0;
        let (mut count_delete, mut count_insert) = (0usize, 0usize);
        let (mut text_delete, mut text_insert): (Vec<u16>, Vec<u16>) = (Vec::new(), Vec::new());
        while pointer < diffs.len() {
            match diffs[pointer].0 {
                Op::Insert => {
                    count_insert += 1;
                    text_insert.extend_from_slice(&diffs[pointer].1);
                    pointer += 1;
                }
                Op::Delete => {
                    count_delete += 1;
                    text_delete.extend_from_slice(&diffs[pointer].1);
                    pointer += 1;
                }
                Op::Equal => {
                    if count_delete + count_insert > 1 {
                        if count_delete != 0 && count_insert != 0 {
                            let common = Self::common_prefix(&text_insert, &text_delete);
                            if common != 0 {
                                let before = pointer - count_delete - count_insert;
                                if before > 0 && diffs[before - 1].0 == Op::Equal {
                                    let prefix = text_insert[..common].to_vec();
                                    diffs[before - 1].1.extend_from_slice(&prefix);
                                } else {
                                    diffs.insert(0, (Op::Equal, text_insert[..common].to_vec()));
                                    pointer += 1;
                                }
                                text_insert.drain(..common);
                                text_delete.drain(..common);
                            }
                            let common = Self::common_suffix(&text_insert, &text_delete);
                            if common != 0 {
                                let suffix = text_insert[text_insert.len() - common..].to_vec();
                                diffs[pointer].1 = concat(&[&suffix, &diffs[pointer].1]);
                                text_insert.truncate(text_insert.len() - common);
                                text_delete.truncate(text_delete.len() - common);
                            }
                        }
                        pointer -= count_delete + count_insert;
                        diffs.drain(pointer..pointer + count_delete + count_insert);
                        if !text_delete.is_empty() {
                            diffs.insert(pointer, (Op::Delete, text_delete.clone()));
                            pointer += 1;
                        }
                        if !text_insert.is_empty() {
                            diffs.insert(pointer, (Op::Insert, text_insert.clone()));
                            pointer += 1;
                        }
                        pointer += 1;
                    } else if pointer != 0 && diffs[pointer - 1].0 == Op::Equal {
                        let text = std::mem::take(&mut diffs[pointer].1);
                        diffs[pointer - 1].1.extend_from_slice(&text);
                        diffs.remove(pointer);
                    } else {
                        pointer += 1;
                    }
                    count_insert = 0;
                    count_delete = 0;
                    text_delete.clear();
                    text_insert.clear();
                }
            }
        }
        if diffs.last().is_some_and(|d| d.1.is_empty()) {
            diffs.pop();
        }

        let mut changes = false;
        let mut pointer = 1;
        while pointer + 1 < diffs.len() {
            if diffs[pointer - 1].0 == Op::Equal && diffs[pointer + 1].0 == Op::Equal {
                let previous = diffs[pointer - 1].1.clone();
                let next = diffs[pointer + 1].1.clone();
                let edit = diffs[pointer].1.clone();
                if edit.len() >= previous.len() && edit[edit.len() - previous.len()..] == previous[..] {
                    diffs[pointer].1 = concat(&[&previous, &edit[..edit.len() - previous.len()]]);
                    diffs[pointer + 1].1 = concat(&[&previous, &next]);
                    diffs.remove(pointer - 1);
                    changes = true;
                } else if edit.len() >= next.len() && edit[..next.len()] == next[..] {
                    diffs[pointer - 1].1.extend_from_slice(&next);
                    diffs[pointer].1 = concat(&[&edit[next.len()..], &next]);
                    diffs.remove(pointer + 1);
                    changes = true;
                }
            }
            pointer += 1;
        }
        if changes {
            Self::cleanup_merge(diffs);
        }
    }

    /// `diff_xIndex`: the location in text2 equivalent to `loc` in text1.
    pub fn x_index(diffs: &[Diff], loc: usize) -> usize {
        let (mut chars1, mut chars2, mut last_chars1, mut last_chars2) = (0, 0, 0, 0);
        let mut x = 0;
        while x < diffs.len() {
            if diffs[x].0 != Op::Insert {
                chars1 += diffs[x].1.len();
            }
            if diffs[x].0 != Op::Delete {
                chars2 += diffs[x].1.len();
            }
            if chars1 > loc {
                break;
            }
            last_chars1 = chars1;
            last_chars2 = chars2;
            x += 1;
        }
        if x != diffs.len() && diffs[x].0 == Op::Delete {
            return last_chars2;
        }
        last_chars2 + (loc - last_chars1)
    }

    /// The source text of a diff (equalities and deletions).
    pub fn text1(diffs: &[Diff]) -> Vec<u16> {
        diffs
            .iter()
            .filter(|d| d.0 != Op::Insert)
            .flat_map(|d| d.1.iter().copied())
            .collect()
    }

    /// The destination text of a diff (equalities and insertions).
    pub fn text2(diffs: &[Diff]) -> Vec<u16> {
        diffs
            .iter()
            .filter(|d| d.0 != Op::Delete)
            .flat_map(|d| d.1.iter().copied())
            .collect()
    }

    /// The Levenshtein distance of a diff.
    pub fn levenshtein(diffs: &[Diff]) -> usize {
        let (mut levenshtein, mut insertions, mut deletions) = (0, 0, 0);
        for (op, text) in diffs {
            match op {
                Op::Insert => insertions += text.len(),
                Op::Delete => deletions += text.len(),
                Op::Equal => {
                    levenshtein += insertions.max(deletions);
                    insertions = 0;
                    deletions = 0;
                }
            }
        }
        levenshtein + insertions.max(deletions)
    }
}

/// JavaScript `<` on two bisection cells, where a cell past the end of the array is `undefined`.
fn lt(a: isize, b: isize) -> bool {
    a != isize::MIN && b != isize::MIN && a < b
}

fn ends_with_blank_line(text: &[u16]) -> bool {
    let n = text.len();
    n >= 2 && text[n - 1] == 0x0A && (text[n - 2] == 0x0A || (n >= 3 && text[n - 2] == 0x0D && text[n - 3] == 0x0A))
}

fn starts_with_blank_line(text: &[u16]) -> bool {
    let mut i = 0;
    for _ in 0..2 {
        if text.get(i) == Some(&0x0D) {
            i += 1;
        }
        if text.get(i) != Some(&0x0A) {
            return false;
        }
        i += 1;
    }
    true
}
