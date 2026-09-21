// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A manual scratch runner, not a regression test: it executes the JavaScript
//! file named by the `PROBE` environment variable through the public
//! `parse`/`compile`/`Vm::execute` path and prints the completion value (or
//! the error), so a Temporal behaviour can be inspected without writing a new
//! test first.
//!
//! It asserts nothing and needs `PROBE`, so it is `#[ignore]`d: an ordinary
//! `cargo test --workspace` (and CI) reports it as ignored rather than failing
//! on the missing variable. Run it deliberately with
//!
//! ```sh
//! PROBE=/path/to/script.js cargo test -p blueice-bluejs \
//!     --test coverage_temporal_zoned_probe -- --ignored --nocapture
//! ```

use blueice_bluejs::{compile, parse, Value, Vm};

#[test]
#[ignore = "manual scratch runner: set PROBE to a script path and pass --ignored"]
fn probe() {
    let path = std::env::var("PROBE").expect("set PROBE to the path of a JavaScript file");
    let source = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let r = Vm::default().execute(&compile(&parse(&source).unwrap()).unwrap());
    match r {
        Ok(Value::String(s)) => println!("PROBE-OK\n{}", s.to_utf8().unwrap()),
        Ok(v) => println!("PROBE-OK {v:?}"),
        Err(e) => println!("PROBE-ERR {e:?}"),
    }
}
