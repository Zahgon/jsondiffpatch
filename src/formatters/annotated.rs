// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! The annotated formatter: an HTML table of the delta's JSON with a note explaining every change.

use super::{pretty_json, DeltaType, Formatter, NodeType, Place};
use crate::js::{JsError, JsString, Value};

/// Formats `delta` as an annotated HTML table.
pub fn format(delta: &Value, left: &Value) -> Result<JsString, JsError> {
    let mut formatter = AnnotatedFormatter::default();
    super::format(&mut formatter, delta, left)?;
    Ok(formatter.buffer)
}

const ARRAY_ROW_JSON: &str = "\"_t\": \"a\",";
const ARRAY_ROW_NOTE: &str = "Array delta (member names indicate array indices)";

#[derive(Default)]
struct AnnotatedFormatter {
    buffer: JsString,
    indent_level: i64,
    indent_pad: Option<JsString>,
}

fn wrap_property_name(name: &JsString) -> JsString {
    let mut text = JsString::from("<pre style=\"display:inline-block\">&quot;");
    text.push_js(name);
    text.push_str("&quot;</pre>");
    text
}

/// The note on a change: what it does, and where.
fn annotation(delta_type: DeltaType, delta: &Value, left_key: Option<&Value>) -> Option<JsString> {
    let describe = |root: &str, index: &str, property: &str, legend: &str| {
        let mut text = JsString::new();
        match left_key {
            None => text.push_str(root),
            Some(Value::Number(_)) => {
                text.push_str(index);
                text.push_js(&left_key.map(Value::to_js_string).unwrap_or_default());
            }
            Some(key) => {
                text.push_str(property);
                text.push_js(&wrap_property_name(&key.to_js_string()));
            }
        }
        text.push_str(legend);
        text
    };
    Some(match delta_type {
        DeltaType::Added => describe(
            "new value",
            "insert at index ",
            "add property ",
            " <pre>([newValue])</pre>",
        ),
        DeltaType::Modified => describe(
            "modify value",
            "modify at index ",
            "modify property ",
            " <pre>([previousValue, newValue])</pre>",
        ),
        DeltaType::Deleted => describe(
            "delete value",
            "remove index ",
            "delete property ",
            " <pre>([previousValue, 0, 0])</pre>",
        ),
        DeltaType::Moved => {
            let mut text = JsString::from("move from <span title=\"(position to remove at original state)\">index ");
            text.push_js(
                &left_key
                    .map(Value::to_js_string)
                    .unwrap_or_else(|| JsString::from("undefined")),
            );
            text.push_str("</span> to <span title=\"(position to insert at final state)\">index ");
            text.push_js(&super::element(delta, 1).to_js_string());
            text.push_str("</span>");
            text
        }
        DeltaType::TextDiff => {
            let mut text = JsString::from("text diff");
            match left_key {
                None => {}
                Some(Value::Number(_)) => {
                    text.push_str(" at index ");
                    text.push_js(&left_key.map(Value::to_js_string).unwrap_or_default());
                }
                Some(key) => {
                    text.push_str(" at property ");
                    text.push_js(&wrap_property_name(&key.to_js_string()));
                }
            }
            text.push_str(
                ", format is <a href=\"https://code.google.com/p/google-diff-match-patch/wiki/Unidiff\">a variation \
                 of Unidiff</a>",
            );
            text
        }
        _ => return None,
    })
}

impl AnnotatedFormatter {
    fn out(&mut self, text: &JsString) {
        self.buffer.push_js(text);
    }

    fn out_str(&mut self, text: &str) {
        self.buffer.push_str(text);
    }

    fn indent(&mut self, levels: i64) -> Result<(), JsError> {
        self.indent_level += levels;
        if self.indent_level + 1 < 0 {
            return Err(JsError::new("RangeError", "Invalid array length"));
        }
        let mut pad = JsString::new();
        for _ in 0..self.indent_level.max(0) {
            pad.push_str("&nbsp;&nbsp;");
        }
        self.indent_pad = Some(pad);
        Ok(())
    }

