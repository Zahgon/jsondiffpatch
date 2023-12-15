// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! Delta formatters and the walk they share. The walk follows the JavaScript `BaseFormatter`: it visits delta
//! members and unchanged left members in sorted order, places moved items at their destinations, and lets a
//! formatter's own error output take the place of a node it could not format.

pub mod annotated;
pub mod console;
pub mod html;
pub mod jsonpatch;

use crate::js::number::parse_int;
use crate::js::sort::{compare_units, sort_by};
use crate::js::{get_named, not_a_function, number_key, JsError, JsString, Object, Value};

/// What a delta (or the absence of one) means at a place in the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeltaType {
    Unchanged,
    MoveDestination,
    Added,
    Modified,
    Deleted,
    TextDiff,
    Moved,
    Node,
    Unknown,
}

impl DeltaType {
    /// The type's name, as formatters print it in class names.
    pub fn name(self) -> &'static str {
        match self {
            DeltaType::Unchanged => "unchanged",
            DeltaType::MoveDestination => "movedestination",
            DeltaType::Added => "added",
            DeltaType::Modified => "modified",
            DeltaType::Deleted => "deleted",
            DeltaType::TextDiff => "textdiff",
            DeltaType::Moved => "moved",
            DeltaType::Node => "node",
            DeltaType::Unknown => "unknown",
        }
    }

    /// The type of `delta`; `moved_from` tells a move destination from an unchanged member.
    pub fn of(delta: &Value, moved_from: Option<&Value>) -> DeltaType {
        match delta {
            Value::Undefined => {
                if moved_from.is_some() {
                    DeltaType::MoveDestination
                } else {
                    DeltaType::Unchanged
                }
            }
            Value::Array(items) => {
                let kind = |n: f64| items.get(2).is_some_and(|v| v.strict_equals(&Value::Number(n)));
                match items.len() {
                    1 => DeltaType::Added,
                    2 => DeltaType::Modified,
                    3 if kind(0.0) => DeltaType::Deleted,
                    3 if kind(2.0) => DeltaType::TextDiff,
                    3 if kind(3.0) => DeltaType::Moved,
                    _ => DeltaType::Unknown,
                }
            }
            Value::Null | Value::Object(_) | Value::Date(_) => DeltaType::Node,
            _ => DeltaType::Unknown,
        }
    }
}

/// The shape of a node delta: `"array"` for array deltas, `"object"` otherwise, `""` for non-nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeType {
    None,
    Object,
    Array,
}

impl NodeType {
    pub fn name(self) -> &'static str {
        match self {
            NodeType::None => "",
            NodeType::Object => "object",
            NodeType::Array => "array",
        }
    }
}

/// A place being formatted below the root: its delta key, and the key into the left value (a number for array
/// deltas, the delta key otherwise).
pub struct Place<'a> {
    pub key: &'a JsString,
    pub left_key: &'a Value,
    pub is_last: bool,
}

/// The hooks a formatter provides to [`format`].
pub trait Formatter {
    /// Whether move destinations are listed as members when there is no left value (`includeMoveDestinations`
    /// is not `false`).
    fn include_move_destinations(&self) -> bool;

    fn root_begin(&mut self, delta_type: DeltaType, node_type: NodeType) -> Result<(), JsError>;
    fn root_end(&mut self, delta_type: DeltaType, node_type: NodeType) -> Result<(), JsError>;
    fn node_begin(&mut self, place: &Place, delta_type: DeltaType, node_type: NodeType) -> Result<(), JsError>;
    fn node_end(&mut self, place: &Place, delta_type: DeltaType, node_type: NodeType) -> Result<(), JsError>;

    /// Output before and after the members of a node delta.
    fn node_open(&mut self, _node_type: NodeType) -> Result<(), JsError> {
        Ok(())
    }
    fn node_close(&mut self, _node_type: NodeType) -> Result<(), JsError> {
        Ok(())
    }

    /// Formats a delta that is not a node (nor unknown).
    fn format_leaf(
        &mut self,
        delta_type: DeltaType,
        delta: &Value,
        left: &Value,
        place: Option<&Place>,
    ) -> Result<(), JsError>;

    /// Output for an error raised while formatting a node.
    fn format_error(&mut self, error: &JsError);
}

