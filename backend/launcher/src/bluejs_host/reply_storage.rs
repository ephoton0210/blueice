// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runtime inspection projections and encoded frames retain native ownership.

use super::*;

#[cfg(test)]
#[path = "../../tests/fixtures/debugger_reply_storage.rs"]
mod tests;
use blueice_bluejs::{VmDebuggerInspection, VmDebuggerOutput, VmDebuggerStackSnapshot};
use std::io::Write;
use std::mem::{size_of, size_of_val};

enum ReplyData {
    Plain(PageHostReply),
    Inspection(VmDebuggerInspection<PageHostReply>),
}
/// A child reply whose runtime inspection storage survives until final use.
/// Serialization allocates a separately admitted fixed buffer and retains both
/// owners through write/flush. Other reply families retain their existing path.
pub struct BlueJsHostReply {
    data: ReplyData,
}
impl From<PageHostReply> for BlueJsHostReply {
    fn from(value: PageHostReply) -> Self {
        Self {
            data: ReplyData::Plain(value),
        }
    }
}
impl AsRef<PageHostReply> for BlueJsHostReply {
    fn as_ref(&self) -> &PageHostReply {
        match &self.data {
            ReplyData::Plain(value) => value,
            ReplyData::Inspection(value) => value.as_ref(),
        }
    }
}
impl std::ops::Deref for BlueJsHostReply {
    type Target = PageHostReply;
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}
impl BlueJsHostReply {
    pub(super) fn projected(
        result: Result<VmDebuggerInspection<PageHostReply>, RuntimeError>,
    ) -> Self {
        match result {
            Ok(value) => Self {
                data: ReplyData::Inspection(value),
            },
            Err(RuntimeError::SchedulingLimit) => resource_limit().into(),
            Err(_) => invalid_debugger_state().into(),
        }
    }
    fn project(
        &self,
        bytes: usize,
        convert: impl FnOnce(
            &PageHostReply,
            &mut VmDebuggerOutput<PageHostReply>,
        ) -> Result<PageHostReply, RuntimeError>,
    ) -> Self {
        match &self.data {
            ReplyData::Inspection(value) => Self::projected(value.try_map(bytes, convert)),
            ReplyData::Plain(_) => invalid_debugger_state().into(),
        }
    }
    /// Write immutable protocol data while its mapped and encoded owners live.
    pub fn write<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        let ReplyData::Inspection(value) = &self.data else {
            return page_host::write_page_host_reply(writer, self.as_ref());
        };
        let length = page_host::page_host_reply_frame_len(value.as_ref())?;
        let encoded = value.try_map(length, |reply, output| {
            let mut bytes = output.vector(length)?;
            bytes.resize(length, 0);
            page_host::encode_page_host_reply_into(reply, &mut bytes).map_err(|_| {
                RuntimeError::Unsupported("native debugger reply serialization failed")
            })?;
            Ok(bytes)
        });
        let frame = match encoded {
            Ok(frame) => frame,
            Err(error) => return write_projection_failure(writer, error),
        };
        writer.write_all(&frame)?;
        writer.flush()
    }
}

// A document admission failure is a normal bounded error reply, not a
// connection failure that could discard healthy neighboring documents.
fn write_projection_failure<W: Write>(writer: &mut W, error: RuntimeError) -> io::Result<()> {
    let reply = match error {
        RuntimeError::SchedulingLimit => resource_limit(),
        _ => host_failure(),
    };
    page_host::write_page_host_reply(writer, &reply)
}

fn slots<T>(count: usize) -> usize {
    count.saturating_mul(size_of::<T>())
}
fn native_scopes(snapshot: &VmDebuggerStackSnapshot) -> usize {
    snapshot
        .frames
        .iter()
        .map(|frame| slots::<PageHostDebuggerScopeEntry>(frame.scope_entries.len()))
        .fold(0, usize::saturating_add)
}
fn wire_scopes(snapshot: &PageHostDebuggerLinkedStackSnapshot) -> usize {
    snapshot
        .frames
        .iter()
        .map(|frame| slots::<PageHostDebuggerScopeEntry>(frame.scope_entries.len()))
        .fold(0, usize::saturating_add)
}
fn system_scopes(snapshot: &PageHostDebuggerSystemStackSnapshot) -> usize {
    snapshot
        .frames
        .iter()
        .map(|frame| slots::<PageHostDebuggerScopeEntry>(frame.scope_entries.len()))
        .fold(0, usize::saturating_add)
}
fn copy_scopes<T>(
    entries: &[VmDebuggerScopeEntry],
    output: &mut VmDebuggerOutput<T>,
) -> Result<Vec<PageHostDebuggerScopeEntry>, RuntimeError> {
    let mut result = output.vector(entries.len())?;
    result.extend(entries.iter().map(|entry| PageHostDebuggerScopeEntry {
        slot_ordinal: entry.slot_ordinal,
        scope_depth: entry.scope_depth,
    }));
    Ok(result)
}

