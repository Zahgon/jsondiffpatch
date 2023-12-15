// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! jsondiffpatch: diff and patch JSON values, with array move detection, text diffs and delta formatters. A port
//! of the JavaScript library jsondiffpatch 0.6.0 that reproduces its output exactly.

pub mod cli;
pub mod diff;
pub mod diffpatcher;
pub mod dmp;
pub mod formatters;
pub mod js;
pub mod options;
pub mod patch;
pub mod reverse;

pub use diffpatcher::{revive_dates, DiffPatcher};
pub use js::{JsError, JsString, Object, Value};
pub use options::Options;
