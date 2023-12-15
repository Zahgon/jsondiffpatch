// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! JavaScript values as the library sees them: JSON values, `undefined`, `Date` objects, and the inherited methods
//! of `Object.prototype` that a property read on a plain object can find.

use super::date;
use super::number::{number_to_string, string_to_number};
use super::string::JsString;

/// The methods every object inherits from `Object.prototype`.
const OBJECT_PROTOTYPE: &[&str] = &[
    "constructor",
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "toLocaleString",
];

const ARRAY_PROTOTYPE: &[&str] = &[
    "constructor",
    "at",
    "concat",
    "copyWithin",
    "fill",
    "find",
    "findIndex",
    "findLast",
    "findLastIndex",
    "lastIndexOf",
    "pop",
    "push",
    "reverse",
    "shift",
    "unshift",
    "slice",
    "sort",
    "splice",
    "includes",
    "indexOf",
    "join",
    "keys",
    "entries",
    "values",
    "forEach",
    "filter",
    "flat",
    "flatMap",
    "map",
    "every",
    "some",
    "reduce",
    "reduceRight",
    "toLocaleString",
    "toString",
    "toReversed",
    "toSorted",
    "toSpliced",
    "with",
];

const STRING_PROTOTYPE: &[&str] = &[
    "constructor",
    "anchor",
    "at",
    "big",
    "blink",
    "bold",
    "charAt",
    "charCodeAt",
    "codePointAt",
    "concat",
    "endsWith",
    "fontcolor",
    "fontsize",
    "fixed",
    "includes",
    "indexOf",
    "isWellFormed",
    "italics",
    "lastIndexOf",
    "link",
    "localeCompare",
    "match",
    "matchAll",
    "normalize",
    "padEnd",
    "padStart",
    "repeat",
    "replace",
    "replaceAll",
    "search",
    "slice",
    "small",
    "split",
    "strike",
    "sub",
    "substr",
    "substring",
    "sup",
    "startsWith",
    "toString",
    "toWellFormed",
    "trim",
    "trimStart",
    "trimLeft",
    "trimEnd",
    "trimRight",
    "toLocaleLowerCase",
    "toLocaleUpperCase",
    "toLowerCase",
    "toUpperCase",
    "valueOf",
];

const NUMBER_PROTOTYPE: &[&str] = &[
    "constructor",
    "toExponential",
    "toFixed",
    "toPrecision",
    "toString",
    "toLocaleString",
    "valueOf",
];

const BOOLEAN_PROTOTYPE: &[&str] = &["constructor", "toString", "valueOf"];

const DATE_PROTOTYPE: &[&str] = &[
    "constructor",
    "toString",
    "toDateString",
    "toTimeString",
    "toISOString",
    "toUTCString",
    "toGMTString",
    "getDate",
    "setDate",
    "getDay",
    "getFullYear",
    "setFullYear",
    "getHours",
    "setHours",
    "getMilliseconds",
    "setMilliseconds",
    "getMinutes",
    "setMinutes",
    "getMonth",
    "setMonth",
    "getSeconds",
    "setSeconds",
    "getTime",
    "setTime",
    "getTimezoneOffset",
    "getUTCDate",
    "setUTCDate",
    "getUTCDay",
    "getUTCFullYear",
    "setUTCFullYear",
    "getUTCHours",
    "setUTCHours",
    "getUTCMilliseconds",
    "setUTCMilliseconds",
    "getUTCMinutes",
    "setUTCMinutes",
    "getUTCMonth",
    "setUTCMonth",
    "getUTCSeconds",
    "setUTCSeconds",
    "valueOf",
    "getYear",
    "setYear",
    "toJSON",
    "toLocaleString",
    "toLocaleDateString",
    "toLocaleTimeString",
];

/// Whether reading `key` from a plain object that lacks it finds an inherited method.
pub fn is_inherited_name(key: &JsString) -> bool {
    OBJECT_PROTOTYPE.iter().any(|name| key.eq_str(name))
}

