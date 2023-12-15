// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! The diff pipe: trivial changes, dates, long texts, objects, and arrays (with the longest common subsequence
//! and move detection). Children are diffed after their parent's filters ran, in the order they were found, as
//! the JavaScript pipe does; errors therefore surface in the same order.

use crate::diffpatcher::DiffPatcher;
use crate::js::number::number_to_string;
use crate::js::{get_prop, JsError, JsString, Object, Value};

const ARRAY_MOVE: f64 = 3.0;
const TEXT_DIFF: f64 = 2.0;

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        other => other.type_of(),
    }
}

fn array(items: Vec<Value>) -> Value {
    Value::Array(items)
}

fn index_key(index: usize) -> JsString {
    JsString::from(index.to_string())
}

/// What array item matching needs: the object hash, and whether to match objects by position.
struct MatchContext<'a> {
    object_hash: Option<&'a JsString>,
    match_by_position: Value,
}

impl MatchContext<'_> {
    /// `matchItems`: whether `array1[index1]` and `array2[index2]` are the same item.
    fn matches(&self, array1: &[Value], array2: &[Value], index1: usize, index2: usize) -> Result<bool, JsError> {
        let value1 = array1.get(index1).cloned().unwrap_or_default();
        let value2 = array2.get(index2).cloned().unwrap_or_default();
        if value1.strict_equals(&value2) {
            return Ok(true);
        }
        if value1.type_of() != "object" || value2.type_of() != "object" {
            return Ok(false);
        }
        let Some(name) = self.object_hash else {
            return Ok(self.match_by_position.truthy() && index1 == index2);
        };
        let hash1 = get_prop(&value1, name)?;
        if hash1.is_undefined() {
            return Ok(false);
        }
        let hash2 = get_prop(&value2, name)?;
        if hash2.is_undefined() {
            return Ok(false);
        }
        Ok(hash1.strict_equals(&hash2))
    }
}

/// The longest common subsequence of two arrays: the matched indices in each.
fn lcs(array1: &[Value], array2: &[Value], context: &MatchContext) -> Result<(Vec<usize>, Vec<usize>), JsError> {
    let (len1, len2) = (array1.len(), array2.len());
    let mut matrix = vec![vec![0usize; len2 + 1]; len1 + 1];
    for x in 1..=len1 {
        for y in 1..=len2 {
            matrix[x][y] = if context.matches(array1, array2, x - 1, y - 1)? {
                matrix[x - 1][y - 1] + 1
            } else {
                matrix[x - 1][y].max(matrix[x][y - 1])
            };
        }
    }
    let (mut index1, mut index2) = (len1, len2);
    let (mut indices1, mut indices2) = (Vec::new(), Vec::new());
    while index1 != 0 && index2 != 0 {
        if context.matches(array1, array2, index1 - 1, index2 - 1)? {
            indices1.insert(0, index1 - 1);
            indices2.insert(0, index2 - 1);
            index1 -= 1;
            index2 -= 1;
        } else if matrix[index1][index2 - 1] > matrix[index1 - 1][index2] {
            index2 -= 1;
        } else {
            index1 -= 1;
        }
    }
    Ok((indices1, indices2))
}

impl DiffPatcher {
    /// `diff(left, right)`: the delta from `left` to `right` (`undefined` when they are equal).
    pub fn diff(&self, left: &Value, right: &Value) -> Result<Value, JsError> {
        if let Some(delta) = Self::diff_trivial(left, right)? {
            return Ok(delta);
        }
        if let Some(delta) = Self::diff_dates(left, right) {
            return Ok(delta);
        }
        match (left, right) {
            (Value::String(a), Value::String(b)) => self.diff_texts(a, b),
            (Value::Object(a), _) => self.diff_objects(a, right),
            (Value::Array(a), Value::Array(b)) => self.diff_arrays(a, b),
            _ => Ok(array(vec![left.clone(), right.clone()])),
        }
    }

