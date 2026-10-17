// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native rejection provenance, separate from the JavaScript rejection value.

use super::*;

impl PromiseJob {
    pub(super) fn rejection_origin(&self) -> Option<&ThrowOrigin> {
        match self {
            Self::Reaction { origin, .. }
            | Self::ModuleAwait { origin, .. }
            | Self::AsyncAwait { origin, .. }
            | Self::AsyncGeneratorYield { origin, .. }
            | Self::AsyncGeneratorDelegate { origin, .. } => Some(origin),
            _ => None,
        }
    }
}

impl Vm {
    pub(super) fn propagate_module_error(&mut self, error: ModuleEvaluationError) -> RuntimeError {
        self.publish_rejection_origin(error.origin.clone());
        RuntimeError::Thrown(error.value.clone())
    }

    pub(super) fn current_rejection_origin(&self) -> ThrowOrigin {
        self.promise_origin_override
            .clone()
            .unwrap_or_else(|| ThrowOrigin {
                source: self.current_diagnostic_source_location(),
                ..ThrowOrigin::default()
            })
    }

    /// A newer language throw wins even when its source is explicitly unknown.
    /// Native failures without a new throw use the caller's certified site.
    pub(super) fn caught_rejection_origin(&self, previous_epoch: u64) -> ThrowOrigin {
        if self.throw_epoch != previous_epoch {
            ThrowOrigin {
                source: self.pending_diagnostic_origin.clone(),
                debugger_site: self.pending_throw_site,
                epoch: self.throw_epoch,
            }
        } else {
            self.current_rejection_origin()
        }
    }

    pub(super) fn publish_rejection_origin(&mut self, mut origin: ThrowOrigin) {
        origin.epoch = self.throw_epoch.wrapping_add(1);
        self.restore_pending_throw_origin(origin);
    }

    /// Keep displaced metadata charged while native callbacks can allocate.
    pub(super) fn with_rejection_origin<T>(
        &mut self,
        origin: Option<ThrowOrigin>,
        callback: impl FnOnce(&mut Self) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        let previous = std::mem::replace(&mut self.promise_origin_override, origin);
        let bytes = previous.as_ref().map_or(0, ThrowOrigin::payload_bytes);
        self.active_scheduling_bytes = self.active_scheduling_bytes.saturating_add(bytes);
        let added_bytes = self
            .promise_origin_override
            .as_ref()
            .map_or(0, ThrowOrigin::payload_bytes);
        let result = if added_bytes == 0 {
            callback(self)
        } else {
            self.synchronize_scheduling_usage()
                .and_then(|()| callback(self))
        };
        self.promise_origin_override = previous;
        self.active_scheduling_bytes = self.active_scheduling_bytes.saturating_sub(bytes);
        result
    }

    pub(super) fn reject_promise_with_origin(
        &mut self,
        promise: ObjectId,
        value: Value,
        origin: ThrowOrigin,
    ) -> Result<(), RuntimeError> {
        self.with_rejection_origin(Some(origin), |vm| {
            vm.settle_promise(promise, PromiseStatus::Rejected(value))
        })
    }
}
