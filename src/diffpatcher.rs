// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! The DiffPatcher (its diff, patch, reverse and unpatch pipes live in their own modules) and the date reviver.

use crate::dmp::DiffMatchPatch;
use crate::js::number::string_to_number;
use crate::js::{date, JsString, Object, Value};
use crate::options::Options;

/// Diffs, patches and reverses JSON values with the given options.
#[derive(Clone, Debug, Default)]
pub struct DiffPatcher {
    pub(crate) options: Options,
    pub(crate) dmp: DiffMatchPatch,
}

impl DiffPatcher {
    /// A DiffPatcher with these options (diff-match-patch is always available for text diffs).
    pub fn new(options: Options) -> Self {
        DiffPatcher {
            options,
            dmp: DiffMatchPatch::default(),
        }
    }

    /// The options in use.
    pub fn options(&self) -> &Options {
        &self.options
    }
}

/// Parses `YYYY-MM-DDTHH:mm:ss[.fraction](Z|±HH:mm)` as the date reviver's expression does; the captured
/// numbers, with the fraction's digits as they are (an empty capture when absent).
fn parse_date_string(text: &[u16]) -> Option<[JsString; 7]> {
    let digit = |i: usize| text.get(i).is_some_and(|u| (0x30..=0x39).contains(u));
    let at = |i: usize, unit: u8| text.get(i) == Some(&u16::from(unit));
    let take = |from: usize, to: usize| JsString::from_units(text[from..to].to_vec());
    let fixed = [(0, 4), (5, 7), (8, 10), (11, 13), (14, 16), (17, 19)];
    if !(fixed.iter().all(|&(a, b)| (a..b).all(digit))
        && at(4, b'-')
        && at(7, b'-')
        && at(10, b'T')
        && at(13, b':')
        && at(16, b':'))
    {
        return None;
    }
    let mut i = 19;
    let mut fraction = JsString::new();
    if at(i, b'.') {
        let start = i + 1;
        i = start;
        while digit(i) {
            i += 1;
        }
        fraction = take(start, i);
    }
    let zone_ok = if at(i, b'Z') {
        i + 1 == text.len()
    } else if at(i, b'+') || at(i, b'-') {
        digit(i + 1) && digit(i + 2) && at(i + 3, b':') && digit(i + 4) && digit(i + 5) && i + 6 == text.len()
    } else {
        false
    };
    if !zone_ok {
        return None;
    }
    Some([
        take(0, 4),
        take(5, 7),
        take(8, 10),
        take(11, 13),
        take(14, 16),
        take(17, 19),
        fraction,
    ])
}

/// jsondiffpatch's `dateReviver`, applied to a whole value as `JSON.parse(JSON.stringify(value), dateReviver)`
/// does: every string in UTC ISO-like form becomes a `Date` (the time zone offset is ignored, the fraction is a
/// number of milliseconds), and `-0` becomes `0`.
pub fn revive_dates(value: &Value) -> Value {
    match value {
        Value::String(text) => match parse_date_string(text.units()) {
            Some([year, month, day, hours, minutes, seconds, fraction]) => {
                let number = |s: &JsString| string_to_number(s);
                let ms = if fraction.is_empty() { 0.0 } else { number(&fraction) };
                Value::Date(date::utc(
                    number(&year),
                    number(&month) - 1.0,
                    number(&day),
                    number(&hours),
                    number(&minutes),
                    number(&seconds),
                    ms,
                ))
            }
            None => value.clone(),
        },
        Value::Number(n) if *n == 0.0 => Value::Number(0.0),
        Value::Number(n) if !n.is_finite() => Value::Null,
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| match item {
                    Value::Undefined | Value::Function(..) => Value::Null,
                    other => revive_dates(other),
                })
                .collect(),
        ),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(_, v)| !matches!(v, Value::Undefined | Value::Function(..)))
                .map(|(k, v)| (k.clone(), revive_dates(v)))
                .collect::<Object>(),
        ),
        Value::Date(t) if t.is_finite() => revive_dates(&Value::String(JsString::from(date::to_iso_string(*t)))),
        Value::Date(_) => Value::Null,
        other => other.clone(),
    }
}
