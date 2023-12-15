// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! DiffPatcher options. Most option values are kept as the JSON values they were given and read with
//! JavaScript's truthiness and comparisons, as the library reads them.

use crate::js::{JsString, Object, Value};

/// The options of a [`crate::DiffPatcher`].
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// `objectHash`: array items that are objects match when this property of both is defined and strictly
    /// equal.
    pub object_hash: Option<JsString>,
    /// `matchByPosition`: match object array items by position when there is no object hash (any value; only a
    /// boolean overrides the library's own choice).
    pub match_by_position: Value,
    /// `arrays`: `detectMove` (strictly `false` turns move detection off) and `includeValueOnMove` (truthy).
    pub arrays: Value,
    /// `textDiff`: `minLength`, the length from which strings get a text diff (60 when falsy).
    pub text_diff: Object,
    /// `propertyFilter`: object properties whose names are in this list are not diffed.
    pub property_filter: Option<Vec<Value>>,
    /// `cloneDiffValues`: values in diffs are copies of the inputs (results are the same either way).
    pub clone_diff_values: Value,
}

impl Options {
    /// Whether move detection is on.
    pub fn detect_move(&self) -> bool {
        !(self.arrays.truthy()
            && self
                .arrays
                .get(&JsString::from("detectMove"))
                .strict_equals(&Value::Bool(false)))
    }

    /// Whether moved items keep their value in the delta.
    pub fn include_value_on_move(&self) -> bool {
        self.arrays.truthy() && self.arrays.get(&JsString::from("includeValueOnMove")).truthy()
    }

    /// The `minLength` text diff option, or 60.
    pub fn text_min_length(&self) -> Value {
        let value = self
            .text_diff
            .get(&JsString::from("minLength"))
            .cloned()
            .unwrap_or_default();
        if value.truthy() {
            value
        } else {
            Value::Number(60.0)
        }
    }

    /// Whether the property filter lets `name` through.
    pub fn includes_property(&self, name: &JsString) -> bool {
        match &self.property_filter {
            Some(names) => !names.iter().any(|n| n.as_string() == Some(name)),
            None => true,
        }
    }
}