pub(super) fn project_classic_stack(
    source: &VmDebuggerInspection<VmDebuggerStackSnapshot>,
    tab_id: u64,
    document_generation: u64,
    program: PageHostDebuggerProgram,
    frame: Option<PageHostDebuggerFrame>,
) -> BlueJsHostReply {
    let buffers = slots::<PageHostDebuggerStackFrame>(source.frames.len())
        .saturating_add(native_scopes(source));
    BlueJsHostReply::projected(source.try_map(buffers, |snapshot, output| {
        let mut frames = output.vector(snapshot.frames.len())?;
        for frame in &snapshot.frames {
            frames.push(PageHostDebuggerStackFrame {
                code_unit_ordinal: frame.code_unit_ordinal,
                bytecode_offset: frame.bytecode_offset,
                scope_entries: copy_scopes(&frame.scope_entries, output)?,
                scope_truncated: frame.scope_truncated,
            });
        }
        Ok(PageHostReply::DebuggerStackSnapshot {
            tab_id,
            document_generation,
            program,
            frame,
            snapshot: PageHostDebuggerStackSnapshot {
                frames,
                stack_truncated: snapshot.stack_truncated,
            },
        })
    }))
}

pub(super) fn project_linked_stack(
    source: &VmDebuggerInspection<VmDebuggerStackSnapshot>,
    points: [PageHostDebuggerSafePoint; 2],
    max_scope_entries: u32,
) -> Result<VmDebuggerInspection<PageHostDebuggerLinkedStackSnapshot>, PageHostReply> {
    source
        .try_map(native_scopes(source), |snapshot, output| {
            let copy = |index: usize,
                        output: &mut VmDebuggerOutput<PageHostDebuggerLinkedStackSnapshot>|
             -> Result<PageHostDebuggerLinkedStackFrame, RuntimeError> {
                let frame = &snapshot.frames[index];
                Ok(PageHostDebuggerLinkedStackFrame {
                    safe_point: points[index],
                    scope_entries: copy_scopes(&frame.scope_entries, output)?,
                    scope_truncated: frame.scope_truncated,
                })
            };
            Ok(PageHostDebuggerLinkedStackSnapshot {
                frames: [copy(0, output)?, copy(1, output)?],
                stack_truncated: snapshot.stack_truncated,
                max_scope_entries,
            })
        })
        .map_err(|_| resource_limit())
}
fn copy_linked<T>(
    source: &PageHostDebuggerLinkedStackSnapshot,
    output: &mut VmDebuggerOutput<T>,
) -> Result<PageHostDebuggerLinkedStackSnapshot, RuntimeError> {
    let copy = |index: usize,
                output: &mut VmDebuggerOutput<T>|
     -> Result<PageHostDebuggerLinkedStackFrame, RuntimeError> {
        let frame = &source.frames[index];
        Ok(PageHostDebuggerLinkedStackFrame {
            safe_point: frame.safe_point,
            scope_entries: output.copy_slice(&frame.scope_entries)?,
            scope_truncated: frame.scope_truncated,
        })
    };
    Ok(PageHostDebuggerLinkedStackSnapshot {
        frames: [copy(0, output)?, copy(1, output)?],
        stack_truncated: source.stack_truncated,
        max_scope_entries: source.max_scope_entries,
    })
}
pub(super) fn project_linked_reply(
    source: &VmDebuggerInspection<PageHostDebuggerLinkedStackSnapshot>,
    frame: PageHostDebuggerLinkedFrame,
) -> BlueJsHostReply {
    let buffers =
        wire_scopes(source).saturating_add(size_of::<PageHostDebuggerLinkedStackSnapshot>());
    BlueJsHostReply::projected(source.try_map(buffers, |snapshot, output| {
        let copied = copy_linked(snapshot, output)?;
        Ok(PageHostReply::DebuggerLinkedStackSnapshot {
            frame,
            snapshot: output.boxed(copied)?,
        })
    }))
}
pub(super) fn project_linked_spans(
    source: &VmDebuggerInspection<PageHostDebuggerLinkedStackSnapshot>,
    frame: PageHostDebuggerLinkedFrame,
    spans: [PageHostDebuggerBlueTsSafePointSpan; 2],
) -> BlueJsHostReply {
    let buffers = wire_scopes(source)
        .saturating_add(size_of::<PageHostDebuggerLinkedStackSnapshot>())
        .saturating_add(size_of_val(&spans));
    BlueJsHostReply::projected(source.try_map(buffers, |snapshot, output| {
        let copied = copy_linked(snapshot, output)?;
        Ok(PageHostReply::DebuggerLinkedStackSpans {
            frame,
            snapshot: output.boxed(copied)?,
            spans: output.boxed(spans)?,
        })
    }))
}