    /// The trivial filter: equal values, additions, deletions, functions, type changes, booleans and numbers.
    /// `Some(delta)` ends the pipe with that delta (an undefined delta means "no change"); `None` passes the pair on.
    fn diff_trivial(left: &Value, right: &Value) -> Result<Option<Value>, JsError> {
        if left.strict_equals(right) {
            return Ok(Some(Value::Undefined));
        }
        if left.is_undefined() {
            if matches!(right, Value::Function(..)) {
                return Err(JsError::error("functions are not supported"));
            }
            return Ok(Some(array(vec![right.clone()])));
        }
        if right.is_undefined() {
            return Ok(Some(array(vec![left.clone(), Value::Number(0.0), Value::Number(0.0)])));
        }
        if matches!(left, Value::Function(..)) || matches!(right, Value::Function(..)) {
            return Err(JsError::error("functions are not supported"));
        }
        let left_type = type_name(left);
        if left_type != type_name(right) || left_type == "boolean" || left_type == "number" {
            return Ok(Some(array(vec![left.clone(), right.clone()])));
        }
        let left_is_array = left_type == "object" && matches!(left, Value::Array(_));
        let right_is_array = left_type == "object" && matches!(right, Value::Array(_));
        if left_is_array != right_is_array {
            return Ok(Some(array(vec![left.clone(), right.clone()])));
        }
        Ok(None)
    }

    /// The dates filter: two dates are a change unless they are the same instant, and a date against anything else
    /// is a change. `None` when neither side is a date.
    fn diff_dates(left: &Value, right: &Value) -> Option<Value> {
        match (left, right) {
            (Value::Date(a), Value::Date(b)) => Some(if a != b {
                array(vec![left.clone(), right.clone()])
            } else {
                Value::Undefined
            }),
            (Value::Date(_), _) | (_, Value::Date(_)) => Some(array(vec![left.clone(), right.clone()])),
            _ => None,
        }
    }

    fn diff_texts(&self, left: &JsString, right: &JsString) -> Result<Value, JsError> {
        let min_length = self.options.text_min_length().to_number();
        if (left.len() as f64) < min_length || (right.len() as f64) < min_length {
            return Ok(array(vec![Value::String(left.clone()), Value::String(right.clone())]));
        }
        let patches = self.dmp.patch_make(left.units(), right.units())?;
        let text = crate::dmp::DiffMatchPatch::patch_to_text(&patches)?;
        Ok(array(vec![
            Value::String(text),
            Value::Number(0.0),
            Value::Number(TEXT_DIFF),
        ]))
    }

    fn diff_objects(&self, left: &Object, right: &Value) -> Result<Value, JsError> {
        let left_value = Value::Object(left.clone());
        let mut children: Vec<(JsString, Value, Value)> = Vec::new();
        for (name, value) in left.iter() {
            if !self.options.includes_property(name) {
                continue;
            }
            children.push((name.clone(), value.clone(), right.get(name)));
        }
        for name in right.for_in_keys() {
            if !self.options.includes_property(&name) {
                continue;
            }
            if left_value.get(&name).is_undefined() {
                children.push((name.clone(), Value::Undefined, right.get(&name)));
            }
        }
        if children.is_empty() {
            return Ok(Value::Undefined);
        }
        let mut result: Option<Object> = None;
        for (name, child_left, child_right) in children {
            let delta = self.diff(&child_left, &child_right)?;
            if !delta.is_undefined() {
                result.get_or_insert_with(Object::new).set(name, delta);
            }
        }
        Ok(result.map_or(Value::Undefined, Value::Object))
    }

