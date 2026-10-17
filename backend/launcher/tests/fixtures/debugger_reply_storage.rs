// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pure in-memory projections and writers; no host state machine or service.

use super::*;

fn program(id: u64) -> PageHostDebuggerProgram {
    PageHostDebuggerProgram {
        program_handle: id,
        program_generation: id,
    }
}
fn native(
    source: &str,
) -> (
    BlueJsPageRuntime,
    VmDebuggerInspection<VmDebuggerStackSnapshot>,
    VmDebuggerInspection<VmDebuggerValuePreview>,
) {
    let mut runtime = BlueJsPageRuntime::default();
    let origin = BlueJsPageOrigin::new("https://reply.test").unwrap();
    runtime.open_realm(1, origin.clone()).unwrap();
    let handle = runtime
        .install_program(
            1,
            &origin,
            BlueJsSourceIdentity::new("page:///reply.js", "reply").unwrap(),
            &BlueJsProgramV1::Script(parse(source).unwrap()),
        )
        .unwrap();
    let halt = runtime
        .safe_points(1, handle, 64)
        .unwrap()
        .into_iter()
        .rfind(|point| point.code_unit.ordinal() == 0)
        .unwrap();
    runtime
        .execute_program_until_debugger_pause(1, handle, halt)
        .unwrap();
    let stack = runtime
        .debugger_stack_snapshot(1, handle, None, 1, 8)
        .unwrap();
    let frame = &stack.frames[0];
    let value = runtime
        .debugger_value_preview(
            1,
            handle,
            None,
            BlueJsPageDebuggerValueTarget {
                frame_index: 0,
                code_unit_ordinal: 0,
                bytecode_offset: frame.bytecode_offset,
                scope_entry: frame.scope_entries[0],
            },
        )
        .unwrap();
    (runtime, stack, value)
}

fn target(stack: &VmDebuggerStackSnapshot) -> PageHostDebuggerValueTarget {
    let frame = &stack.frames[0];
    PageHostDebuggerValueTarget {
        tab_id: 1,
        document_generation: 1,
        program: program(1),
        frame: None,
        frame_index: 0,
        safe_point: PageHostDebuggerSafePoint {
            program: program(1),
            code_unit_ordinal: 0,
            bytecode_offset: frame.bytecode_offset,
        },
        scope_entry: PageHostDebuggerScopeEntry {
            slot_ordinal: frame.scope_entries[0].slot_ordinal,
            scope_depth: frame.scope_entries[0].scope_depth,
        },
    }
}
fn frames(
    source: &VmDebuggerInspection<VmDebuggerStackSnapshot>,
    count: usize,
) -> VmDebuggerInspection<VmDebuggerStackSnapshot> {
    let entries = &source.frames[0].scope_entries;
    let bytes = slots::<blueice_bluejs::VmDebuggerStackFrame>(count)
        + slots::<VmDebuggerScopeEntry>(count * entries.len());
    source
        .try_map(bytes, |snapshot, output| {
            let mut frames = output.vector(count)?;
            for index in 0..count {
                frames.push(blueice_bluejs::VmDebuggerStackFrame {
                    program_generation: if (count == 2 && index == 0) || (count == 3 && index == 1)
                    {
                        2
                    } else {
                        1
                    },
                    code_unit_ordinal: if index + 1 == count {
                        0
                    } else {
                        index as u32 + 1
                    },
                    bytecode_offset: 0,
                    scope_entries: output.copy_slice(&snapshot.frames[0].scope_entries)?,
                    scope_truncated: false,
                });
            }
            Ok(VmDebuggerStackSnapshot {
                program_generation: 1,
                frames,
                stack_truncated: false,
            })
        })
        .unwrap()
}
fn linked_frame() -> PageHostDebuggerLinkedFrame {
    PageHostDebuggerLinkedFrame {
        tab_id: 1,
        document_generation: 1,
        entry_program: program(1),
        dependency_program: program(2),
        code_unit_ordinal: 1,
        invocation_serial: 1,
    }
}
fn linked(
    source: &VmDebuggerInspection<VmDebuggerStackSnapshot>,
) -> VmDebuggerInspection<PageHostDebuggerLinkedStackSnapshot> {
    project_linked_stack(
        source,
        [
            PageHostDebuggerSafePoint {
                program: program(2),
                code_unit_ordinal: 1,
                bytecode_offset: 0,
            },
            PageHostDebuggerSafePoint {
                program: program(1),
                code_unit_ordinal: 0,
                bytecode_offset: 0,
            },
        ],
        8,
    )
    .unwrap()
}

