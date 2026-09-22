// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Library smoke runner, not the planned BlueJS shell/REPL.
//! cargo run -p blueice-bluejs --example run_bytecode -- path/to/script.js
use blueice_bluejs::{compile, parse, Vm};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = match std::env::args().nth(1) {
        Some(path) => std::fs::read_to_string(path)?,
        None => "let a=[1,2,3,4,5]; let sum=0; for(let i=0;i<a.length;i++){sum+=a[i];} sum".into(),
    };
    let bytecode = compile(&parse(&source).map_err(|error| format!("{error:?}"))?)?;
    let mut vm = Vm::default();
    println!("{:?}", vm.execute(&bytecode)?);
    Ok(())
}
