// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

const LOADER: PageHostDebuggerProgram = PageHostDebuggerProgram {
    program_handle: 11,
    program_generation: 13,
};
const EMITTED: PageHostDebuggerProgram = PageHostDebuggerProgram {
    program_handle: 17,
    program_generation: 19,
};
const TARGET: JavaScriptPageDebuggerSafePoint = JavaScriptPageDebuggerSafePoint {
    code_unit_ordinal: 2,
    bytecode_offset: 4,
};

#[derive(Clone, Copy)]
enum ArmReply {
    Exact,
    WrongTab,
    WrongGeneration,
    WrongLoader,
    WrongTarget,
    PrivateError,
}

struct SystemLoaderChild {
    lifecycle: RecordingChild,
    arm_reply: ArmReply,
    arm_calls: usize,
}

impl PageHostClient for SystemLoaderChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        self.lifecycle.synchronize_document(document)
    }

    fn close_realm(&mut self, tab_id: u64, generation: u64) -> io::Result<PageHostReply> {
        self.lifecycle.close_realm(tab_id, generation)
    }

    fn debugger_realm_stats(&mut self, tab_id: u64, generation: u64) -> io::Result<PageHostReply> {
        self.lifecycle.debugger_realm_stats(tab_id, generation)
    }

    fn debugger_execution_control_available(&self) -> bool {
        true
    }

    fn debugger_nested_frames_available(&self) -> bool {
        true
    }

    fn debugger_programs(&mut self, tab_id: u64, generation: u64) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerPrograms {
            tab_id,
            document_generation: generation,
            programs: vec![LOADER, EMITTED],
        })
    }

    fn validate_debugger_safe_point(
        &mut self,
        tab_id: u64,
        generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        assert_eq!(safe_point.program, EMITTED);
        assert_eq!(safe_point.code_unit_ordinal, TARGET.code_unit_ordinal);
        assert_eq!(safe_point.bytecode_offset, TARGET.bytecode_offset);
        Ok(PageHostReply::DebuggerSafePointValidated {
            tab_id,
            document_generation: generation,
            safe_point,
        })
    }

    fn arm_debugger_system_nested_breakpoint(
        &mut self,
        tab_id: u64,
        generation: u64,
        loader_program: PageHostDebuggerProgram,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        assert_eq!(loader_program, LOADER);
        assert_eq!(safe_point.program, EMITTED);
        self.arm_calls += 1;
        if matches!(self.arm_reply, ArmReply::PrivateError) {
            return Ok(PageHostReply::Error {
                code: page_host::PageHostErrorCode::InvalidDebuggerState,
                message: "not pending".to_string(),
            });
        }
        Ok(PageHostReply::DebuggerSystemNestedBreakpointArmed {
            tab_id: tab_id + u64::from(matches!(self.arm_reply, ArmReply::WrongTab)),
            document_generation: generation
                + u64::from(matches!(self.arm_reply, ArmReply::WrongGeneration)),
            loader_program: if matches!(self.arm_reply, ArmReply::WrongLoader) {
                EMITTED
            } else {
                loader_program
            },
            safe_point: if matches!(self.arm_reply, ArmReply::WrongTarget) {
                PageHostDebuggerSafePoint {
                    bytecode_offset: safe_point.bytecode_offset + 1,
                    ..safe_point
                }
            } else {
                safe_point
            },
        })
    }
}

fn setup(
    arm_reply: ArmReply,
) -> (
    OutOfProcessJavaScriptPageExecutor<SystemLoaderChild>,
    TabId,
    JavaScriptPageDebuggerProgram,
    JavaScriptPageDebuggerProgram,
) {
    let (tabs, tab_id) = loaded_tabs(
        "<script>globalThis.loader = true;</script><script type=\"application/x-blueice-typescript\">const value: number = 1;</script>",
        "https://example.test/system-loader.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
        SystemLoaderChild {
            lifecycle: RecordingChild::default(),
            arm_reply,
            arm_calls: 0,
        },
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    let programs = executor.debugger_programs(tab_id, 1).unwrap();
    assert_eq!(programs.len(), 2);
    assert_ne!(programs[0].program_handle, LOADER.program_handle);
    assert_ne!(programs[1].program_handle, EMITTED.program_handle);
    (executor, tab_id, programs[0], programs[1])
}

#[test]
fn core_system_loader_arm_remints_both_programs_and_requires_exact_child_acknowledgement() {
    for reply in [
        ArmReply::Exact,
        ArmReply::WrongTab,
        ArmReply::WrongGeneration,
        ArmReply::WrongLoader,
        ArmReply::WrongTarget,
        ArmReply::PrivateError,
    ] {
        let (mut executor, tab_id, loader, emitted) = setup(reply);
        let result =
            executor.arm_debugger_system_nested_breakpoint(tab_id, 1, loader, emitted, TARGET);
        let expected = match reply {
            ArmReply::Exact => Ok(()),
            ArmReply::PrivateError => Err(JavaScriptPageDebuggerError::InvalidExecutionState),
            _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
        };
        assert_eq!(result, expected);
        assert_eq!(executor.child.arm_calls, 1);
    }
}

#[test]
fn core_system_loader_arm_rejects_stale_or_same_program_before_contacting_child() {
    let (mut executor, tab_id, loader, emitted) = setup(ArmReply::Exact);
    let stale_loader = JavaScriptPageDebuggerProgram {
        program_generation: loader.program_generation + 1,
        ..loader
    };
    assert_eq!(
        executor.arm_debugger_system_nested_breakpoint(tab_id, 1, stale_loader, emitted, TARGET),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    assert_eq!(
        executor.arm_debugger_system_nested_breakpoint(tab_id, 1, emitted, emitted, TARGET),
        Err(JavaScriptPageDebuggerError::InvalidSafePoint)
    );
    assert_eq!(
        executor.arm_debugger_system_nested_breakpoint(
            tab_id,
            1,
            loader,
            emitted,
            JavaScriptPageDebuggerSafePoint {
                code_unit_ordinal: 0,
                ..TARGET
            },
        ),
        Err(JavaScriptPageDebuggerError::ExecutionControlUnavailable)
    );
    assert_eq!(executor.child.arm_calls, 0);
}