    fn diff_arrays(&self, array1: &[Value], array2: &[Value]) -> Result<Value, JsError> {
        let mut context = MatchContext {
            object_hash: self.options.object_hash.as_ref(),
            match_by_position: self.options.match_by_position.clone(),
        };
        let (len1, len2) = (array1.len(), array2.len());
        let mut children: Vec<(usize, usize)> = Vec::new();

        if len1 > 0 && len2 > 0 && context.object_hash.is_none() && !matches!(context.match_by_position, Value::Bool(_))
        {
            let by_ref = (0..len1).any(|i1| (0..len2).any(|i2| i1 != i2 && array1[i1].strict_equals(&array2[i2])));
            context.match_by_position = Value::Bool(!by_ref);
        }

        let mut common_head = 0;
        while common_head < len1 && common_head < len2 && context.matches(array1, array2, common_head, common_head)? {
            children.push((common_head, common_head));
            common_head += 1;
        }
        let mut common_tail = 0;
        while common_tail + common_head < len1
            && common_tail + common_head < len2
            && context.matches(array1, array2, len1 - 1 - common_tail, len2 - 1 - common_tail)?
        {
            children.push((len1 - 1 - common_tail, len2 - 1 - common_tail));
            common_tail += 1;
        }

        let mut result: Option<Object> = None;
        if common_head + common_tail == len1 {
            if len1 != len2 {
                let mut delta = Object::new();
                delta.set(JsString::from("_t"), Value::str("a"));
                for (index, item) in array2.iter().enumerate().take(len2 - common_tail).skip(common_head) {
                    delta.set(index_key(index), array(vec![item.clone()]));
                }
                result = Some(delta);
            }
        } else if common_head + common_tail == len2 {
            let mut delta = Object::new();
            delta.set(JsString::from("_t"), Value::str("a"));
            for (index, item) in array1.iter().enumerate().take(len1 - common_tail).skip(common_head) {
                delta.set(
                    JsString::from(format!("_{index}")),
                    array(vec![item.clone(), Value::Number(0.0), Value::Number(0.0)]),
                );
            }
            result = Some(delta);
        } else {
            let trimmed1 = &array1[common_head..len1 - common_tail];
            let trimmed2 = &array2[common_head..len2 - common_tail];
            let (indices1, indices2) = lcs(trimmed1, trimmed2, &context)?;
            let mut delta = Object::new();
            delta.set(JsString::from("_t"), Value::str("a"));
            let mut removed: Vec<usize> = Vec::new();
            let mut removed_values: Vec<(usize, Value)> = Vec::new();
            for (index, item) in array1.iter().enumerate().take(len1 - common_tail).skip(common_head) {
                if !indices1.contains(&(index - common_head)) {
                    removed_values.push((index, item.clone()));
                    removed.push(index);
                }
            }
            let mut entries: Vec<(usize, Vec<Value>)> = removed_values
                .into_iter()
                .map(|(i, v)| (i, vec![v, Value::Number(0.0), Value::Number(0.0)]))
                .collect();
            let detect_move = self.options.detect_move();
            let include_value_on_move = self.options.include_value_on_move();
            let removed_length = removed.len();
            let mut added: Vec<(usize, Value)> = Vec::new();
            for (index, item) in array2.iter().enumerate().take(len2 - common_tail).skip(common_head) {
                match indices2.iter().position(|&i| i == index - common_head) {
                    None => {
                        let mut is_move = false;
                        if detect_move && removed_length > 0 {
                            for remove_index in 0..removed_length {
                                let Some(&index1) = removed.get(remove_index) else {
                                    continue;
                                };
                                if context.matches(trimmed1, trimmed2, index1 - common_head, index - common_head)? {
                                    if let Some(entry) = entries.iter_mut().find(|(i, _)| *i == index1) {
                                        entry.1.truncate(1);
                                        entry.1.push(Value::Number(index as f64));
                                        entry.1.push(Value::Number(ARRAY_MOVE));
                                        if !include_value_on_move {
                                            entry.1[0] = Value::str("");
                                        }
                                    }
                                    children.push((index1, index));
                                    removed.remove(remove_index);
                                    is_move = true;
                                    break;
                                }
                            }
                        }
                        if !is_move {
                            added.push((index, item.clone()));
                        }
                    }
                    Some(position) => {
                        children.push((indices1[position] + common_head, indices2[position] + common_head));
                    }
                }
            }
            for (index, value) in added {
                delta.set(index_key(index), array(vec![value]));
            }
            for (index, entry) in entries {
                delta.set(JsString::from(format!("_{index}")), array(entry));
            }
            result = Some(delta);
        }

        let mut collected = result;
        let mut any_child = false;
        for (index1, index2) in children {
            let delta = self.diff(&array1[index1], &array2[index2])?;
            if !delta.is_undefined() {
                any_child = true;
                collected.get_or_insert_with(Object::new).set(index_key(index2), delta);
            }
        }
        if let Some(delta) = collected.as_mut() {
            if any_child {
                delta.set(JsString::from("_t"), Value::str("a"));
            }
        }
        Ok(collected.map_or(Value::Undefined, Value::Object))
    }
}

/// The `_N` key of an original array index, as a template literal writes it.
pub fn removed_key(index: f64) -> JsString {
    JsString::from(format!("_{}", number_to_string(index)))
}