/// The inherited method a value of the prototype `owner` (with its own methods `own`) has under `key`.
fn inherited_from(owner: &'static str, own: &'static [&'static str], key: &JsString) -> Value {
    if let Some(name) = own.iter().find(|name| key.eq_str(name)) {
        return Value::Function(owner, name);
    }
    match OBJECT_PROTOTYPE.iter().find(|name| key.eq_str(name)) {
        Some(name) => Value::Function("Object", name),
        None => Value::Undefined,
    }
}

/// A JavaScript value.
#[derive(Clone, Debug, Default)]
pub enum Value {
    /// `undefined`; also an array hole.
    #[default]
    Undefined,
    /// `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A number.
    Number(f64),
    /// A string.
    String(JsString),
    /// An array.
    Array(Vec<Value>),
    /// A plain object.
    Object(Object),
    /// A `Date` object holding its time value (`NaN` for an invalid date).
    Date(f64),
    /// A built-in method read through a property the value does not own: the prototype's owner (`Object`,
    /// `Array`, `String`, ...) and the method's name (`constructor` for the constructor itself).
    Function(&'static str, &'static str),
}

/// A plain object: properties in insertion order, enumerated in JavaScript order (array index keys first, in
/// ascending numeric order, then the other keys in insertion order).
#[derive(Clone, Debug, Default)]
pub struct Object {
    entries: Vec<(JsString, Value)>,
}

impl Object {
    /// An empty object.
    pub fn new() -> Self {
        Object::default()
    }

    /// The own property `key`, if present.
    pub fn get(&self, key: &JsString) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// A mutable reference to the own property `key`, if present.
    pub fn get_mut(&mut self, key: &JsString) -> Option<&mut Value> {
        self.entries.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Whether `key` is an own property.
    pub fn has_own(&self, key: &JsString) -> bool {
        self.entries.iter().any(|(k, _)| k == key)
    }

    /// Sets `key`, keeping its place when it already exists.
    pub fn set(&mut self, key: JsString, value: Value) {
        match self.get_mut(&key) {
            Some(slot) => *slot = value,
            None => self.entries.push((key, value)),
        }
    }

    /// Deletes the own property `key`.
    pub fn remove(&mut self, key: &JsString) -> Option<Value> {
        let index = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(index).1)
    }

    /// The number of own properties.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the object has no own properties.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The own keys in JavaScript enumeration order (`for...in`, `Object.keys`, `JSON.stringify`).
    pub fn keys(&self) -> Vec<JsString> {
        self.iter().map(|(k, _)| k.clone()).collect()
    }

    /// The own properties in JavaScript enumeration order.
    pub fn iter(&self) -> impl Iterator<Item = (&JsString, &Value)> {
        let mut indexed: Vec<(u32, usize)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, (k, _))| k.array_index().map(|n| (n, i)))
            .collect();
        indexed.sort_unstable();
        let named = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, (k, _))| k.array_index().is_none())
            .map(|(i, _)| i);
        let order: Vec<usize> = indexed.into_iter().map(|(_, i)| i).chain(named).collect();
        order.into_iter().map(move |i| (&self.entries[i].0, &self.entries[i].1))
    }
}

impl FromIterator<(JsString, Value)> for Object {
    fn from_iter<I: IntoIterator<Item = (JsString, Value)>>(iter: I) -> Self {
        let mut object = Object::new();
        for (k, v) in iter {
            object.set(k, v);
        }
        object
    }
}

impl Value {
    /// A string value from Rust text.
    pub fn str(text: &str) -> Value {
        Value::String(JsString::from(text))
    }

