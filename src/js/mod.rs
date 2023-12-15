// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! The slice of JavaScript semantics jsondiffpatch's behaviour rests on: UTF-16 strings, number formatting and
//! parsing, JSON, dates, URI escapes, V8's sort, and property access with the errors strict-mode code raises.

pub mod date;
pub mod error;
pub mod json;
pub mod number;
pub mod sort;
pub mod string;
pub mod uri;
pub mod value;

pub use error::JsError;
pub use string::JsString;
pub use value::{Object, Value};

use number::{number_to_string, to_integer_or_infinity};

/// The property key a number denotes (`ToPropertyKey`).
pub fn number_key(value: f64) -> JsString {
    JsString::from(number_to_string(value))
}

/// A message made of literal text and JavaScript strings (which may hold lone surrogates).
macro_rules! message {
    ($($part:expr),* $(,)?) => {{
        let mut out = JsString::new();
        $( Piece::push_to(&$part, &mut out); )*
        out
    }};
}

trait Piece {
    fn push_to(&self, out: &mut JsString);
}

impl Piece for str {
    fn push_to(&self, out: &mut JsString) {
        out.push_str(self);
    }
}

impl Piece for JsString {
    fn push_to(&self, out: &mut JsString) {
        out.push_js(self);
    }
}

impl<T: Piece + ?Sized> Piece for &T {
    fn push_to(&self, out: &mut JsString) {
        (**self).push_to(out);
    }
}

/// `target[key]`: a `TypeError` when the target is `undefined` or `null`.
pub fn get_prop(target: &Value, key: &JsString) -> Result<Value, JsError> {
    match target {
        Value::Undefined | Value::Null => Err(JsError::type_error(message!(
            "Cannot read properties of ",
            primitive_name(target),
            " (reading '",
            key,
            "')",
        ))),
        _ => Ok(target.get(key)),
    }
}

/// `target[key]` with a static key.
pub fn get_named(target: &Value, key: &str) -> Result<Value, JsError> {
    get_prop(target, &JsString::from(key))
}

fn primitive_name(value: &Value) -> &'static str {
    match value {
        Value::Undefined => "undefined",
        _ => "null",
    }
}

/// The error of calling `receiver.method(...)` when `receiver` has no such method: `TypeError: name.method is
/// not a function`, or the property read error for `undefined` and `null`.
pub fn not_a_function(receiver: &Value, name: &str, method: &str) -> JsError {
    match receiver {
        Value::Undefined | Value::Null => match get_named(receiver, method) {
            Err(error) => error,
            Ok(_) => JsError::type_error(format!("{name}.{method} is not a function")),
        },
        _ => JsError::type_error(format!("{name}.{method} is not a function")),
    }
}

/// `target[key] = value` in strict mode code.
pub fn set_prop(target: &mut Value, key: &JsString, value: Value) -> Result<(), JsError> {
    match target {
        Value::Undefined | Value::Null => Err(JsError::type_error(message!(
            "Cannot set properties of ",
            primitive_name(target),
            " (setting '",
            key,
            "')",
        ))),
        Value::Number(_) | Value::Bool(_) => Err(JsError::type_error(message!(
            "Cannot create property '",
            key,
            "' on ",
            target.type_of(),
            " '",
            &target.to_js_string(),
            "'",
        ))),
        Value::String(s) => {
            let read_only = key.eq_str("length") || key.array_index().is_some_and(|i| (i as usize) < s.len());
            if read_only {
                Err(JsError::type_error(message!(
                    "Cannot assign to read only property '",
                    key,
                    "' of string '",
                    &*s,
                    "'",
                )))
            } else {
                Err(JsError::type_error(message!(
                    "Cannot create property '",
                    key,
                    "' on string '",
                    &*s,
                    "'"
                )))
            }
        }
        Value::Array(items) => {
            if let Some(index) = key.array_index() {
                let index = index as usize;
                if index >= items.len() {
                    items.resize(index + 1, Value::Undefined);
                }
                items[index] = value;
            } else if key.eq_str("length") {
                let length = value.to_number();
                if length < 0.0 || length.fract() != 0.0 || length > f64::from(u32::MAX) || length.is_nan() {
                    return Err(JsError::new("RangeError", "Invalid array length"));
                }
                items.resize(length as usize, Value::Undefined);
            }
            Ok(())
        }
        Value::Object(object) => {
            object.set(key.clone(), value);
            Ok(())
        }
        Value::Date(_) | Value::Function(..) => Ok(()),
    }
}

/// `delete target[key]` in strict mode code.
pub fn delete_prop(target: &mut Value, key: &JsString) -> Result<(), JsError> {
    let undeletable =
        |kind: &str| JsError::type_error(message!("Cannot delete property '", key, "' of [object ", kind, "]"));
    match target {
        Value::Object(object) => {
            object.remove(key);
            Ok(())
        }
        Value::Array(items) => {
            if key.eq_str("length") {
                return Err(undeletable("Array"));
            }
            if let Some(index) = key.array_index() {
                if let Some(slot) = items.get_mut(index as usize) {
                    *slot = Value::Undefined;
                }
            }
            Ok(())
        }
        Value::String(s) => {
            if key.eq_str("length") || key.array_index().is_some_and(|i| (i as usize) < s.len()) {
                Err(undeletable("String"))
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

/// The start index `Array.prototype.splice` derives from its first argument.
pub fn relative_index(start: &Value, length: usize) -> usize {
    let relative = to_integer_or_infinity(start.to_number());
    let length = length as f64;
    let index = if relative < 0.0 {
        (length + relative).max(0.0)
    } else {
        relative.min(length)
    };
    index as usize
}

/// `String(array.join(','))` element conversion: `undefined` and `null` become empty strings.
pub fn join_element(value: &Value) -> JsString {
    match value {
        Value::Undefined | Value::Null => JsString::new(),
        other => other.to_js_string(),
    }
}
