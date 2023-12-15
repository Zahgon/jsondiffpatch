// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

use std::io::{self, Read, Write};
use std::process::ExitCode;

use jsondiffpatch::cli;

fn main() -> ExitCode {
    if std::env::args_os().len() > 1 {
        eprint!("{}", cli::USAGE);
        return ExitCode::from(2);
    }
    let mut input = Vec::new();
    if let Err(error) = io::stdin().read_to_end(&mut input) {
        eprintln!("jsondiffpatch: cannot read standard input: {error}");
        return ExitCode::from(1);
    }
    let (output, status) = cli::run(&input);
    let mut stdout = io::stdout().lock();
    if stdout.write_all(&output).and_then(|()| stdout.flush()).is_err() {
        return ExitCode::from(1);
    }
    ExitCode::from(status as u8)
}
