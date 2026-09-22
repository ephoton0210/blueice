// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The reusable implementation behind `bluejs script.js` and MCP's
//! `bluejs_run` adapter. Keeping it here means those surfaces cannot drift
//! into two subtly different evaluators.

use crate::{Vm, VmError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchResult {
    pub output: Vec<String>,
    pub completion: String,
}

pub fn run_batch(source: &str) -> Result<BatchResult, VmError> {
    let mut vm = Vm::new();
    let value = vm.evaluate(source)?;
    Ok(BatchResult {
        output: vm.take_output(),
        completion: vm.format_value(&value),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_result_matches_the_shells_console_then_completion_contract() {
        assert_eq!(
            run_batch("console.log('hello'); 2 + 3;").unwrap(),
            BatchResult {
                output: vec!["hello".to_string()],
                completion: "5".to_string()
            }
        );
    }
}
