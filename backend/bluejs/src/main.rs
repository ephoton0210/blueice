// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! BlueJS's standalone shell: `bluejs` opens a line REPL and
//! `bluejs script.js` executes a file in one persistent realm.

use blueice_bluejs::{Vm, run_batch, run_script_process};
use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::ExitCode;

fn execute(vm: &mut Vm, source: &str) -> Result<(), String> {
    let value = vm.evaluate(source).map_err(|error| error.to_string())?;
    for line in vm.take_output() {
        println!("{line}");
    }
    println!("{}", vm.format_value(&value));
    Ok(())
}

fn execute_batch(source: &str, stdout_only: bool) -> Result<(), String> {
    let result = run_batch(source).map_err(|error| error.to_string())?;
    for line in result.output {
        println!("{line}");
    }
    if !stdout_only {
        println!("{}", result.completion);
    }
    Ok(())
}

fn run_daemon(socket: &Path) -> Result<(), String> {
    let stream = UnixStream::connect(socket)
        .map_err(|error| format!("cannot connect script socket {}: {error}", socket.display()))?;
    run_script_process(stream)
}

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if let [flag, socket] = args.as_slice() {
        if flag == "--script-socket" {
            return match run_daemon(Path::new(socket)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("bluejs: {error}");
                    ExitCode::from(1)
                }
            };
        }
    }
    let (path, stdout_only) = match args.as_slice() {
        [path] => (Some(path.as_str()), false),
        [flag, path] if flag == "--stdout-only" => (Some(path.as_str()), true),
        [] => (None, false),
        _ => {
            eprintln!("usage: bluejs [--stdout-only] [script.js] | bluejs --script-socket <path>");
            return ExitCode::from(2);
        }
    };
    let mut vm = Vm::new();
    if let Some(path) = path {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("bluejs: cannot read {path}: {error}");
                return ExitCode::from(1);
            }
        };
        return match execute_batch(&source, stdout_only) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("bluejs: {error}");
                ExitCode::from(1)
            }
        };
    }

    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let mut stdout = io::stdout().lock();
    loop {
        if write!(stdout, "> ").and_then(|()| stdout.flush()).is_err() {
            return ExitCode::from(1);
        }
        let mut source = String::new();
        match stdin.read_line(&mut source) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => {
                eprintln!("bluejs: failed to read stdin: {error}");
                return ExitCode::from(1);
            }
        }
        if source.trim().is_empty() {
            continue;
        }
        if let Err(error) = execute(&mut vm, &source) {
            eprintln!("bluejs: {error}");
        }
    }
    ExitCode::SUCCESS
}