pub(super) fn project_system_stack(
    source: &VmDebuggerInspection<VmDebuggerStackSnapshot>,
    frame: PageHostDebuggerFrame,
    loader_program: PageHostDebuggerProgram,
    max_scope_entries: u32,
) -> BlueJsHostReply {
    let buffers =
        native_scopes(source).saturating_add(size_of::<PageHostDebuggerSystemStackSnapshot>());
    BlueJsHostReply::projected(source.try_map(buffers, |snapshot, output| {
        let copy = |index: usize,
                    program: PageHostDebuggerProgram,
                    output: &mut VmDebuggerOutput<PageHostReply>|
         -> Result<PageHostDebuggerSystemStackFrame, RuntimeError> {
            let source = &snapshot.frames[index];
            Ok(PageHostDebuggerSystemStackFrame {
                safe_point: PageHostDebuggerSafePoint {
                    program,
                    code_unit_ordinal: source.code_unit_ordinal,
                    bytecode_offset: source.bytecode_offset,
                },
                scope_entries: copy_scopes(&source.scope_entries, output)?,
                scope_truncated: source.scope_truncated,
            })
        };
        let mapped = PageHostDebuggerSystemStackSnapshot {
            frames: [
                copy(0, frame.program, output)?,
                copy(1, loader_program, output)?,
                copy(2, frame.program, output)?,
            ],
            stack_truncated: false,
            max_scope_entries,
        };
        if !mapped.is_well_formed(frame, loader_program) {
            return Err(RuntimeError::Unsupported(
                "invalid native System stack projection",
            ));
        }
        Ok(PageHostReply::DebuggerSystemStackSnapshot {
            frame,
            loader_program,
            snapshot: output.boxed(mapped)?,
        })
    }))
}
fn copy_system<T>(
    snapshot: &PageHostDebuggerSystemStackSnapshot,
    output: &mut VmDebuggerOutput<T>,
) -> Result<PageHostDebuggerSystemStackSnapshot, RuntimeError> {
    let copy = |index: usize,
                output: &mut VmDebuggerOutput<T>|
     -> Result<PageHostDebuggerSystemStackFrame, RuntimeError> {
        let frame = &snapshot.frames[index];
        Ok(PageHostDebuggerSystemStackFrame {
            safe_point: frame.safe_point,
            scope_entries: output.copy_slice(&frame.scope_entries)?,
            scope_truncated: frame.scope_truncated,
        })
    };
    Ok(PageHostDebuggerSystemStackSnapshot {
        frames: [copy(0, output)?, copy(1, output)?, copy(2, output)?],
        stack_truncated: snapshot.stack_truncated,
        max_scope_entries: snapshot.max_scope_entries,
    })
}
pub(super) fn project_system_locations(
    source: &BlueJsHostReply,
    locations: [Option<PageHostDebuggerEmittedPausedRootLocation>; 3],
) -> BlueJsHostReply {
    let PageHostReply::DebuggerSystemStackSnapshot { snapshot, .. } = source.as_ref() else {
        return invalid_debugger_state().into();
    };
    let buffers = system_scopes(snapshot)
        .saturating_add(size_of::<PageHostDebuggerSystemStackSnapshot>())
        .saturating_add(size_of_val(&locations));
    source.project(buffers, |reply, output| {
        let PageHostReply::DebuggerSystemStackSnapshot {
            frame,
            loader_program,
            snapshot,
        } = reply
        else {
            return Err(RuntimeError::Unsupported(
                "System stack owner is unavailable",
            ));
        };
        let copied = copy_system(snapshot, output)?;
        Ok(PageHostReply::DebuggerSystemStackLocations {
            frame: *frame,
            loader_program: *loader_program,
            snapshot: output.boxed(copied)?,
            locations: output.boxed(locations)?,
        })
    })
}

