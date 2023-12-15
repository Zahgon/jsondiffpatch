// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! The `jsondiffpatch` command: JSON-lines requests on standard input, one JSON response line per request on
//! standard output.

use crate::formatters::{annotated, console, html, jsonpatch};
use crate::js::json;
use crate::js::{JsError, JsString, Object, Value};
use crate::options::Options;
use crate::{revive_dates, DiffPatcher};

/// The usage text, written to standard error when the command is given arguments.
pub const USAGE: &str = "usage: jsondiffpatch < requests.jsonl\n\
\n\
Reads one JSON request per line on standard input and writes one JSON response per line.\n\
Requests: {\"op\":\"diff\",\"left\":L,\"right\":R}, {\"op\":\"patch\",\"left\":L,\"delta\":D},\n          \
{\"op\":\"unpatch\",\"right\":R,\"delta\":D}, {\"op\":\"reverse\",\"delta\":D},\n          \
{\"op\":\"format\",\"delta\":D,\"left\":L}\n\
with optional \"options\", \"format\" (delta, jsonpatch, annotated, console, html) and \"reviveDates\".\n";

fn request_error(message: impl Into<JsString>) -> JsError {
    JsError::new("RequestError", message)
}

/// `JSON.stringify(value)` as a template literal shows it (`undefined` for values JSON cannot represent).
fn json_text(value: &Value) -> JsString {
    json::stringify(value).unwrap_or_else(|| JsString::from("undefined"))
}

fn is_plain_object(value: &Value) -> bool {
    matches!(value, Value::Object(_))
}

/// DiffPatcher options from a request's `options` member.
fn build_options(spec: &Value) -> Result<Options, JsError> {
    let mut options = Options::default();
    let mut text_diff = Value::Undefined;
    if !spec.is_undefined() {
        let Value::Object(spec) = spec else {
            return Err(request_error("options must be an object"));
        };
        let member = |name: &str| spec.get(&JsString::from(name)).cloned();
        if let Some(value) = member("matchByPosition") {
            options.match_by_position = value;
        }
        if let Some(value) = member("arrays") {
            options.arrays = value;
        }
        if let Some(value) = member("textDiff") {
            text_diff = value;
        }
        if let Some(value) = member("cloneDiffValues") {
            options.clone_diff_values = value;
        }
        if let Some(name) = member("objectHash") {
            let Value::String(name) = name else {
                return Err(request_error("objectHash must be a property name"));
            };
            options.object_hash = Some(name);
        }
        if let Some(names) = member("propertyFilter") {
            let Value::Array(names) = names else {
                return Err(request_error("propertyFilter must be an array of property names"));
            };
            options.property_filter = Some(names);
        }
    }
    match text_diff {
        Value::Undefined => {}
        Value::Object(object) => options.text_diff = object,
        _ => return Err(request_error("textDiff must be an object")),
    }
    Ok(options)
}

fn single(name: &str, value: Value) -> Value {
    let mut object = Object::new();
    object.set(JsString::from(name), value);
    Value::Object(object)
}

/// The response to a delta: the delta itself, or one of the formatters' renderings of it.
fn respond(delta: &Value, left: &Value, format: &Value) -> Result<Value, JsError> {
    let format_name = match format {
        Value::Undefined => Some("delta"),
        Value::String(name) => ["delta", "jsonpatch", "annotated", "console", "html"]
            .into_iter()
            .find(|n| name.eq_str(n)),
        _ => None,
    };
    let text = |text: JsString| single("text", Value::String(text));
    match format_name {
        Some("delta") => Ok(single("delta", delta.clone())),
        Some("jsonpatch") => {
            let operations = jsonpatch::format(delta, left)?;
            Ok(single(
                "jsonpatch",
                Value::Array(operations.iter().map(jsonpatch::Operation::to_value).collect()),
            ))
        }
        Some("annotated") => Ok(text(annotated::format(delta, left)?)),
        Some("console") => Ok(text(console::format(delta, left)?)),
        Some("html") => Ok(text(html::format(delta, left)?)),
        _ => {
            let mut message = JsString::from("unknown format: ");
            message.push_js(&json_text(format));
            Err(request_error(message))
        }
    }
}

/// Handles one request value.
pub fn handle(request: &Value) -> Result<Value, JsError> {
    if !is_plain_object(request) {
        return Err(request_error("a request must be a JSON object"));
    }
    let member = |name: &str| request.get(&JsString::from(name));
    let options = build_options(&member("options"))?;
    let (mut left, mut right, mut delta) = (member("left"), member("right"), member("delta"));
    if member("reviveDates").truthy() {
        left = revive_dates(&left);
        right = revive_dates(&right);
        delta = revive_dates(&delta);
    }
    let diffpatcher = DiffPatcher::new(options);
    let op = member("op");
    let format = member("format");
    match op.as_string().map(JsString::to_string_lossy).as_deref() {
        Some("diff") => {
            let delta = diffpatcher.diff(&left, &right)?;
            respond(&delta, &left, &format)
        }
        Some("patch") => Ok(single("result", diffpatcher.patch(left, &delta)?)),
        Some("unpatch") => Ok(single("result", diffpatcher.unpatch(right, &delta)?)),
        Some("reverse") => {
            let reversed = diffpatcher.reverse(&delta)?;
            respond(&reversed, &Value::Undefined, &format)
        }
        Some("format") => respond(&delta, &left, &format),
        _ => {
            let mut message = JsString::from("unknown op: ");
            message.push_js(&json_text(&op));
            Err(request_error(message))
        }
    }
}

/// The response line for one request line (without the line break), and whether it is an error.
pub fn respond_to_line(line: &[u16]) -> (JsString, bool) {
    let response = json::parse(line).and_then(|request| handle(&request));
    match response {
        Ok(value) => (json_text(&value), false),
        Err(error) => {
            let mut body = Object::new();
            body.set(
                JsString::from("name"),
                Value::String(JsString::from(error.name.as_str())),
            );
            body.set(JsString::from("message"), Value::String(error.message.clone()));
            (json_text(&single("error", Value::Object(body))), true)
        }
    }
}

/// Runs the command over `input`; the standard output it produces and the exit status.
pub fn run(input: &[u8]) -> (Vec<u8>, i32) {
    let text: Vec<u16> = String::from_utf8_lossy(input).encode_utf16().collect();
    let mut output = Vec::new();
    let mut failed = false;
    for line in text.split(|&unit| unit == 0x0A) {
        if line.is_empty() {
            continue;
        }
        let (response, is_error) = respond_to_line(line);
        failed |= is_error;
        output.extend_from_slice(response.to_utf8().as_bytes());
        output.push(b'\n');
    }
    (output, i32::from(failed))
}