    fn row(&mut self, json: &JsString, note: Option<&JsString>) {
        self.out_str(
            "<tr><td style=\"white-space: nowrap;\"><pre class=\"jsondiffpatch-annotated-indent\" style=\"display: \
             inline-block\">",
        );
        if let Some(pad) = self.indent_pad.clone() {
            self.out(&pad);
        }
        self.out_str("</pre><pre style=\"display: inline-block\">");
        self.out(json);
        self.out_str("</pre></td><td class=\"jsondiffpatch-delta-note\"><div>");
        if let Some(note) = note {
            self.out(note);
        }
        self.out_str("</div></td></tr>");
    }

    fn row_str(&mut self, json: &str, note: Option<&str>) {
        let note = note.map(JsString::from);
        self.row(&JsString::from(json), note.as_ref());
    }

    fn format_any_change(
        &mut self,
        delta_type: DeltaType,
        delta: &Value,
        place: Option<&Place>,
    ) -> Result<(), JsError> {
        let note = annotation(delta_type, delta, place.map(|p| p.left_key));
        let mut json = pretty_json(delta).unwrap_or_else(|| JsString::from("undefined"));
        if delta_type == DeltaType::TextDiff {
            let pieces = json.split_str("\\n");
            json = JsString::join(&pieces, "\\n\"+\n   \"");
        }
        self.indent(1)?;
        self.row(&json, note.as_ref());
        self.indent(-1)
    }
}

impl Formatter for AnnotatedFormatter {
    fn include_move_destinations(&self) -> bool {
        false
    }

    fn root_begin(&mut self, delta_type: DeltaType, node_type: NodeType) -> Result<(), JsError> {
        self.out_str("<table class=\"jsondiffpatch-annotated-delta\">");
        if delta_type == DeltaType::Node {
            self.row_str("{", None);
            self.indent(1)?;
        }
        if node_type == NodeType::Array {
            self.row_str(ARRAY_ROW_JSON, Some(ARRAY_ROW_NOTE));
        }
        Ok(())
    }

    fn root_end(&mut self, delta_type: DeltaType, _node_type: NodeType) -> Result<(), JsError> {
        if delta_type == DeltaType::Node {
            self.indent(-1)?;
            self.row_str("}", None);
        }
        self.out_str("</table>");
        Ok(())
    }

    fn node_begin(&mut self, place: &Place, delta_type: DeltaType, node_type: NodeType) -> Result<(), JsError> {
        let mut json = JsString::from("&quot;");
        json.push_js(place.key);
        json.push_str("&quot;: {");
        self.row(&json, None);
        if delta_type == DeltaType::Node {
            self.indent(1)?;
        }
        if node_type == NodeType::Array {
            self.row_str(ARRAY_ROW_JSON, Some(ARRAY_ROW_NOTE));
        }
        Ok(())
    }

    fn node_end(&mut self, place: &Place, delta_type: DeltaType, _node_type: NodeType) -> Result<(), JsError> {
        if delta_type == DeltaType::Node {
            self.indent(-1)?;
        }
        self.row_str(if place.is_last { "}" } else { "}," }, None);
        Ok(())
    }

    fn format_leaf(
        &mut self,
        delta_type: DeltaType,
        delta: &Value,
        _left: &Value,
        place: Option<&Place>,
    ) -> Result<(), JsError> {
        match delta_type {
            DeltaType::Unchanged | DeltaType::MoveDestination | DeltaType::Node | DeltaType::Unknown => Ok(()),
            other => self.format_any_change(other, delta, place),
        }
    }

    fn format_error(&mut self, error: &JsError) {
        let mut note = JsString::from("<pre class=\"jsondiffpatch-error\">");
        note.push_js(&error.to_js_string());
        note.push_str("</pre>");
        self.row(&JsString::new(), Some(&note));
    }
}
