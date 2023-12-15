// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! The reverse pipe: a delta that undoes a delta. Array deltas need their indices remapped, which follows the
//! library's own arithmetic, including how it treats keys and values that are not numbers.

use crate::diffpatcher::DiffPatcher;
use crate::js::number::parse_int;
use crate::js::{get_named, not_a_function, number_key, JsError, JsString, Object, Value};

const ARRAY_MOVE: f64 = 3.0;
const TEXT_DIFF: f64 = 2.0;

fn is_number(value: &Value, n: f64) -> bool {
    value.strict_equals(&Value::Number(n))
}

/// The result of one reverse context: its result (if any filter produced one) and the new name a reversed move
/// asks its parent to store it under.
struct Reversed {
    result: Option<Value>,
    new_name: Option<JsString>,
}

impl Reversed {
    fn value(result: Value) -> Self {
        Reversed {
            result: Some(result),
            new_name: None,
        }
    }

    fn none() -> Self {
        Reversed {
            result: None,
            new_name: None,
        }
    }
}

/// Parses `@@ -A,B +C,D @@` with any runs of spaces, as the reverse filter's header expression does.
fn parse_reverse_header(line: &[u16]) -> Option<[Vec<u16>; 4]> {
    let mut i = 0;
    let literal = |i: &mut usize, unit: u8| -> bool {
        if line.get(*i) == Some(&u16::from(unit)) {
            *i += 1;
            true
        } else {
            false
        }
    };
    let spaces = |i: &mut usize| -> bool {
        let start = *i;
        while line.get(*i) == Some(&0x20) {
            *i += 1;
        }
        *i > start
    };
    let digits = |i: &mut usize| -> Option<Vec<u16>> {
        let start = *i;
        while line.get(*i).is_some_and(|u| (0x30..=0x39).contains(u)) {
            *i += 1;
        }
        (*i > start).then(|| line[start..*i].to_vec())
    };
    if !(literal(&mut i, b'@') && literal(&mut i, b'@') && spaces(&mut i) && literal(&mut i, b'-')) {
        return None;
    }
    let a = digits(&mut i)?;
    if !literal(&mut i, b',') {
        return None;
    }
    let b = digits(&mut i)?;
    if !(spaces(&mut i) && literal(&mut i, b'+')) {
        return None;
    }
    let c = digits(&mut i)?;
    if !literal(&mut i, b',') {
        return None;
    }
    let d = digits(&mut i)?;
    if !(spaces(&mut i) && literal(&mut i, b'@') && literal(&mut i, b'@')) || i != line.len() {
        return None;
    }
    Some([a, b, c, d])
}

/// The reverse of a text diff: hunk headers swapped, insertions and deletions exchanged.
fn text_delta_reverse(delta: &Value) -> Result<JsString, JsError> {
    let Value::String(text) = delta else {
        return Err(not_a_function(delta, "delta", "split"));
    };
    let mut lines: Vec<Vec<u16>> = text.split_unit(0x0A).into_iter().map(JsString::into_units).collect();
    for i in 0..lines.len() {
        let line = lines[i].clone();
        match line.first() {
            Some(0x40) => {
                let Some([a, b, c, d]) = parse_reverse_header(&line) else {
                    return Err(JsError::type_error("Cannot read properties of null (reading '3')"));
                };
                let mut header: Vec<u16> = "@@ -".encode_utf16().collect();
                header.extend(c);
                header.push(u16::from(b','));
                header.extend(d);
                header.extend(" +".encode_utf16());
                header.extend(a);
                header.push(u16::from(b','));
                header.extend(b);
                header.extend(" @@".encode_utf16());
                lines[i] = header;
            }
            Some(0x2B) => {
                let mut flipped = vec![u16::from(b'-')];
                flipped.extend_from_slice(&line[1..]);
                lines[i] = flipped;
                if i == 0 {
                    return Err(JsError::type_error(
                        "Cannot read properties of undefined (reading 'slice')",
                    ));
                }
                if lines[i - 1].first() == Some(&u16::from(b'+')) {
                    lines.swap(i, i - 1);
                }
            }
            Some(0x2D) => {
                let mut flipped = vec![u16::from(b'+')];
                flipped.extend_from_slice(&line[1..]);
                lines[i] = flipped;
            }
            _ => {}
        }
    }
    let parts: Vec<JsString> = lines.into_iter().map(JsString::from_units).collect();
    Ok(JsString::join(&parts, "\n"))
}

/// `reverseArrayDeltaIndex`: where an item of a reversed array delta goes.
fn reverse_array_delta_index(delta: &Object, index: &JsString, item_delta: &Value) -> JsString {
    if index.unit_at(0) == Some(u16::from(b'_')) {
        return number_key(parse_int(&index.slice_from(1)));
    }
    if let Value::Array(items) = item_delta {
        if items.get(2).is_some_and(|v| is_number(v, 0.0)) {
            let mut name = JsString::from("_");
            name.push_js(index);
            return name;
        }
    }
    let target = Value::String(index.clone()).to_number();
    let mut reverse_index = target;
    for (delta_index, delta_item) in delta.iter() {
        let Value::Array(items) = delta_item else {
            continue;
        };
        let kind = items.get(2).cloned().unwrap_or_default();
        if is_number(&kind, ARRAY_MOVE) {
            let move_from = parse_int(&delta_index.slice_from(1));
            let move_to = items.get(1).cloned().unwrap_or_default();
            if move_to.strict_equals(&Value::Number(target)) {
                return number_key(move_from);
            }
            let move_to = move_to.to_number();
            if move_from <= reverse_index && move_to > reverse_index {
                reverse_index += 1.0;
            } else if move_from >= reverse_index && move_to < reverse_index {
                reverse_index -= 1.0;
            }
        } else if is_number(&kind, 0.0) {
            let delete_index = parse_int(&delta_index.slice_from(1));
            if delete_index <= reverse_index {
                reverse_index += 1.0;
            }
        } else if items.len() == 1 && parse_int(delta_index) <= reverse_index {
            reverse_index -= 1.0;
        }
    }
    number_key(reverse_index)
}

