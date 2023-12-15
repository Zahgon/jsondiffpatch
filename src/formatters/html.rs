// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! The HTML formatter: nested lists with a class per kind of change, and arrows for moved array items (drawn by
//! a script appended to the output).

use super::{element, parse_text_diff, pretty_json, DeltaType, Formatter, NodeType, Place};
use crate::js::uri::decode_uri;
use crate::js::{JsError, JsString, Value};

/// The arrow drawn from a moved item to its destination.
const ARROW: &str = r#"<div class="jsondiffpatch-arrow" style="position: relative; left: -34px;">
          <svg width="30" height="60" style="position: absolute; display: none;">
          <defs>
              <marker id="markerArrow" markerWidth="8" markerHeight="8"
                 refx="2" refy="4"
                     orient="auto" markerUnits="userSpaceOnUse">
                  <path d="M1,1 L1,7 L7,4 L1,1" style="fill: #339;" />
              </marker>
          </defs>
          <path d="M30,0 Q-10,25 26,50"
            style="stroke: #88f; stroke-width: 2px; fill: none; stroke-opacity: 0.5; marker-end: url(#markerArrow);"
          ></path>
          </svg>
      </div>"#;
/// The script that positions the arrows once the HTML is in a page.
const ADJUST_ARROWS: &str = r#"function jsondiffpatchHtmlFormatterAdjustArrows(nodeArg) {
    const node = nodeArg || document;
    const getElementText = ({ textContent, innerText }) => textContent || innerText;
    const eachByQuery = (el, query, fn) => {
        const elems = el.querySelectorAll(query);
        for (let i = 0, l = elems.length; i < l; i++) {
            fn(elems[i]);
        }
    };
    const eachChildren = ({ children }, fn) => {
        for (let i = 0, l = children.length; i < l; i++) {
            fn(children[i], i);
        }
    };
    eachByQuery(node, '.jsondiffpatch-arrow', ({ parentNode, children, style }) => {
        const arrowParent = parentNode;
        const svg = children[0];
        const path = svg.children[1];
        svg.style.display = 'none';
        const destination = getElementText(arrowParent.querySelector('.jsondiffpatch-moved-destination'));
        const container = arrowParent.parentNode;
        let destinationElem;
        eachChildren(container, (child) => {
            if (child.getAttribute('data-key') === destination) {
                destinationElem = child;
            }
        });
        if (!destinationElem) {
            return;
        }
        try {
            const distance = destinationElem.offsetTop - arrowParent.offsetTop;
            svg.setAttribute('height', `${Math.abs(distance) + 6}`);
            style.top = `${-8 + (distance > 0 ? 0 : distance)}px`;
            const curve = distance > 0
                ? `M30,0 Q-10,${Math.round(distance / 2)} 26,${distance - 4}`
                : `M30,${-distance} Q-10,${Math.round(-distance / 2)} 26,4`;
            path.setAttribute('d', curve);
            svg.style.display = '';
        }
        catch (err) {
            // continue regardless of error
        }
    });
}"#;

/// Formats `delta` as HTML.
pub fn format(delta: &Value, left: &Value) -> Result<JsString, JsError> {
    let mut formatter = HtmlFormatter::default();
    super::format(&mut formatter, delta, left)?;
    Ok(formatter.buffer)
}

#[derive(Default)]
struct HtmlFormatter {
    buffer: JsString,
    has_arrows: bool,
}

/// Escapes `&`, `<`, `>`, `'` and `"`.
pub fn html_escape(text: &JsString) -> JsString {
    let mut out = JsString::new();
    for &unit in text.units() {
        match unit {
            0x26 => out.push_str("&amp;"),
            0x3C => out.push_str("&lt;"),
            0x3E => out.push_str("&gt;"),
            0x27 => out.push_str("&apos;"),
            0x22 => out.push_str("&quot;"),
            other => out.push_units(&[other]),
        }
    }
    out
}

fn node_class(delta_type: DeltaType, node_type: NodeType) -> JsString {
    let mut class = JsString::from("jsondiffpatch-");
    class.push_str(delta_type.name());
    if node_type != NodeType::None {
        class.push_str(" jsondiffpatch-child-node-type-");
        class.push_str(node_type.name());
    }
    class
}

impl HtmlFormatter {
    fn out(&mut self, text: &JsString) {
        self.buffer.push_js(text);
    }

    fn out_str(&mut self, text: &str) {
        self.buffer.push_str(text);
    }

    fn format_value(&mut self, value: &Value) -> Result<(), JsError> {
        let Some(json) = pretty_json(value) else {
            return Err(JsError::type_error(
                "Cannot read properties of undefined (reading 'replace')",
            ));
        };
        let mut text = JsString::from("<pre>");
        text.push_js(&html_escape(&json));
        text.push_str("</pre>");
        self.out(&text);
        Ok(())
    }

