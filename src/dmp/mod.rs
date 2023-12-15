// SPDX-License-Identifier: Apache-2.0
// Copyright 2018 The diff-match-patch Authors. https://github.com/google/diff-match-patch
// Ported to Rust from the JavaScript diff-match-patch 1.0.5 for the jsondiffpatch port.

//! diff-match-patch: the text diff, fuzzy match and patch library jsondiffpatch uses for long strings. Texts are
//! UTF-16 code units, as in JavaScript, so a diff can split a surrogate pair exactly where the JavaScript library
//! does. The time limit of the JavaScript library (one second per diff) is not modelled: within it, results are the
//! same.

mod diff;
mod matching;
mod patch;

pub use patch::Patch;

/// What a diff does with its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// Text only in the old version.
    Delete,
    /// Text only in the new version.
    Insert,
    /// Text in both.
    Equal,
}

/// One diff: an operation and its text.
pub type Diff = (Op, Vec<u16>);

/// The library's settings, with the JavaScript library's defaults.
#[derive(Clone, Debug)]
pub struct DiffMatchPatch {
    /// Cost of an empty edit in characters, for the efficiency cleanup.
    pub diff_edit_cost: usize,
    /// At what score no match is declared (0.0 perfect, 1.0 very loose).
    pub match_threshold: f64,
    /// How far from the expected location a match may be before it costs 1.0.
    pub match_distance: f64,
    /// How closely the contents of a large deletion must match.
    pub patch_delete_threshold: f64,
    /// The context size of patches.
    pub patch_margin: usize,
    /// The number of bits in a match pattern.
    pub match_max_bits: usize,
}

impl Default for DiffMatchPatch {
    fn default() -> Self {
        DiffMatchPatch {
            diff_edit_cost: 4,
            match_threshold: 0.5,
            match_distance: 1000.0,
            patch_delete_threshold: 0.5,
            patch_margin: 4,
            match_max_bits: 32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js::JsString;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn patch_text(a: &str, b: &str) -> String {
        let dmp = DiffMatchPatch::default();
        let patches = dmp
            .patch_make(&units(a), &units(b))
            .expect("patch_make succeeds on plain test strings");
        DiffMatchPatch::patch_to_text(&patches)
            .map(|t| t.to_string_lossy())
            .expect("patch_toText succeeds on patches made from plain test strings")
    }

    #[test]
    fn makes_patches_like_javascript() {
        assert_eq!(
            patch_text("The quick brown fox jumps over the lazy dog.", "That quick brown fox jumped over a lazy dog."),
            "@@ -1,11 +1,12 @@\n Th\n-e\n+at\n  quick b\n@@ -22,18 +22,17 @@\n jump\n-s\n+ed\n  over \n-the\n+a\n  laz\n"
        );
    }

    #[test]
    fn applies_patches_fuzzily() {
        let dmp = DiffMatchPatch::default();
        let patches = dmp
            .patch_make(
                &units("The quick brown fox jumps over the lazy dog."),
                &units("That quick brown fox jumped over a lazy dog."),
            )
            .expect("patch_make succeeds on plain test strings");
        let (text, results) = dmp
            .patch_apply(
                &patches,
                &JsString::from("The quick red rabbit jumps over the tired tiger."),
            )
            .expect("patch_apply succeeds on a fuzzy match");
        assert_eq!(
            text.to_string_lossy(),
            "That quick red rabbit jumped over a tired tiger."
        );
        assert_eq!(results, vec![true, true]);
    }

    #[test]
    fn huge_header_coordinates_do_not_overflow() {
        let dmp = DiffMatchPatch::default();
        let text = JsString::from("@@ -9223372036854775807,1 +9223372036854775807,1 @@\n-a\n+b\n");
        let patches = DiffMatchPatch::patch_from_text(&text).expect("the patch text is well formed");
        let (_, results) = dmp
            .patch_apply(&patches, &JsString::from("abc"))
            .expect("a patch with an out-of-range header is applied or reported, never a panic");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn parses_patch_text() {
        let text = JsString::from("@@ -21,18 +22,17 @@\n jump\n-ed\n+s\n  over \n-a\n+the\n  laz\n");
        let patches = DiffMatchPatch::patch_from_text(&text).expect("the patch text is well formed");
        assert_eq!(patches.len(), 1);
        assert_eq!(
            (
                patches[0].start1,
                patches[0].length1,
                patches[0].start2,
                patches[0].length2
            ),
            (20, 18, 21, 17)
        );
        let error = DiffMatchPatch::patch_from_text(&JsString::from("@@ -1 +1 @@\n x\n+%zz")).err();
        assert_eq!(
            error.map(|e| e.message.to_string_lossy()).as_deref(),
            Some("Illegal escape in patch_fromText: x")
        );
    }
}