    /// `typeof value`.
    pub fn type_of(&self) -> &'static str {
        match self {
            Value::Undefined => "undefined",
            Value::Null | Value::Array(_) | Value::Object(_) | Value::Date(_) => "object",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Function(..) => "function",
        }
    }

    /// Whether the value is `undefined`.
    pub fn is_undefined(&self) -> bool {
        matches!(self, Value::Undefined)
    }

    /// Whether the value is an object of any kind (`typeof` "object" and not null).
    pub fn is_object(&self) -> bool {
        matches!(self, Value::Array(_) | Value::Object(_) | Value::Date(_))
    }

    /// The array, when the value is one.
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    /// The plain object, when the value is one.
    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(object) => Some(object),
            _ => None,
        }
    }

    /// The number, when the value is one.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// The string, when the value is one.
    pub fn as_string(&self) -> Option<&JsString> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// JavaScript truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => !(n.is_nan() || *n == 0.0),
            Value::String(s) => !s.is_empty(),
            _ => true,
        }
    }

    /// Strict equality (`===`). Objects are equal only to themselves, and every object value this crate holds is
    /// a distinct object, so objects are never strictly equal to one another.
    pub fn strict_equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Function(a, m), Value::Function(b, n)) => a == b && m == n,
            _ => false,
        }
    }

    /// `value[key]`, reading own properties, string and array indices and lengths, and the inherited methods of a
    /// plain object. Reading a property of `undefined` or `null` is a `TypeError` the caller reports.
    pub fn get(&self, key: &JsString) -> Value {
        match self {
            Value::Object(object) => match object.get(key) {
                Some(value) => value.clone(),
                None => inherited_from("Object", &[], key),
            },
            Value::Array(items) => {
                if key.eq_str("length") {
                    return Value::Number(items.len() as f64);
                }
                match key.array_index() {
                    Some(i) => items.get(i as usize).cloned().unwrap_or_default(),
                    None => inherited_from("Array", ARRAY_PROTOTYPE, key),
                }
            }
            Value::String(s) => {
                if key.eq_str("length") {
                    return Value::Number(s.len() as f64);
                }
                match key.array_index().and_then(|i| s.unit_at(i as usize)) {
                    Some(unit) => Value::String(JsString::from_unit(unit)),
                    None => inherited_from("String", STRING_PROTOTYPE, key),
                }
            }
            Value::Number(_) => inherited_from("Number", NUMBER_PROTOTYPE, key),
            Value::Bool(_) => inherited_from("Boolean", BOOLEAN_PROTOTYPE, key),
            Value::Date(_) => inherited_from("Date", DATE_PROTOTYPE, key),
            Value::Undefined | Value::Null | Value::Function(..) => Value::Undefined,
        }
    }

    /// `Object.prototype.hasOwnProperty.call(value, key)`.
    pub fn has_own(&self, key: &JsString) -> bool {
        match self {
            Value::Object(object) => object.has_own(key),
            Value::Array(items) => {
                key.eq_str("length")
                    || key
                        .array_index()
                        .is_some_and(|i| (i as usize) < items.len() && !items[i as usize].is_undefined())
            }
            Value::String(s) => key.eq_str("length") || key.array_index().is_some_and(|i| (i as usize) < s.len()),
            _ => false,
        }
    }

    /// The keys `for (name in value)` visits: own enumerable properties (string and array indices included).
    pub fn for_in_keys(&self) -> Vec<JsString> {
        match self {
            Value::Object(object) => object.keys(),
            Value::Array(items) => items
                .iter()
                .enumerate()
                .filter(|(_, v)| !v.is_undefined())
                .map(|(i, _)| JsString::from(i.to_string()))
                .collect(),
            Value::String(s) => (0..s.len()).map(|i| JsString::from(i.to_string())).collect(),
            _ => Vec::new(),
        }
    }

    /// `ToNumber(value)` for the primitive values and arrays a delta can hold.
    pub fn to_number(&self) -> f64 {
        match self {
            Value::Undefined | Value::Function(..) => f64::NAN,
            Value::Null => 0.0,
            Value::Bool(b) => f64::from(u8::from(*b)),
            Value::Number(n) => *n,
            Value::String(s) => string_to_number(s),
            Value::Date(t) => *t,
            Value::Array(_) | Value::Object(_) => string_to_number(&self.to_js_string()),
        }
    }

    /// `String(value)`, as a template literal or string concatenation converts it.
    pub fn to_js_string(&self) -> JsString {
        match self {
            Value::Undefined => JsString::from("undefined"),
            Value::Null => JsString::from("null"),
            Value::Bool(b) => JsString::from(if *b { "true" } else { "false" }),
            Value::Number(n) => JsString::from(number_to_string(*n)),
            Value::String(s) => s.clone(),
            Value::Array(items) => {
                let parts: Vec<JsString> = items
                    .iter()
                    .map(|item| match item {
                        Value::Undefined | Value::Null => JsString::new(),
                        other => other.to_js_string(),
                    })
                    .collect();
                JsString::join(&parts, ",")
            }
            Value::Object(_) => JsString::from("[object Object]"),
            Value::Date(t) => JsString::from(date::to_date_string(*t)),
            Value::Function(owner, name) => {
                let name = if *name == "constructor" { owner } else { name };
                JsString::from(format!("function {name}() {{ [native code] }}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_follow_javascript_order() {
        let object: Object = ["b", "4294967295", "4294967294", "01", "1", "-1", "0", "a", "10", "9"]
            .into_iter()
            .map(|k| (JsString::from(k), Value::Null))
            .collect();
        let keys: Vec<String> = object.keys().iter().map(JsString::to_string_lossy).collect();
        assert_eq!(
            keys,
            ["0", "1", "9", "10", "4294967294", "b", "4294967295", "01", "-1", "a"]
        );
    }

    #[test]
    fn converts_to_string() {
        let array = Value::Array(vec![Value::Number(1.0), Value::Null, Value::str("x")]);
        assert_eq!(array.to_js_string(), JsString::from("1,,x"));
        assert_eq!(Value::Number(-0.0).to_js_string(), JsString::from("0"));
    }

    #[test]
    fn reads_inherited_names() {
        let object = Value::Object(Object::new());
        assert_eq!(object.get(&JsString::from("toString")).type_of(), "function");
        assert!(object.get(&JsString::from("missing")).is_undefined());
    }
}