    fn format_wrapped_value(&mut self, value: &Value) -> Result<(), JsError> {
        self.out_str("<div class=\"jsondiffpatch-value\">");
        self.format_value(value)?;
        self.out_str("</div>");
        Ok(())
    }

    fn format_text_diff_string(&mut self, value: &Value) -> Result<(), JsError> {
        let lines = parse_text_diff(value)?;
        self.out_str("<ul class=\"jsondiffpatch-textdiff\">");
        for line in lines {
            let mut text = JsString::from(
                "<li><div class=\"jsondiffpatch-textdiff-location\"><span class=\"jsondiffpatch-textdiff-line-number\">",
            );
            text.push_js(&line.line);
            text.push_str("</span><span class=\"jsondiffpatch-textdiff-char\">");
            text.push_js(&line.chr);
            text.push_str("</span></div><div class=\"jsondiffpatch-textdiff-line\">");
            self.out(&text);
            for (piece_type, piece) in &line.pieces {
                let decoded = decode_uri(piece.units())?;
                let mut span = JsString::from("<span class=\"jsondiffpatch-textdiff-");
                span.push_str(piece_type.name());
                span.push_str("\">");
                span.push_js(&html_escape(&decoded));
                span.push_str("</span>");
                self.out(&span);
            }
            self.out_str("</div></li>");
        }
        self.out_str("</ul>");
        Ok(())
    }
}

impl Formatter for HtmlFormatter {
    fn include_move_destinations(&self) -> bool {
        true
    }

    fn root_begin(&mut self, delta_type: DeltaType, node_type: NodeType) -> Result<(), JsError> {
        let mut text = JsString::from("<div class=\"jsondiffpatch-delta ");
        text.push_js(&node_class(delta_type, node_type));
        text.push_str("\">");
        self.out(&text);
        Ok(())
    }

    fn root_end(&mut self, _delta_type: DeltaType, _node_type: NodeType) -> Result<(), JsError> {
        self.out_str("</div>");
        if self.has_arrows {
            self.out_str("<script type=\"text/javascript\">setTimeout(");
            self.out_str(ADJUST_ARROWS);
            self.out_str(",10);</script>");
        }
        Ok(())
    }

    fn node_begin(&mut self, place: &Place, delta_type: DeltaType, node_type: NodeType) -> Result<(), JsError> {
        let left_key = place.left_key.to_js_string();
        let mut text = JsString::from("<li class=\"");
        text.push_js(&node_class(delta_type, node_type));
        text.push_str("\" data-key=\"");
        text.push_js(&left_key);
        text.push_str("\"><div class=\"jsondiffpatch-property-name\">");
        text.push_js(&left_key);
        text.push_str("</div>");
        self.out(&text);
        Ok(())
    }

    fn node_end(&mut self, _place: &Place, _delta_type: DeltaType, _node_type: NodeType) -> Result<(), JsError> {
        self.out_str("</li>");
        Ok(())
    }

    fn node_open(&mut self, node_type: NodeType) -> Result<(), JsError> {
        self.out_str("<ul class=\"jsondiffpatch-node jsondiffpatch-node-type-");
        self.out_str(node_type.name());
        self.out_str("\">");
        Ok(())
    }

    fn node_close(&mut self, _node_type: NodeType) -> Result<(), JsError> {
        self.out_str("</ul>");
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
                self.format_wrapped_value(left)
            }
            DeltaType::Added | DeltaType::Deleted => self.format_wrapped_value(&element(delta, 0)),
            DeltaType::Modified => {
                self.out_str("<div class=\"jsondiffpatch-value jsondiffpatch-left-value\">");
                self.format_value(&element(delta, 0))?;
                self.out_str("</div><div class=\"jsondiffpatch-value jsondiffpatch-right-value\">");
                self.format_value(&element(delta, 1))?;
                self.out_str("</div>");
                Ok(())
            }
            DeltaType::Moved => {
                self.out_str("<div class=\"jsondiffpatch-value\">");
                self.format_value(&element(delta, 0))?;
                let mut text = JsString::from("</div><div class=\"jsondiffpatch-moved-destination\">");
                text.push_js(&element(delta, 1).to_js_string());
                text.push_str("</div>");
                self.out(&text);
                self.out_str(ARROW);
                self.has_arrows = true;
                Ok(())
            }
            DeltaType::TextDiff => {
                self.out_str("<div class=\"jsondiffpatch-value\">");
                self.format_text_diff_string(&element(delta, 0))?;
                self.out_str("</div>");
                Ok(())
            }
            DeltaType::Node | DeltaType::Unknown => Ok(()),
        }
    }

    fn format_error(&mut self, error: &JsError) {
        let mut text = JsString::from("<pre class=\"jsondiffpatch-error\">");
        text.push_js(&error.to_js_string());
        text.push_str("</pre>");
        self.out(&text);
    }
}