fn preview_buffers(value: &VmDebuggerValuePreview) -> usize {
    match value {
        VmDebuggerValuePreview::BigIntBytes(bytes) => bytes.len(),
        VmDebuggerValuePreview::StringUnits(units) => slots::<u16>(units.len()),
        VmDebuggerValuePreview::Array(items) => {
            slots::<Option<PageHostDebuggerValuePreview>>(items.len()).saturating_add(
                items
                    .iter()
                    .flatten()
                    .map(preview_buffers)
                    .fold(0, usize::saturating_add),
            )
        }
        VmDebuggerValuePreview::Record(entries) => {
            slots::<(Vec<u16>, PageHostDebuggerValuePreview)>(entries.len()).saturating_add(
                entries
                    .iter()
                    .map(|(key, value)| {
                        slots::<u16>(key.len()).saturating_add(preview_buffers(value))
                    })
                    .fold(0, usize::saturating_add),
            )
        }
        _ => 0,
    }
}
fn copy_preview<T>(
    value: &VmDebuggerValuePreview,
    output: &mut VmDebuggerOutput<T>,
) -> Result<PageHostDebuggerValuePreview, RuntimeError> {
    Ok(match value {
        VmDebuggerValuePreview::Undefined => PageHostDebuggerValuePreview::Undefined,
        VmDebuggerValuePreview::Null => PageHostDebuggerValuePreview::Null,
        VmDebuggerValuePreview::Bool(value) => PageHostDebuggerValuePreview::Bool(*value),
        VmDebuggerValuePreview::NumberBits(bits) => PageHostDebuggerValuePreview::NumberBits(*bits),
        VmDebuggerValuePreview::BigIntBytes(bytes) => {
            PageHostDebuggerValuePreview::BigIntBytes(output.copy_slice(bytes)?)
        }
        VmDebuggerValuePreview::StringUnits(units) => {
            PageHostDebuggerValuePreview::StringUnits(output.copy_slice(units)?)
        }
        VmDebuggerValuePreview::Array(items) => {
            let mut result = output.vector(items.len())?;
            for item in items {
                result.push(
                    item.as_ref()
                        .map(|value| copy_preview(value, output))
                        .transpose()?,
                );
            }
            PageHostDebuggerValuePreview::Array(result)
        }
        VmDebuggerValuePreview::Record(entries) => {
            let mut result = output.vector(entries.len())?;
            for (key, value) in entries {
                result.push((
                    output.copy_slice(key.as_code_units())?,
                    copy_preview(value, output)?,
                ));
            }
            PageHostDebuggerValuePreview::Record(result)
        }
    })
}
pub(super) fn project_value_reply(
    source: &VmDebuggerInspection<VmDebuggerValuePreview>,
    target: PageHostDebuggerValueTarget,
) -> BlueJsHostReply {
    let buffers =
        preview_buffers(source).saturating_add(size_of::<PageHostDebuggerValueSnapshot>());
    BlueJsHostReply::projected(source.try_map(buffers, |value, output| {
        let snapshot = PageHostDebuggerValueSnapshot {
            target,
            preview: copy_preview(value, output)?,
        };
        if !snapshot.is_well_formed() {
            return Err(RuntimeError::Unsupported("invalid native value projection"));
        }
        Ok(PageHostReply::DebuggerValueSnapshot(
            output.boxed(snapshot)?,
        ))
    }))
}
pub(super) fn project_linked_value_reply(
    source: &VmDebuggerInspection<VmDebuggerValuePreview>,
    target: PageHostDebuggerLinkedValueTarget,
) -> BlueJsHostReply {
    let retained = size_of::<PageHostDebuggerLinkedStackSnapshot>().saturating_add(
        target
            .expected_stack
            .frames
            .iter()
            .map(|frame| slots::<PageHostDebuggerScopeEntry>(frame.scope_entries.capacity()))
            .fold(0, usize::saturating_add),
    );
    let buffers = retained
        .saturating_add(preview_buffers(source))
        .saturating_add(size_of::<PageHostDebuggerLinkedValueSnapshot>());
    BlueJsHostReply::projected(source.try_map(buffers, |value, output| {
        output.retain_bytes(retained)?;
        let snapshot = PageHostDebuggerLinkedValueSnapshot {
            target,
            preview: copy_preview(value, output)?,
        };
        if !snapshot.is_well_formed() {
            return Err(RuntimeError::Unsupported(
                "invalid native linked value projection",
            ));
        }
        Ok(PageHostReply::DebuggerLinkedValueSnapshot(
            output.boxed(snapshot)?,
        ))
    }))
}