/// Formats `delta` (with the `left` value it applies to, when known) with `formatter`.
pub fn format<F: Formatter>(formatter: &mut F, delta: &Value, left: &Value) -> Result<(), JsError> {
    recurse(formatter, delta, left, None, None)
}

fn recurse<F: Formatter>(
    formatter: &mut F,
    delta: &Value,
    left: &Value,
    place: Option<&Place>,
    moved_from: Option<&Value>,
) -> Result<(), JsError> {
    let left_value = match moved_from {
        Some(value) if delta.truthy() => value,
        _ => left,
    };
    if delta.is_undefined() && place.is_none() {
        return Ok(());
    }
    let delta_type = DeltaType::of(delta, moved_from);
    let node_type = if delta_type == DeltaType::Node {
        if get_named(delta, "_t")?.strict_equals(&Value::str("a")) {
            NodeType::Array
        } else {
            NodeType::Object
        }
    } else {
        NodeType::None
    };
    match place {
        Some(place) => formatter.node_begin(place, delta_type, node_type)?,
        None => formatter.root_begin(delta_type, node_type)?,
    }
    let formatted = match delta_type {
        DeltaType::Unknown => Err(JsError::error("cannot format delta type: unknown")),
        DeltaType::Node => format_node(formatter, delta, left_value, node_type),
        other => formatter.format_leaf(other, delta, left_value, place),
    };
    if let Err(error) = formatted {
        formatter.format_error(&error);
    }
    match place {
        Some(place) => formatter.node_end(place, delta_type, node_type),
        None => formatter.root_end(delta_type, node_type),
    }
}

fn format_node<F: Formatter>(
    formatter: &mut F,
    delta: &Value,
    left: &Value,
    node_type: NodeType,
) -> Result<(), JsError> {
    formatter.node_open(node_type)?;
    let include_move_destinations = formatter.include_move_destinations();
    for_each_delta_key(
        delta,
        left,
        include_move_destinations,
        |key, left_key, moved_from, is_last| {
            let child_left = if left.truthy() {
                left.get(&property_key(left_key))
            } else {
                Value::Undefined
            };
            let place = Place { key, left_key, is_last };
            recurse(formatter, &delta.get(key), &child_left, Some(&place), moved_from)
        },
    )?;
    formatter.node_close(node_type)
}

/// `ToPropertyKey` of a left key.
fn property_key(key: &Value) -> JsString {
    match key {
        Value::Number(n) => number_key(*n),
        other => other.to_js_string(),
    }
}

fn array_key_sort_number(key: &JsString) -> f64 {
    if key.eq_str("_t") {
        -1.0
    } else if key.unit_at(0) == Some(u16::from(b'_')) {
        parse_int(&key.slice_from(1))
    } else {
        parse_int(key) + 0.1
    }
}

fn trim_underscore(key: &JsString) -> JsString {
    if key.unit_at(0) == Some(u16::from(b'_')) {
        key.slice_from(1)
    } else {
        key.clone()
    }
}

/// Calls `visit(key, left_key, moved_from, is_last)` for every member to format, in order: the delta's own
/// keys, unchanged members of `left`, and (when asked and there is no left value) move destinations.
fn for_each_delta_key(
    delta: &Value,
    left: &Value,
    include_move_destinations: bool,
    mut visit: impl FnMut(&JsString, &Value, Option<&Value>, bool) -> Result<(), JsError>,
) -> Result<(), JsError> {
    let mut keys: Vec<JsString> = match delta {
        Value::Object(object) => object.keys(),
        _ => Vec::new(),
    };
    let array_keys = delta.get(&JsString::from("_t")).strict_equals(&Value::str("a"));
    let mut move_destinations = Object::new();
    if !left.is_undefined() {
        for name in left.for_in_keys() {
            if left.has_own(&name) && delta.get(&name).is_undefined() {
                let mut underscored = JsString::from("_");
                underscored.push_js(&name);
                if !array_keys || delta.get(&underscored).is_undefined() {
                    keys.push(name);
                }
            }
        }
    }
    if let Value::Object(object) = delta {
        for (name, value) in object.iter() {
            let Value::Array(items) = value else {
                continue;
            };
            if !items.get(2).is_some_and(|v| v.strict_equals(&Value::Number(3.0))) {
                continue;
            }
            let target = items.get(1).cloned().unwrap_or_default();
            let from_value = if left.truthy() {
                left.get(&number_key(parse_int(&name.substring(1.0, f64::INFINITY))))
            } else {
                left.clone()
            };
            move_destinations.set(target.to_js_string(), from_value);
            if include_move_destinations && left.is_undefined() && delta.get(&target.to_js_string()).is_undefined() {
                if matches!(target, Value::Undefined | Value::Null) {
                    return Err(not_a_function(&target, "value", "toString"));
                }
                keys.push(target.to_js_string());
            }
        }
    }
    if array_keys {
        sort_by(&mut keys, |a, b| array_key_sort_number(a) - array_key_sort_number(b));
    } else {
        sort_by(&mut keys, |a, b| compare_units(a.units(), b.units()));
    }
    let length = keys.len();
    for (index, key) in keys.iter().enumerate() {
        if array_keys && key.eq_str("_t") {
            continue;
        }
        let left_key = if array_keys {
            Value::Number(parse_int(&trim_underscore(key)))
        } else {
            Value::String(key.clone())
        };
        let lookup = property_key(&left_key);
        let moved_from = match move_destinations.get(&lookup) {
            Some(value) => Some(value.clone()),
            None if lookup.eq_str("__proto__") || crate::js::value::is_inherited_name(&lookup) => {
                Some(Value::Undefined)
            }
            None => None,
        };
        visit(key, &left_key, moved_from.as_ref(), index == length - 1)?;
    }
    Ok(())
}

