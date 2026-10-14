// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn owner_page_console_ambient_selects_only_emitted_classic_documents() {
    let (tabs, _) = loaded_tabs(
        "<script type=\"application/x-blueice-typescript\">const answer: number = 42;</script>",
        "https://example.test/classic-console.html",
    );
    let mut standard = OutOfProcessJavaScriptPageExecutor::new(RecordingChild::default());
    standard.enable_emitted_blue_ts_classic();
    standard.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(standard.child.documents.len(), 1);
    assert!(!standard.child.documents[0].emitted_classic_page_console_ambient);

    let mut selected = OutOfProcessJavaScriptPageExecutor::new(RecordingChild::default());
    selected.enable_emitted_blue_ts_classic_page_console_ambient();
    selected.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(selected.child.documents.len(), 1);
    let document = &selected.child.documents[0];
    assert!(document.emitted_classic_page_console_ambient);
    assert!(!document.emitted_module_page_console_ambient);
    assert_eq!(document.scripts.len(), 1);
    assert_eq!(
        document.scripts[0].language,
        page_host::PageHostScriptLanguage::BlueTsEmitted
    );
    assert_eq!(
        document.scripts[0].kind,
        page_host::PageHostScriptKind::Classic
    );
}

#[test]
fn owner_page_console_ambient_selects_only_emitted_module_documents() {
    let (tabs, _) = loaded_tabs(
        "<script type=\"application/x-blueice-typescript-module\">export const answer: number = 42;</script>",
        "https://example.test/module-console.html",
    );
    let mut standard = OutOfProcessJavaScriptPageExecutor::new(RecordingChild::default());
    standard.enable_emitted_blue_ts_module();
    standard.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(standard.child.documents.len(), 1);
    assert!(!standard.child.documents[0].emitted_module_page_console_ambient);

    let mut selected = OutOfProcessJavaScriptPageExecutor::new(RecordingChild::default());
    selected.enable_emitted_blue_ts_module_page_console_ambient();
    selected.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(selected.child.documents.len(), 1);
    let document = &selected.child.documents[0];
    assert!(document.emitted_module_page_console_ambient);
    assert_eq!(document.scripts.len(), 1);
    assert_eq!(
        document.scripts[0].language,
        page_host::PageHostScriptLanguage::BlueTsEmitted
    );
    assert_eq!(
        document.scripts[0].kind,
        page_host::PageHostScriptKind::Module
    );
    assert!(!selected.emitted_blue_ts_classic);
}

const CHILD_PROGRAM: PageHostDebuggerProgram = PageHostDebuggerProgram {
    program_handle: 11,
    program_generation: 13,
};

struct EmittedProbeChild {
    lifecycle: RecordingChild,
    wrong_reply_generation: bool,
    malformed_map_failure: bool,
}

impl EmittedProbeChild {
    fn reply_program(&self) -> PageHostDebuggerProgram {
        PageHostDebuggerProgram {
            program_generation: CHILD_PROGRAM.program_generation
                + u64::from(self.wrong_reply_generation),
            ..CHILD_PROGRAM
        }
    }
}

