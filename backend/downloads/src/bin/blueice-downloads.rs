// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_downloads::cli::{parse_args, parse_credential_args, run, set_credential_from_stdin};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut supplied = std::env::args().skip(1);
    if matches!(supplied.next().as_deref(), Some("credential")) {
        let args = match parse_credential_args(supplied) {
            Ok(args) => args,
            Err(message) => {
                eprintln!("blueice-downloads: {message}");
                return ExitCode::from(2);
            }
        };
        return match set_credential_from_stdin(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("blueice-downloads: could not store credential: {error}");
                ExitCode::FAILURE
            }
        };
    }
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("blueice-downloads: {message}");
            return ExitCode::from(2);
        }
    };
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("blueice-downloads: {e}");
            ExitCode::FAILURE
        }
    }
}
