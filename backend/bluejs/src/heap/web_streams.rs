// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Stream queues, pending reads and controller relationships are GC-owned.
use super::*;
use std::collections::VecDeque;

#[derive(Clone)]
pub(crate) struct StreamData {
    pub status: u8,
    pub error: Value,
    pub controller: Option<ObjectId>,
    pub reader: Option<ObjectId>,
    pub source: Value,
    pub pull: Value,
    pub cancel: Value,
    pub size: Value,
    pub started: bool,
    pub pulling: bool,
    pub pull_again: bool,
    pub high_water_mark: f64,
    pub close_requested: bool,
    pub queue: VecDeque<(Value, f64)>,
    pub queue_size: f64,
    pub byte_stream: bool,
    pub auto_allocate: Option<usize>,
    pub blob: Option<ObjectId>,
    pub blob_offset: usize,
    pub text_stream: bool,
    pub text_tail: Vec<u8>,
}

#[derive(Clone)]
pub(crate) struct PullInto {
    pub buffer: ObjectId,
    pub buffer_length: usize,
    pub offset: usize,
    pub length: usize,
    pub filled: usize,
    pub minimum: usize,
    pub kind: Option<TypedArrayKind>,
}
#[derive(Clone)]
pub(crate) struct StreamRead {
    pub promise: ObjectId,
    pub into: Option<PullInto>,
}
#[derive(Clone)]
pub(crate) struct StreamReader {
    pub stream: Option<ObjectId>,
    pub closed: ObjectId,
    pub byob: bool,
    pub reads: VecDeque<StreamRead>,
}

#[derive(Clone)]
pub(crate) struct StreamTee {
    pub source: ObjectId,
    pub reader: ObjectId,
    pub branches: [Option<ObjectId>; 2],
    pub cancel_promise: ObjectId,
    pub canceled: [bool; 2],
    pub reasons: [Value; 2],
    pub reading: bool,
    pub read_again: [bool; 2],
    pub finished: bool,
    pub byte_stream: bool,
    pub pending: Value,
    pub pending_branch: Option<u8>,
}
#[derive(Clone)]
pub(crate) enum WebStreamState {
    Iterator {
        reader: ObjectId,
        prevent_cancel: bool,
        finished: bool,
        ongoing: Option<ObjectId>,
    },
    Iteration {
        iterator: ObjectId,
        target: ObjectId,
        returned: bool,
        value: Value,
    },
    Tee(Box<StreamTee>),
    Stream(Box<StreamData>),
    Reader(StreamReader),
    Controller {
        stream: ObjectId,
        byte_stream: bool,
        request: Option<ObjectId>,
    },
    Request {
        stream: Option<ObjectId>,
        view: Option<ObjectId>,
    },
}
impl WebStreamState {
    pub fn references(&self) -> Vec<ObjectId> {
        match self {
            Self::Iterator {
                reader, ongoing, ..
            } => std::iter::once(*reader).chain(*ongoing).collect(),
            Self::Iteration {
                iterator,
                target,
                value,
                ..
            } => [*iterator, *target]
                .into_iter()
                .chain(value.object_id())
                .collect(),
            Self::Tee(state) => std::iter::once(state.source)
                .chain(Some(state.reader))
                .chain(state.branches.into_iter().flatten())
                .chain(Some(state.cancel_promise))
                .chain(
                    state
                        .reasons
                        .iter()
                        .chain(std::iter::once(&state.pending))
                        .filter_map(Value::object_id),
                )
                .collect(),
            Self::Stream(state) => state
                .controller
                .into_iter()
                .chain(state.reader)
                .chain(state.blob)
                .chain(
                    [
                        &state.error,
                        &state.source,
                        &state.pull,
                        &state.cancel,
                        &state.size,
                    ]
                    .into_iter()
                    .filter_map(Value::object_id),
                )
                .chain(
                    state
                        .queue
                        .iter()
                        .filter_map(|(value, _)| value.object_id()),
                )
                .collect(),
            Self::Reader(state) => state
                .stream
                .into_iter()
                .chain(Some(state.closed))
                .chain(state.reads.iter().flat_map(|read| {
                    std::iter::once(read.promise).chain(read.into.as_ref().map(|into| into.buffer))
                }))
                .collect(),
            Self::Controller {
                stream, request, ..
            } => std::iter::once(*stream).chain(*request).collect(),
            Self::Request { stream, view } => stream.iter().chain(view).copied().collect(),
        }
    }
    pub fn bytes(&self) -> usize {
        size_of::<Self>()
            + match self {
                Self::Iteration { value, .. } => value_bytes(value),
                Self::Tee(state) => {
                    size_of::<StreamTee>()
                        + state
                            .reasons
                            .iter()
                            .chain(std::iter::once(&state.pending))
                            .map(value_bytes)
                            .sum::<usize>()
                }
                Self::Stream(state) => {
                    size_of::<StreamData>()
                        + state.text_tail.len()
                        + state
                            .queue
                            .iter()
                            .map(|(v, _)| size_of::<(Value, f64)>() + value_bytes(v))
                            .sum::<usize>()
                        + [
                            &state.error,
                            &state.source,
                            &state.pull,
                            &state.cancel,
                            &state.size,
                        ]
                        .into_iter()
                        .map(value_bytes)
                        .sum::<usize>()
                }
                Self::Reader(state) => state.reads.len() * size_of::<StreamRead>(),
                _ => 0,
            }
    }
}
fn value_bytes(value: &Value) -> usize {
    value.payload_bytes()
}

impl Heap {
    pub(crate) fn alloc_web_stream(
        &mut self,
        state: WebStreamState,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::WebStream(Box::new(state)), Some(prototype))
    }
    pub(crate) fn web_stream(&self, object: ObjectId) -> Result<&WebStreamState, HeapError> {
        match &self.object(object)?.kind {
            ObjectKind::WebStream(state) => Ok(state),
            _ => Err(HeapError::InvalidInternalSlot(object)),
        }
    }
    pub(crate) fn set_web_stream(
        &mut self,
        object: ObjectId,
        state: WebStreamState,
    ) -> Result<(), HeapError> {
        let previous = self.web_stream(object)?.bytes();
        let bytes = state.bytes();
        let references = state.references();
        for reference in &references {
            self.object(*reference)?;
        }
        let protected: Vec<_> = std::iter::once(object)
            .chain(references.iter().copied())
            .collect();
        self.ensure_room(bytes.saturating_sub(previous), &protected)?;
        let entry = self
            .objects
            .get_mut(&object)
            .expect("protected across collection");
        entry.kind = ObjectKind::WebStream(Box::new(state));
        entry.bytes = entry.bytes - previous + bytes;
        self.managed_bytes
            .set(self.managed_bytes.get() - previous + bytes);
        for reference in references {
            self.write_barrier(object, Some(reference));
        }
        Ok(())
    }
}
