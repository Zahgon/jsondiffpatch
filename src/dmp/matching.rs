// SPDX-License-Identifier: Apache-2.0
// Copyright 2018 The diff-match-patch Authors. https://github.com/google/diff-match-patch
// Ported to Rust from the JavaScript diff-match-patch 1.0.5 for the jsondiffpatch port.

//! Fuzzy matching: the Bitap algorithm on 32-bit integers, exactly as JavaScript's bitwise operators compute it.

use std::collections::HashMap;

use super::DiffMatchPatch;
use crate::js::string::{index_of, last_index_of};
use crate::js::JsError;

impl DiffMatchPatch {
    /// `match_main(text, pattern, loc)`: the best match of `pattern` in `text` near `loc`, or `None`.
    pub fn match_main(&self, text: &[u16], pattern: &[u16], loc: i64) -> Result<Option<usize>, JsError> {
        let loc = loc.clamp(0, text.len() as i64) as usize;
        if text == pattern {
            return Ok(Some(0));
        }
        if text.is_empty() {
            return Ok(None);
        }
        let end = (loc + pattern.len()).min(text.len());
        if text[loc..end] == *pattern {
            return Ok(Some(loc));
        }
        self.match_bitap(text, pattern, loc)
    }

    fn bitap_score(&self, errors: usize, x: i64, loc: usize, pattern_length: usize) -> f64 {
        let accuracy = errors as f64 / pattern_length as f64;
        let proximity = (loc as i64 - x).abs() as f64;
        if self.match_distance == 0.0 {
            return if proximity != 0.0 { 1.0 } else { accuracy };
        }
        accuracy + proximity / self.match_distance
    }

    fn match_bitap(&self, text: &[u16], pattern: &[u16], loc: usize) -> Result<Option<usize>, JsError> {
        if pattern.len() > self.match_max_bits {
            return Err(JsError::error("Pattern too long for this browser."));
        }
        let alphabet = Self::match_alphabet(pattern);
        let pattern_length = pattern.len();

        let mut score_threshold = self.match_threshold;
        if let Some(best) = index_of(text, pattern, loc) {
            score_threshold = self
                .bitap_score(0, best as i64, loc, pattern_length)
                .min(score_threshold);
            if let Some(best) = last_index_of(text, pattern, loc + pattern_length) {
                score_threshold = self
                    .bitap_score(0, best as i64, loc, pattern_length)
                    .min(score_threshold);
            }
        }

        let match_mask: i32 = 1i32.wrapping_shl((pattern_length as u32).wrapping_sub(1) & 31);
        let mut best_loc: Option<usize> = None;
        let mut bin_max = pattern_length + text.len();
        let mut last_rd: Vec<i32> = Vec::new();
        for d in 0..pattern_length {
            let mut bin_min = 0;
            let mut bin_mid = bin_max;
            while bin_min < bin_mid {
                if self.bitap_score(d, (loc + bin_mid) as i64, loc, pattern_length) <= score_threshold {
                    bin_min = bin_mid;
                } else {
                    bin_max = bin_mid;
                }
                bin_mid = (bin_max - bin_min) / 2 + bin_min;
            }
            bin_max = bin_mid;
            let mut start = (loc as i64 - bin_mid as i64 + 1).max(1) as usize;
            let finish = (loc + bin_mid).min(text.len()) + pattern_length;

            let mut rd = vec![0i32; finish + 2];
            rd[finish + 1] = ((1i64 << d) - 1) as i32;
            let mut j = finish;
            while j >= start {
                let char_match = if j - 1 < text.len() {
                    alphabet.get(&text[j - 1]).copied().unwrap_or(0)
                } else {
                    0
                };
                let last = |i: usize| last_rd.get(i).copied().unwrap_or(0);
                rd[j] = if d == 0 {
                    (rd[j + 1].wrapping_shl(1) | 1) & char_match
                } else {
                    ((rd[j + 1].wrapping_shl(1) | 1) & char_match)
                        | ((last(j + 1) | last(j)).wrapping_shl(1) | 1)
                        | last(j + 1)
                };
                if rd[j] & match_mask != 0 {
                    let score = self.bitap_score(d, j as i64 - 1, loc, pattern_length);
                    if score <= score_threshold {
                        score_threshold = score;
                        let found = j - 1;
                        best_loc = Some(found);
                        if found > loc {
                            start = (2 * loc as i64 - found as i64).max(1) as usize;
                        } else {
                            break;
                        }
                    }
                }
                j -= 1;
            }
            if self.bitap_score(d + 1, loc as i64, loc, pattern_length) > score_threshold {
                break;
            }
            last_rd = rd;
        }
        Ok(best_loc)
    }

    fn match_alphabet(pattern: &[u16]) -> HashMap<u16, i32> {
        let mut alphabet = HashMap::new();
        let length = pattern.len();
        for (i, &unit) in pattern.iter().enumerate() {
            let bit = 1i32.wrapping_shl(((length - i - 1) & 31) as u32);
            *alphabet.entry(unit).or_insert(0) |= bit;
        }
        alphabet
    }
}
