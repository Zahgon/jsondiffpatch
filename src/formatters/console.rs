// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! The console formatter, without colours: an indented, JSON-like listing of the changes.

use super::{element, parse_text_diff, pretty_json, DeltaType, Formatter, NodeType, Place};
use crate::js::{JsError, JsString, Value};

/// Formats `delta` for a terminal.
pub fn format(delta: &Value, left: &Value) -> Result<JsString, JsError> {
    let mut formatter = ConsoleFormatter::default();
    super::format(&mut formatter, delta, left)?;
    Ok(formatter.buffer)
}

#[derive(Default)]
struct ConsoleFormatter {
    buffer: JsString,
    indent_level: i64,
    indent_pad: JsString,
}

impl ConsoleFormatter {
    fn out(&mut self, text: &JsString) {
        let mut pad = JsString::from("\n");
        pad.push_js(&self.indent_pad);
        let lines = text.split_unit(0x0A);
        let mut joined = JsString::new();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                joined.push_js(&pad);
            }
            joined.push_js(line);
        }
        self.buffer.push_js(&joined);
    }

    fn out_str(&mut self, text: &str) {
        self.out(&JsString::from(text));
    }

    fn out_line(&mut self) {
        self.buffer.push_str("\n");
        let pad = self.indent_pad.clone();
        self.buffer.push_js(&pad);
    }

    fn indent(&mut self, levels: i64) -> Result<(), JsError> {
        self.indent_level += levels;
        if self.indent_level + 1 < 0 {
            return Err(JsError::new("RangeError", "Invalid array length"));
        }
        let mut pad = JsString::new();
        for _ in 0..self.indent_level.max(0) {
            pad.push_str("  ");
        }
        self.indent_pad = pad;
        self.out_line();
        Ok(())
    }

    fn format_value(&mut self, value: &Value) -> Result<(), JsError> {
        match pretty_json(value) {
            Some(json) => {
                self.out(&json);
                Ok(())
            }
            None => Err(JsError::type_error(
                "Cannot read properties of undefined (reading 'split')",
            )),
        }
    }

    fn format_text_diff_string(&mut self, value: &Value) -> Result<(), JsError> {
        let lines = parse_text_diff(value)?;
        self.indent(1)?;
        let count = lines.len();
        for (i, line) in lines.into_iter().enumerate() {
            let mut location = line.line.clone();
            location.push_str(",");
            location.push_js(&line.chr);
            location.push_str(" ");
            self.out(&location);
            for (_, text) in &line.pieces {
                self.out(text);
            }
            if i + 1 < count {
                self.out_line();
            }
        }
        self.indent(-1)
    }
}

impl Formatter for ConsoleFormatter {
    fn include_move_destinations(&self) -> bool {
        false
    }

    fn root_begin(&mut self, delta_type: DeltaType, node_type: NodeType) -> Result<(), JsError> {
        if delta_type == DeltaType::Node {
            self.out_str(if node_type == NodeType::Array { "[" } else { "{" });
            self.indent(1)?;
        }
        Ok(())
    }

    fn root_end(&mut self, delta_type: DeltaType, node_type: NodeType) -> Result<(), JsError> {
        if delta_type == DeltaType::Node {
            self.indent(-1)?;
            self.out_str(if node_type == NodeType::Array { "]" } else { "}" });
        }
        Ok(())
    }

    fn node_begin(&mut self, place: &Place, delta_type: DeltaType, node_type: NodeType) -> Result<(), JsError> {
        let mut label = place.left_key.to_js_string();
        label.push_str(": ");
        self.out(&label);
        if delta_type == DeltaType::Node {
            self.out_str(if node_type == NodeType::Array { "[" } else { "{" });
            self.indent(1)?;
        }
        Ok(())
    }

    fn node_end(&mut self, place: &Place, delta_type: DeltaType, node_type: NodeType) -> Result<(), JsError> {
        if delta_type == DeltaType::Node {
            self.indent(-1)?;
            if node_type == NodeType::Array {
                self.out_str("]");
            } else {
                self.out_str(if place.is_last { "}" } else { "}," });
            }
        }
        if !place.is_last {
            self.out_line();
        }
        Ok(())
    }

    fn format_leaf(
        &mut self,
        delta_type: DeltaType,
        delta: &Value,
        left: &Value,
        _place: Option<&Place>,
    ) -> Result<(), JsError> {
        match delta_type {
            DeltaType::Unchanged | DeltaType::MoveDestination => {
                if left.is_undefined() {
                    return Ok(());
                }
                self.format_value(left)
            }
            DeltaType::Added | DeltaType::Deleted => self.format_value(&element(delta, 0)),
            DeltaType::Modified => {
                self.format_value(&element(delta, 0))?;
                self.out_str(" => ");
                self.format_value(&element(delta, 1))
            }
            DeltaType::Moved => {
                let mut text = JsString::from("==> ");
                text.push_js(&element(delta, 1).to_js_string());
                self.out(&text);
                Ok(())
            }
            DeltaType::TextDiff => self.format_text_diff_string(&element(delta, 0)),
            DeltaType::Node | DeltaType::Unknown => Ok(()),
        }
    }

    fn format_error(&mut self, error: &JsError) {
        let mut text = JsString::from("[ERROR]");
        text.push_js(&error.to_js_string());
        self.out(&text);
    }
}