impl PageHostClient for EmittedProbeChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        self.lifecycle.synchronize_document(document)
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        self.lifecycle.close_realm(tab_id, document_generation)
    }

    fn debugger_realm_stats(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        self.lifecycle
            .debugger_realm_stats(tab_id, document_generation)
    }

    fn debugger_programs(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerPrograms {
            tab_id,
            document_generation,
            programs: vec![CHILD_PROGRAM],
        })
    }

    fn debugger_emitted_map_available(&self) -> bool {
        true
    }

    fn debugger_emitted_paused_root_location_available(&self) -> bool {
        true
    }

    fn debugger_execution_control_available(&self) -> bool {
        true
    }

    fn debugger_emitted_original_position(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        _program: PageHostDebuggerProgram,
        generated_line: u32,
        generated_column_utf16: u32,
    ) -> io::Result<PageHostReply> {
        if self.malformed_map_failure {
            return Ok(PageHostReply::Error {
                code: page_host::PageHostErrorCode::HostFailure,
                message: "private map decode failed".to_string(),
            });
        }
        Ok(PageHostReply::DebuggerEmittedOriginalPosition {
            tab_id,
            document_generation,
            program: self.reply_program(),
            generated_line,
            generated_column_utf16,
            original: Some(page_host::PageHostDebuggerEmittedOriginalPosition {
                source_index: 0,
                line: 2,
                column_utf16: 3,
            }),
        })
    }

    fn debugger_emitted_safe_point(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        _program: PageHostDebuggerProgram,
        generated_line: u32,
        generated_column_utf16: u32,
    ) -> io::Result<PageHostReply> {
        if self.malformed_map_failure {
            return Ok(PageHostReply::Error {
                code: page_host::PageHostErrorCode::HostFailure,
                message: "private map decode failed".to_string(),
            });
        }
        Ok(PageHostReply::DebuggerEmittedSafePoint {
            tab_id,
            document_generation,
            program: self.reply_program(),
            generated_line,
            generated_column_utf16,
            safe_point: Some(PageHostDebuggerSafePoint {
                program: CHILD_PROGRAM,
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            }),
        })
    }

    fn debugger_emitted_paused_root_location(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        _program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        if self.malformed_map_failure {
            return Ok(PageHostReply::Error {
                code: page_host::PageHostErrorCode::HostFailure,
                message: "private map decode failed".to_string(),
            });
        }
        Ok(PageHostReply::DebuggerEmittedPausedRootLocation {
            tab_id,
            document_generation,
            program: self.reply_program(),
            safe_point: PageHostDebuggerSafePoint {
                program: CHILD_PROGRAM,
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
            location: Some(page_host::PageHostDebuggerEmittedPausedRootLocation {
                generated_line: 5,
                generated_column_utf16: 8,
                original: Some(page_host::PageHostDebuggerEmittedOriginalPosition {
                    source_index: 0,
                    line: 2,
                    column_utf16: 3,
                }),
            }),
        })
    }
}

#[test]
fn core_adapter_remints_emitted_positions_and_rejects_substituted_child_programs() {
    let (tabs, tab_id) = loaded_tabs(
        "<script type=\"application/x-blueice-typescript\">const value: number = 42;</script>",
        "https://example.test/emitted.html",
    );
    for wrong_reply_generation in [false, true] {
        let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
            EmittedProbeChild {
                lifecycle: RecordingChild::default(),
                wrong_reply_generation,
                malformed_map_failure: false,
            },
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
        let original = executor.debugger_emitted_original_position(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            5,
            8,
        );
        let safe_point = executor.debugger_emitted_safe_point(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            5,
            8,
        );
        let paused = executor.debugger_emitted_paused_root_location(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        );
        if wrong_reply_generation {
            assert_eq!(original, Err(JavaScriptPageDebuggerError::NoLiveRealm));
            assert_eq!(safe_point, Err(JavaScriptPageDebuggerError::NoLiveRealm));
            assert_eq!(paused, Err(JavaScriptPageDebuggerError::NoLiveRealm));
        } else {
            assert_eq!(
                original,
                Ok(Some(JavaScriptPageDebuggerEmittedOriginalPosition {
                    source_index: 0,
                    line: 2,
                    column_utf16: 3,
                }))
            );
            assert_eq!(
                safe_point,
                Ok(Some(JavaScriptPageDebuggerSafePoint {
                    code_unit_ordinal: 0,
                    bytecode_offset: 4,
                }))
            );
            assert_eq!(
                paused,
                Ok((
                    JavaScriptPageDebuggerSafePoint {
                        code_unit_ordinal: 0,
                        bytecode_offset: 4,
                    },
                    Some(JavaScriptPageDebuggerEmittedPausedRootLocation {
                        generated_line: 5,
                        generated_column_utf16: 8,
                        original: Some(JavaScriptPageDebuggerEmittedOriginalPosition {
                            source_index: 0,
                            line: 2,
                            column_utf16: 3,
                        }),
                    }),
                ))
            );
        }
    }
}

#[test]
fn core_adapter_hides_private_malformed_map_failures_at_every_emitted_lookup() {
    let (tabs, tab_id) = loaded_tabs(
        "<script type=\"application/x-blueice-typescript\">const value: number = 42;</script>",
        "https://example.test/malformed-emitted.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
        EmittedProbeChild {
            lifecycle: RecordingChild::default(),
            wrong_reply_generation: false,
            malformed_map_failure: true,
        },
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
    assert_eq!(
        executor.debugger_emitted_original_position(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            5,
            8,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    assert_eq!(
        executor.debugger_emitted_safe_point(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            5,
            8,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    assert_eq!(
        executor.debugger_emitted_paused_root_location(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}
