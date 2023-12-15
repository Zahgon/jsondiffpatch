// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! The patch pipe. A delta is any JSON value here: the library reads it with JavaScript property access, so a
//! malformed delta fails (or quietly does something odd) exactly where the JavaScript library would.

use crate::diff::removed_key;
use crate::diffpatcher::DiffPatcher;
use crate::dmp::DiffMatchPatch;
use crate::js::number::{number_to_string, parse_int};
use crate::js::sort::sort_by;
use crate::js::{
    delete_prop, get_named, get_prop, not_a_function, number_key, relative_index, set_prop, JsError, JsString, Object,
    Value,
};

const ARRAY_MOVE: f64 = 3.0;
const TEXT_DIFF: f64 = 2.0;

fn is_number(value: &Value, n: f64) -> bool {
    value.strict_equals(&Value::Number(n))
}

/// `array.splice(start, 1)[0]`.
fn splice_remove(target: &mut Value, start: f64) -> Result<Value, JsError> {
    match target {
        Value::Array(items) => {
            let at = relative_index(&Value::Number(start), items.len());
            Ok(if at < items.len() {
                items.remove(at)
            } else {
                Value::Undefined
            })
        }
        other => Err(not_a_function(other, "array", "splice")),
    }
}

/// `array.splice(start, 0, value)`.
fn splice_insert(target: &mut Value, start: &Value, value: Value) -> Result<(), JsError> {
    match target {
        Value::Array(items) => {
            let at = relative_index(start, items.len());
            items.insert(at, value);
            Ok(())
        }
        other => Err(not_a_function(other, "array", "splice")),
    }
}

/// `textline.split('\n')` as `patch_fromText` calls it, with the errors of a non-string.
pub fn split_error(value: &Value, name: &str) -> JsError {
    not_a_function(value, name, "split")
}

impl DiffPatcher {
    /// `patch(left, delta)`: the patched value. The left value is consumed, as the library patches in place.
    pub fn patch(&self, left: Value, delta: &Value) -> Result<Value, JsError> {
        match self.patch_value(left, delta)? {
            Some(result) => Ok(result),
            None => Err(JsError::error("patch failed")),
        }
    }

    /// One patch context: `None` when no filter produced a result.
    fn patch_value(&self, left: Value, delta: &Value) -> Result<Option<Value>, JsError> {
        if delta.is_undefined() {
            return Ok(Some(left));
        }
        if let Value::Array(items) = delta {
            match items.len() {
                1 => return Ok(Some(items[0].clone())),
                2 => return Ok(Some(items[1].clone())),
                3 if is_number(&items[2], 0.0) => return Ok(Some(Value::Undefined)),
                _ => {}
            }
            if items.get(2).is_some_and(|v| is_number(v, TEXT_DIFF)) {
                return self.patch_text(left, &items[0]).map(Some);
            }
            return Ok(None);
        }
        let array_type = get_named(delta, "_t")?;
        if !array_type.truthy() {
            return self.patch_object(left, delta);
        }
        if array_type.strict_equals(&Value::str("a")) {
            if let Value::Object(object) = delta {
                return self.patch_array(left, object).map(Some);
            }
        }
        Ok(None)
    }

    fn patch_text(&self, left: Value, textline: &Value) -> Result<Value, JsError> {
        let patches = if !textline.truthy() {
            Vec::new()
        } else {
            match textline {
                Value::String(text) => DiffMatchPatch::patch_from_text(text)?,
                other => return Err(split_error(other, "textline")),
            }
        };
        if patches.is_empty() {
            return Ok(left);
        }
        let (text, _) = self.dmp.patch_apply(&patches, &left.to_js_string())?;
        Ok(Value::String(text))
    }

    fn patch_object(&self, mut left: Value, delta: &Value) -> Result<Option<Value>, JsError> {
        let mut children: Vec<(JsString, Value, Value)> = Vec::new();
        for name in delta.for_in_keys() {
            let child_left = get_prop(&left, &name)?;
            children.push((name.clone(), child_left, delta.get(&name)));
        }
        if children.is_empty() {
            return Ok(None);
        }
        let mut results = Vec::with_capacity(children.len());
        for (name, child_left, child_delta) in children {
            let result = self.patch_value(child_left, &child_delta)?.unwrap_or_default();
            results.push((name, result));
        }
        for (name, result) in results {
            if left.has_own(&name) && result.is_undefined() {
                delete_prop(&mut left, &name)?;
            } else if result.is_object() || !left.get(&name).strict_equals(&result) {
                set_prop(&mut left, &name, result)?;
            }
        }
        Ok(Some(left))
    }

    fn patch_array(&self, mut left: Value, delta: &Object) -> Result<Value, JsError> {
        let mut to_remove: Vec<f64> = Vec::new();
        let mut to_insert: Vec<(Value, Value)> = Vec::new();
        let mut to_modify: Vec<(f64, Value)> = Vec::new();
        for (key, item) in delta.iter() {
            if key.eq_str("_t") {
                continue;
            }
            if key.unit_at(0) == Some(u16::from(b'_')) {
                let kind = get_named(item, "2")?;
                if is_number(&kind, 0.0) || is_number(&kind, ARRAY_MOVE) {
                    to_remove.push(parse_int(&key.slice_from(1)));
                } else {
                    let mut message = JsString::from(
                        "only removal or move can be applied at original array indices, invalid diff type: ",
                    );
                    message.push_js(&kind.to_js_string());
                    return Err(JsError::error(message));
                }
            } else if is_number(&get_named(item, "length")?, 1.0) {
                to_insert.push((Value::Number(parse_int(key)), get_named(item, "0")?));
            } else {
                to_modify.push((parse_int(key), item.clone()));
            }
        }

        sort_by(&mut to_remove, |a, b| a - b);
        for &index in to_remove.iter().rev() {
            let index_diff = delta.get(&removed_key(index)).cloned().unwrap_or_default();
            let removed = splice_remove(&mut left, index)?;
            if is_number(&get_named(&index_diff, "2")?, ARRAY_MOVE) {
                to_insert.push((get_named(&index_diff, "1")?, removed));
            }
        }

        sort_by(&mut to_insert, |a, b| a.0.to_number() - b.0.to_number());
        for (index, value) in to_insert {
            splice_insert(&mut left, &index, value)?;
        }

        if to_modify.is_empty() {
            return Ok(left);
        }
        let mut children = Vec::with_capacity(to_modify.len());
        for (index, child_delta) in to_modify {
            children.push((index, get_prop(&left, &number_key(index))?, child_delta));
        }
        let mut results = Vec::with_capacity(children.len());
        for (index, child_left, child_delta) in children {
            results.push((index, self.patch_value(child_left, &child_delta)?.unwrap_or_default()));
        }
        for (index, result) in results {
            set_prop(&mut left, &JsString::from(number_to_string(index)), result)?;
        }
        Ok(left)
    }
}