impl DiffPatcher {
    /// `reverse(delta)`: the delta that undoes `delta`.
    pub fn reverse(&self, delta: &Value) -> Result<Value, JsError> {
        match self.reverse_value(delta, None)?.result {
            Some(result) => Ok(result),
            None => Err(JsError::error("reverse failed")),
        }
    }

    /// `unpatch(right, delta)`: `patch(right, reverse(delta))`.
    pub fn unpatch(&self, right: Value, delta: &Value) -> Result<Value, JsError> {
        let reversed = self.reverse(delta)?;
        self.patch(right, &reversed)
    }

    fn reverse_value(&self, delta: &Value, child_name: Option<&JsString>) -> Result<Reversed, JsError> {
        if delta.is_undefined() {
            return Ok(Reversed::value(Value::Undefined));
        }
        if let Value::Array(items) = delta {
            if let Some(reversed) = Self::reverse_trivial(items) {
                return Ok(reversed);
            }
            let kind = items.get(2).cloned().unwrap_or_default();
            if is_number(&kind, TEXT_DIFF) {
                return Self::reverse_text(items);
            }
            if is_number(&kind, ARRAY_MOVE) {
                return Self::reverse_move(items, child_name);
            }
            return Ok(Reversed::none());
        }
        let array_type = get_named(delta, "_t")?;
        if !array_type.truthy() {
            return self.reverse_object(delta);
        }
        let Value::Object(array_delta) = delta else {
            return Ok(Reversed::none());
        };
        if !array_type.strict_equals(&Value::str("a")) {
            return Ok(Reversed::none());
        }
        self.reverse_array(array_delta)
    }

    /// The trivial filter: an addition (`[new]`) becomes a deletion, a modification (`[old, new]`) swaps its sides
    /// and a deletion (`[old, 0, 0]`) becomes an addition. `None` for the other leaf deltas.
    fn reverse_trivial(items: &[Value]) -> Option<Reversed> {
        match items.len() {
            1 => Some(Reversed::value(Value::Array(vec![
                items[0].clone(),
                Value::Number(0.0),
                Value::Number(0.0),
            ]))),
            2 => Some(Reversed::value(Value::Array(vec![items[1].clone(), items[0].clone()]))),
            3 if is_number(&items[2], 0.0) => Some(Reversed::value(Value::Array(vec![items[0].clone()]))),
            _ => None,
        }
    }

    /// The texts filter: a text diff reverses into the text diff that undoes it.
    fn reverse_text(items: &[Value]) -> Result<Reversed, JsError> {
        let text = text_delta_reverse(&items[0])?;
        Ok(Reversed::value(Value::Array(vec![
            Value::String(text),
            Value::Number(0.0),
            Value::Number(TEXT_DIFF),
        ])))
    }

    /// The arrays filter's moves: a move to `target` reverses into a move back from it, renaming the item to `_target`.
    fn reverse_move(items: &[Value], child_name: Option<&JsString>) -> Result<Reversed, JsError> {
        let target = items.get(1).cloned().unwrap_or_default();
        let mut new_name = JsString::from("_");
        new_name.push_js(&target.to_js_string());
        let Some(name) = child_name else {
            return Err(JsError::type_error(
                "Cannot read properties of undefined (reading 'substring')",
            ));
        };
        let from = parse_int(&name.slice_from(1));
        Ok(Reversed {
            result: Some(Value::Array(vec![
                items[0].clone(),
                Value::Number(from),
                Value::Number(ARRAY_MOVE),
            ])),
            new_name: Some(new_name),
        })
    }

    /// The objects filter: every member delta is reversed in place.
    fn reverse_object(&self, delta: &Value) -> Result<Reversed, JsError> {
        let names = delta.for_in_keys();
        if names.is_empty() {
            return Ok(Reversed::none());
        }
        let mut reversed = Object::new();
        for name in names {
            let result = self
                .reverse_value(&delta.get(&name), Some(&name))?
                .result
                .unwrap_or_default();
            // An undefined result still becomes an own member where a plain object inherits a value.
            if !result.is_undefined() || !Value::Object(Object::new()).get(&name).is_undefined() {
                reversed.set(name, result);
            }
        }
        Ok(Reversed::value(Value::Object(reversed)))
    }

    /// The arrays filter: item deltas are reversed and re-keyed to the indices they had before the change.
    fn reverse_array(&self, array_delta: &Object) -> Result<Reversed, JsError> {
        let names: Vec<JsString> = array_delta.keys().into_iter().filter(|n| !n.eq_str("_t")).collect();
        if names.is_empty() {
            return Ok(Reversed::none());
        }
        let mut children = Vec::with_capacity(names.len());
        for name in names {
            let item = array_delta.get(&name).cloned().unwrap_or_default();
            let child = self.reverse_value(&item, Some(&name))?;
            children.push((name, child));
        }
        let mut reversed = Object::new();
        reversed.set(JsString::from("_t"), Value::str("a"));
        for (name, child) in children {
            let result = child.result.unwrap_or_default();
            let key = match child.new_name {
                Some(new_name) => new_name,
                None => reverse_array_delta_index(array_delta, &name, &result),
            };
            let current = reversed.get(&key).cloned().unwrap_or_default();
            if result.is_object() || !current.strict_equals(&result) {
                reversed.set(key, result);
            }
        }
        Ok(Reversed::value(Value::Object(reversed)))
    }
}