#[test]
fn reply_storage_classic_copy_has_independent_native_credit() {
    let (mut runtime, stack, _) = native("let data='kept';0;");
    let before = runtime.host_scheduling_usage(1).unwrap().payload_bytes;
    let reply = project_classic_stack(&stack, 1, 1, program(1), None);
    let PageHostReply::DebuggerStackSnapshot { snapshot, .. } = reply.as_ref() else {
        panic!("classic snapshot")
    };
    assert_eq!(snapshot.frames.len(), 1);
    assert_eq!(snapshot.frames[0].scope_entries.len(), 1);
    assert!(runtime.host_scheduling_usage(1).unwrap().payload_bytes > before);
    drop(reply);
    assert_eq!(
        runtime.host_scheduling_usage(1).unwrap().payload_bytes,
        before
    );
}

#[test]
fn reply_storage_value_tree_copy_keeps_its_native_owner() {
    let (mut runtime, stack, value) =
        native("let data={letters:'kept',items:[true,,128n],zero:-0};0;");
    let before = runtime.host_scheduling_usage(1).unwrap().payload_bytes;
    let reply = project_value_reply(&value, target(&stack));
    let PageHostReply::DebuggerValueSnapshot(snapshot) = reply.as_ref() else {
        panic!("value snapshot")
    };
    assert!(snapshot.is_well_formed());
    let PageHostDebuggerValuePreview::Record(entries) = &snapshot.preview else {
        panic!("record")
    };
    assert_eq!(entries.len(), 3);
    assert!(runtime.host_scheduling_usage(1).unwrap().payload_bytes > before);
    drop(reply);
    assert_eq!(
        runtime.host_scheduling_usage(1).unwrap().payload_bytes,
        before
    );
}

#[test]
fn reply_storage_linked_stack_and_spans_keep_their_native_owners() {
    let (mut runtime, root, _) = native("let data='kept';0;");
    let native = frames(&root, 2);
    let snapshot = linked(&native);
    let before = runtime.host_scheduling_usage(1).unwrap().payload_bytes;
    let reply = project_linked_reply(&snapshot, linked_frame());
    assert!(
        matches!(reply.as_ref(),PageHostReply::DebuggerLinkedStackSnapshot{snapshot,..} if snapshot.is_well_formed(linked_frame()))
    );
    let span = PageHostDebuggerBlueTsSafePointSpan {
        source_id: 1,
        start_byte: 0,
        end_byte: 1,
        coordinates: DebuggerSourceCoordinates {
            start_line: 1,
            start_column_utf16: 0,
            end_line: 1,
            end_column_utf16: 1,
        },
    };
    let locations = project_linked_spans(&snapshot, linked_frame(), [span; 2]);
    assert!(
        matches!(locations.as_ref(),PageHostReply::DebuggerLinkedStackSpans{spans,..} if **spans==[span;2])
    );
    assert!(runtime.host_scheduling_usage(1).unwrap().payload_bytes > before);
    drop(reply);
    drop(locations);
    assert_eq!(
        runtime.host_scheduling_usage(1).unwrap().payload_bytes,
        before
    );
}

#[test]
fn reply_storage_system_stack_and_locations_keep_their_native_owners() {
    let (mut runtime, root, _) = native("let data='kept';0;");
    let native = frames(&root, 3);
    let frame = PageHostDebuggerFrame {
        tab_id: 1,
        document_generation: 1,
        program: program(1),
        code_unit_ordinal: 1,
        invocation_serial: 1,
    };
    let before = runtime.host_scheduling_usage(1).unwrap().payload_bytes;
    let reply = project_system_stack(&native, frame, program(2), 8);
    assert!(
        matches!(reply.as_ref(),PageHostReply::DebuggerSystemStackSnapshot{snapshot,..} if snapshot.is_well_formed(frame,program(2)))
    );
    let location = PageHostDebuggerEmittedPausedRootLocation {
        generated_line: 1,
        generated_column_utf16: 0,
        original: None,
    };
    let locations = project_system_locations(&reply, [Some(location), None, Some(location)]);
    assert!(
        matches!(locations.as_ref(),PageHostReply::DebuggerSystemStackLocations{locations,..} if locations[1].is_none())
    );
    assert!(runtime.host_scheduling_usage(1).unwrap().payload_bytes > before);
    drop(reply);
    drop(locations);
    assert_eq!(
        runtime.host_scheduling_usage(1).unwrap().payload_bytes,
        before
    );
}

