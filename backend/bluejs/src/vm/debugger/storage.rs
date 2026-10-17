// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Saved debugger state keeps native credit outside its originating VM.

use super::*;

impl Vm {
    pub(in crate::vm) fn prepare_debugger_module_pause_request(
        &self,
        entry: &str,
        target: usize,
    ) -> Result<ModuleDebuggerPauseRequest, RuntimeError> {
        Ok(ModuleDebuggerPauseRequest {
            entry: super::super::module_name_storage::ModuleName::copy(
                entry,
                self.host_scheduling.as_ref(),
            )?,
            target,
        })
    }

    #[cfg(any(test, coverage))]
    pub(in crate::vm) fn retained_debugger_record_bytes(&self) -> usize {
        self.debugger_continuation.as_ref().map_or(0, |frame| {
            frame.iterators.retained_native_bytes() + frame.handlers.retained_native_bytes()
        })
    }

    pub(in crate::vm) fn prepare_debugger_saved_code(
        &mut self,
        code: &Bytecode,
        header: usize,
        iterators: &mut IteratorRecords,
        handlers: &mut HandlerRecords,
    ) -> Result<
        (
            SavedRootCode,
            Option<host_scheduling::SharedSchedulingCharge>,
        ),
        RuntimeError,
    > {
        let storage = self.prepare_debugger_control_storage(header)?;
        let code = SavedRootCode::copy(code, self)?;
        iterators.retain_native_storage(self)?;
        handlers.retain_native_storage(self)?;
        Ok((code, storage))
    }

    pub(in crate::vm) fn prepare_debugger_child_storage(
        &mut self,
        code: &Bytecode,
        header: usize,
        at: usize,
        iterators: &mut IteratorRecords,
        handlers: &mut HandlerRecords,
    ) -> Result<
        (
            SavedRootCode,
            Option<host_scheduling::SharedSchedulingCharge>,
            SuspendedModuleExecution,
        ),
        RuntimeError,
    > {
        let (code, storage) =
            self.prepare_debugger_saved_code(code, header, iterators, handlers)?;
        let execution = self.capture_suspension_execution(at)?;
        Ok((code, storage, execution))
    }

    pub(in crate::vm) fn retain_debugger_execution_storage(
        &mut self,
        execution: &mut SuspendedModuleExecution,
        iterators: &mut IteratorRecords,
        handlers: &mut HandlerRecords,
    ) -> Result<(), RuntimeError> {
        iterators.retain_native_storage(self)?;
        handlers.retain_native_storage(self)?;
        execution.retain_native_storage(self)
    }

    pub(in crate::vm) fn debugger_root_storage_bytes() -> usize {
        std::mem::size_of::<DebuggerContinuation>() - std::mem::size_of::<SavedRootCode>()
    }

    pub(in crate::vm) fn prepare_debugger_control_storage(
        &self,
        bytes: usize,
    ) -> Result<Option<host_scheduling::SharedSchedulingCharge>, RuntimeError> {
        self.host_scheduling
            .as_ref()
            .map(|scope| scope.shared_charge(bytes))
            .transpose()
    }

    pub(in crate::vm) fn attach_retained_debugger_storage(&mut self) -> Result<(), RuntimeError> {
        if let Some(request) = &mut self.debugger_module_pause_request {
            request.entry.attach(self.host_scheduling.as_ref())?;
        }

        if let Some(mut frame) = self.debugger_continuation.take() {
            let result = self.attach_root_debugger_storage(&mut frame);
            self.debugger_continuation = Some(frame);
            result?;
        }
        if let Some(mut frame) = self.debugger_nested_continuation.take() {
            let result = self.attach_nested_debugger_storage(&mut frame);
            self.debugger_nested_continuation = Some(frame);
            result?;
        }
        if let Some(mut frame) = self.debugger_nested_intermediate_continuation.take() {
            let result = self.attach_intermediate_debugger_storage(&mut frame);
            self.debugger_nested_intermediate_continuation = Some(frame);
            result?;
        }
        if let Some(mut frame) = self.debugger_module_continuation.take() {
            let result = self.attach_module_debugger_storage(&mut frame);
            self.debugger_module_continuation = Some(frame);
            result?;
        }
        Ok(())
    }

    fn attach_debugger_control_storage(
        &self,
        storage: &mut Option<host_scheduling::SharedSchedulingCharge>,
        bytes: usize,
    ) -> Result<(), RuntimeError> {
        if let Some(scope) = &self.host_scheduling {
            if let Some(owner) = storage {
                owner.reserve_peer(0)?;
                if !owner.belongs_to(scope) {
                    return Err(scope.fail());
                }
            } else {
                *storage = self.prepare_debugger_control_storage(bytes)?;
            }
        }
        Ok(())
    }

    fn attach_root_debugger_storage(
        &mut self,
        frame: &mut DebuggerContinuation,
    ) -> Result<(), RuntimeError> {
        if let Some(scope) = &self.host_scheduling {
            let budget = scope.heap_native_budget();
            frame.code.attach(&budget, &mut self.heap)?;
        }
        self.attach_debugger_control_storage(
            &mut frame._storage,
            Self::debugger_root_storage_bytes(),
        )?;
        frame.iterators.retain_native_storage(self)?;
        frame.handlers.retain_native_storage(self)
    }

    fn attach_nested_debugger_storage(
        &mut self,
        frame: &mut NestedDebuggerContinuation,
    ) -> Result<(), RuntimeError> {
        if let Some(scope) = &self.host_scheduling {
            frame
                .code
                .attach(&scope.heap_native_budget(), &mut self.heap)?;
        }
        self.attach_debugger_control_storage(
            &mut frame._storage,
            std::mem::size_of::<NestedDebuggerContinuation>()
                - std::mem::size_of::<SavedRootCode>(),
        )?;
        self.retain_debugger_execution_storage(
            &mut frame.execution,
            &mut frame.iterators,
            &mut frame.handlers,
        )
    }

    fn attach_intermediate_debugger_storage(
        &mut self,
        frame: &mut IntermediateDebuggerContinuation,
    ) -> Result<(), RuntimeError> {
        if let Some(scope) = &self.host_scheduling {
            frame
                .code
                .attach(&scope.heap_native_budget(), &mut self.heap)?;
        }
        self.attach_debugger_control_storage(
            &mut frame._storage,
            std::mem::size_of::<IntermediateDebuggerContinuation>()
                - std::mem::size_of::<SavedRootCode>(),
        )?;
        self.retain_debugger_execution_storage(
            &mut frame.execution,
            &mut frame.iterators,
            &mut frame.handlers,
        )
    }

    fn attach_module_debugger_storage(
        &mut self,
        frame: &mut ModuleDebuggerContinuation,
    ) -> Result<(), RuntimeError> {
        if let Some(scope) = &self.host_scheduling {
            frame.code.registry().attach(scope)?;
        }
        self.attach_debugger_control_storage(
            &mut frame._storage,
            std::mem::size_of::<ModuleDebuggerContinuation>(),
        )?;
        frame
            .module_charge
            .retain_native(self, frame.module.capacity())?;
        self.retain_debugger_execution_storage(
            &mut frame.execution,
            &mut frame.iterators,
            &mut frame.handlers,
        )
    }
}
