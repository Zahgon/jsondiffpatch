// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! The JSON Patch (RFC 6902) formatter: `add`, `remove`, `replace` and `move` operations, ordered so that they
//! apply one after another (removals deepest-last and highest index first, then moves, then the rest).

use super::{element, DeltaType, Formatter, NodeType, Place};
use crate::js::number::parse_int;
use crate::js::sort::sort_by;
use crate::js::{join_element, JsError, JsString, Object, Value};

/// One JSON Patch operation.
#[derive(Clone, Debug)]
pub enum Operation {
    Add { path: JsString, value: Value },
    Remove { path: JsString },
    Replace { path: JsString, value: Value },
    Move { from: JsString, path: JsString },
}

impl Operation {
    fn path(&self) -> &JsString {
        match self {
            Operation::Add { path, .. }
            | Operation::Remove { path }
            | Operation::Replace { path, .. }
            | Operation::Move { path, .. } => path,
        }
    }

    /// The operation as a JSON object.
    pub fn to_value(&self) -> Value {
        let mut object = Object::new();
        let mut set = |key: &str, value: Value| object.set(JsString::from(key), value);
        match self {
            Operation::Add { path, value } => {
                set("op", Value::str("add"));
                set("path", Value::String(path.clone()));
                set("value", value.clone());
            }
            Operation::Remove { path } => {
                set("op", Value::str("remove"));
                set("path", Value::String(path.clone()));
            }
            Operation::Replace { path, value } => {
                set("op", Value::str("replace"));
                set("path", Value::String(path.clone()));
                set("value", value.clone());
            }
            Operation::Move { from, path } => {
                set("op", Value::str("move"));
                set("from", Value::String(from.clone()));
                set("path", Value::String(path.clone()));
            }
        }
        Value::Object(object)
    }
}

/// Formats `delta` (applied to `left`, when known) as JSON Patch operations.
pub fn format(delta: &Value, left: &Value) -> Result<Vec<Operation>, JsError> {
    let mut formatter = JsonPatchFormatter::default();
    super::format(&mut formatter, delta, left)?;
    Ok(reorder(formatter.result))
}

#[derive(Default)]
struct JsonPatchFormatter {
    result: Vec<Operation>,
    path: Vec<Value>,
}

fn join_path(path: &[Value]) -> JsString {
    let parts: Vec<JsString> = path.iter().map(join_element).collect();
    let mut text = JsString::from("/");
    text.push_js(&JsString::join(&parts, "/"));
    text
}

impl JsonPatchFormatter {
    fn current_path(&self) -> JsString {
        join_path(&self.path)
    }

    fn to_path(&self, to: &Value) -> JsString {
        let mut path = self.path.clone();
        match path.last_mut() {
            Some(last) => *last = to.clone(),
            None => return JsString::from("/"),
        }
        join_path(&path)
    }
}

impl Formatter for JsonPatchFormatter {
    fn include_move_destinations(&self) -> bool {
        true
    }

    fn root_begin(&mut self, _delta_type: DeltaType, _node_type: NodeType) -> Result<(), JsError> {
        Ok(())
    }

    fn root_end(&mut self, _delta_type: DeltaType, _node_type: NodeType) -> Result<(), JsError> {
        Ok(())
    }

    fn node_begin(&mut self, place: &Place, _delta_type: DeltaType, _node_type: NodeType) -> Result<(), JsError> {
        self.path.push(place.left_key.clone());
        Ok(())
    }

    fn node_end(&mut self, _place: &Place, _delta_type: DeltaType, _node_type: NodeType) -> Result<(), JsError> {
        self.path.pop();
        Ok(())
    }

    fn format_leaf(
        &mut self,
        delta_type: DeltaType,
        delta: &Value,
        _left: &Value,
        _place: Option<&Place>,
    ) -> Result<(), JsError> {
        let path = self.current_path();
        match delta_type {
            DeltaType::Added => self.result.push(Operation::Add {
                path,
                value: element(delta, 0),
            }),
            DeltaType::Modified => self.result.push(Operation::Replace {
                path,
                value: element(delta, 1),
            }),
            DeltaType::Deleted => self.result.push(Operation::Remove { path }),
            DeltaType::Moved => {
                let to = self.to_path(&element(delta, 1));
                self.result.push(Operation::Move { from: path, path: to });
            }
            DeltaType::TextDiff => return Err(JsError::error("Not implemented")),
            _ => {}
        }
        Ok(())
    }

    /// JSON Patch output is the list of operations, which has no way to carry an error. The original formatter writes
    /// `[ERROR] <error>` to its text buffer here, but its `format` returns only the operations, so the error never
    /// reaches the output; this port leaves the operations untouched in the same way.
    fn format_error(&mut self, _error: &JsError) {}
}

fn compare_by_index_desc(a: &JsString, b: &JsString) -> f64 {
    let (last_a, last_b) = (parse_int(a), parse_int(b));
    if last_a.is_nan() || last_b.is_nan() {
        0.0
    } else {
        last_b - last_a
    }
}

/// Removals first (shallowest first, then highest index first), then moves, then everything else.
fn reorder(operations: Vec<Operation>) -> Vec<Operation> {
    let mut moves = Vec::new();
    let mut removes = Vec::new();
    let mut rest = Vec::new();
    for operation in operations {
        match operation {
            Operation::Move { .. } => moves.push(operation),
            Operation::Remove { .. } => removes.push(operation),
            _ => rest.push(operation),
        }
    }
    sort_by(&mut removes, |a, b| {
        let split_a = a.path().split_unit(0x2F);
        let split_b = b.path().split_unit(0x2F);
        if split_a.len() != split_b.len() {
            split_a.len() as f64 - split_b.len() as f64
        } else {
            compare_by_index_desc(
                split_a.last().expect("split is never empty"),
                split_b.last().expect("split is never empty"),
            )
        }
    });
    removes.into_iter().chain(moves).chain(rest).collect()
}