/// One line of a text diff as formatters show it: where it applies, and its pieces.
pub struct TextDiffLine {
    pub line: JsString,
    pub chr: JsString,
    pub pieces: Vec<(PieceType, JsString)>,
}

/// What a piece of a text diff line does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceType {
    Context,
    Added,
    Deleted,
}

impl PieceType {
    pub fn name(self) -> &'static str {
        match self {
            PieceType::Context => "context",
            PieceType::Added => "added",
            PieceType::Deleted => "deleted",
        }
    }
}

/// `/^(?:@@ )?[-+]?(\d+),(\d+)/` on one hunk.
fn hunk_location(hunk: &[u16]) -> Option<(JsString, JsString)> {
    fn at(hunk: &[u16], mut i: usize) -> Option<(JsString, JsString)> {
        if matches!(hunk.get(i), Some(0x2B | 0x2D)) {
            i += 1;
        }
        let digits = |i: &mut usize| {
            let start = *i;
            while hunk.get(*i).is_some_and(|u| (0x30..=0x39).contains(u)) {
                *i += 1;
            }
            (*i > start).then(|| JsString::from_units(hunk[start..*i].to_vec()))
        };
        let line = digits(&mut i)?;
        if hunk.get(i) != Some(&0x2C) {
            return None;
        }
        i += 1;
        let chr = digits(&mut i)?;
        Some((line, chr))
    }
    let prefixed: Vec<u16> = "@@ ".encode_utf16().collect();
    if hunk.starts_with(&prefixed) {
        if let Some(found) = at(hunk, 3) {
            return Some(found);
        }
    }
    at(hunk, 0)
}

/// `parseTextDiff`: the hunks of a text diff string.
pub fn parse_text_diff(value: &Value) -> Result<Vec<TextDiffLine>, JsError> {
    let Value::String(text) = value else {
        return Err(not_a_function(value, "value", "split"));
    };
    let mut output = Vec::new();
    for hunk in text.split_str("\n@@ ") {
        let Some((line, chr)) = hunk_location(hunk.units()) else {
            return Err(JsError::type_error("Cannot read properties of null (reading 'slice')"));
        };
        let mut pieces = Vec::new();
        for piece in hunk.split_unit(0x0A).into_iter().skip(1) {
            let piece_type = match piece.unit_at(0) {
                None => continue,
                Some(0x2B) => PieceType::Added,
                Some(0x2D) => PieceType::Deleted,
                Some(_) => PieceType::Context,
            };
            pieces.push((piece_type, piece.slice_from(1)));
        }
        output.push(TextDiffLine { line, chr, pieces });
    }
    Ok(output)
}

/// `JSON.stringify(value, null, 2)`, which formatters pass on as a string.
pub fn pretty_json(value: &Value) -> Option<JsString> {
    crate::js::json::stringify_pretty(value)
}

/// `delta[n]` of an array delta.
pub fn element(delta: &Value, n: usize) -> Value {
    delta
        .as_array()
        .and_then(|items| items.get(n))
        .cloned()
        .unwrap_or_default()
}
