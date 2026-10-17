// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Account copied diagnostic payloads while Rust frames own them temporarily.

use super::*;
use std::cell::Cell;

pub(super) struct DiagnosticPayloadCharge {
    counter: Rc<Cell<usize>>,
    bytes: usize,
}

impl DiagnosticPayloadCharge {
    pub(super) fn set_bytes(&mut self, bytes: usize) {
        self.counter.set(
            self.counter
                .get()
                .saturating_sub(self.bytes)
                .saturating_add(bytes),
        );
        self.bytes = bytes;
    }
}

impl Drop for DiagnosticPayloadCharge {
    fn drop(&mut self) {
        self.set_bytes(0);
    }
}

impl Vm {
    pub(super) fn charge_diagnostic_payload(&self, bytes: usize) -> DiagnosticPayloadCharge {
        let mut charge = DiagnosticPayloadCharge {
            counter: self.active_diagnostic_bytes.clone(),
            bytes: 0,
        };
        charge.set_bytes(bytes);
        charge
    }

    pub(super) fn diagnostic_origin_bytes(
        source: &Option<crate::diagnostics::DiagnosticSourceLocation>,
    ) -> usize {
        source
            .as_ref()
            .map_or(0, |source| source.filename.capacity())
    }

    #[cfg(any(test, coverage))]
    pub(super) fn diagnostic_handler_bytes(handlers: &[HandlerFrame]) -> usize {
        handlers.iter().fold(0usize, |bytes, handler| {
            bytes.saturating_add(handler.pending_origin.payload_bytes())
        })
    }
}