#[test]
fn reply_storage_linked_value_retains_the_transferred_target_backing() {
    let (mut runtime, root, value) = native("let data='kept';0;");
    let native = frames(&root, 2);
    let snapshot = linked(&native);
    let target = PageHostDebuggerLinkedValueTarget {
        frame: linked_frame(),
        expected_stack: Box::new(snapshot.as_ref().clone()),
        frame_index: 1,
        scope_entry: snapshot.frames[1].scope_entries[0],
    };
    let before = runtime.host_scheduling_usage(1).unwrap().payload_bytes;
    let reply = project_linked_value_reply(&value, target);
    assert!(
        matches!(reply.as_ref(),PageHostReply::DebuggerLinkedValueSnapshot(snapshot) if snapshot.is_well_formed())
    );
    assert!(runtime.host_scheduling_usage(1).unwrap().payload_bytes > before);
    drop(reply);
    assert_eq!(
        runtime.host_scheduling_usage(1).unwrap().payload_bytes,
        before
    );
}

struct Writer<'a> {
    runtime: &'a mut BlueJsPageRuntime,
    bytes: Vec<u8>,
    usage: Vec<usize>,
    flush_usage: Option<usize>,
    refuse: bool,
}
impl Write for Writer<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.usage.push(
            self.runtime
                .host_scheduling_usage(1)
                .map_err(io::Error::other)?
                .payload_bytes,
        );
        if self.refuse {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "fixed sink refusal",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.flush_usage = Some(
            self.runtime
                .host_scheduling_usage(1)
                .map_err(io::Error::other)?
                .payload_bytes,
        );
        Ok(())
    }
}
#[test]
fn reply_storage_encoded_credit_stays_live_through_flush() {
    let (mut runtime, stack, value) = native("let data='kept';0;");
    let reply = project_value_reply(&value, target(&stack));
    let before = runtime.host_scheduling_usage(1).unwrap().payload_bytes;
    let mut writer = Writer {
        runtime: &mut runtime,
        bytes: Vec::new(),
        usage: Vec::new(),
        flush_usage: None,
        refuse: false,
    };
    reply.write(&mut writer).unwrap();
    assert!(writer.usage.iter().all(|usage| *usage > before));
    assert!(writer.flush_usage.unwrap() > before);
    assert_eq!(
        page_host::read_page_host_reply(&mut io::Cursor::new(&writer.bytes)).unwrap(),
        *reply.as_ref()
    );
    assert_eq!(
        writer
            .runtime
            .host_scheduling_usage(1)
            .unwrap()
            .payload_bytes,
        before
    );
}
#[test]
fn reply_storage_write_refusal_releases_only_the_encoded_candidate() {
    let (mut runtime, stack, value) = native("let data='kept';0;");
    let baseline = runtime.host_scheduling_usage(1).unwrap().payload_bytes;
    let reply = project_value_reply(&value, target(&stack));
    let before = runtime.host_scheduling_usage(1).unwrap().payload_bytes;
    let mut writer = Writer {
        runtime: &mut runtime,
        bytes: Vec::new(),
        usage: Vec::new(),
        flush_usage: None,
        refuse: true,
    };
    assert_eq!(
        reply.write(&mut writer).unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert!(writer.usage[0] > before);
    assert!(writer.flush_usage.is_none());
    assert_eq!(
        writer
            .runtime
            .host_scheduling_usage(1)
            .unwrap()
            .payload_bytes,
        before
    );
    drop(writer);
    drop(reply);
    assert_eq!(
        runtime.host_scheduling_usage(1).unwrap().payload_bytes,
        baseline
    );
}
#[test]
fn reply_storage_invalid_value_projection_releases_every_candidate() {
    let (mut runtime, stack, value) = native("let data='kept';0;");
    let mut invalid = target(&stack);
    invalid.program.program_handle = 0;
    let before = runtime.host_scheduling_usage(1).unwrap().payload_bytes;
    let reply = project_value_reply(&value, invalid);
    assert!(matches!(reply.as_ref(), PageHostReply::Error { .. }));
    assert_eq!(
        runtime.host_scheduling_usage(1).unwrap().payload_bytes,
        before
    );
}

#[test]
fn reply_storage_native_refusal_delivers_a_plain_error_without_disconnect() {
    for (error, expected) in [
        (
            RuntimeError::SchedulingLimit,
            PageHostErrorCode::ResourceLimit,
        ),
        (
            RuntimeError::Unsupported("fixed encoder refusal"),
            PageHostErrorCode::HostFailure,
        ),
    ] {
        let mut wire = Vec::new();
        write_projection_failure(&mut wire, error).unwrap();
        assert!(
            matches!(page_host::read_page_host_reply(&mut io::Cursor::new(wire)).unwrap(),PageHostReply::Error{code,..} if code==expected)
        );
    }
}
