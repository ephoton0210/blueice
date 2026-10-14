// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reader-owned asynchronous iteration with serialized next/return requests.
//! References: Streams §4.2.5 and Web IDL §3.7.10.
use super::*;

impl Vm {
    pub(super) fn install_stream_iteration(&mut self) -> Result<(), RuntimeError> {
        let parent = self.async_iterator_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(parent)))?;
        let base = self.stack.len();
        self.stack.push(Value::Object(prototype));
        let result = (|| {
            let function = self.function_prototype()?;
            for (name, length, kind) in [("next", 0, IteratorNext), ("return", 1, IteratorReturn)] {
                self.install_native(
                    prototype,
                    function,
                    name,
                    length,
                    NativeFunction::WebStream(kind),
                )?;
                self.with_roots(|heap| {
                    heap.define_own_property(
                        prototype,
                        name,
                        PropertyDescriptor {
                            enumerable: Some(true),
                            ..Default::default()
                        },
                    )
                })?;
            }
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("ReadableStream AsyncIterator".into()),
                false,
                false,
                true,
            )?;
            self.heap.root(prototype)?;
            self.globals
                .insert("%ReadableStreamIterator%".into(), prototype);
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }
    pub(super) fn stream_values(
        &mut self,
        stream: ObjectId,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        self.stream_data(stream)?;
        if !matches!(options, Value::Undefined | Value::Null | Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Stream iteration options must be an object".into(),
            ));
        }
        let base = self.stack.len();
        self.stack.extend([Value::Object(stream), options.clone()]);
        let result = (|| {
            let prevent = self.stream_option(options, "preventCancel")?;
            let prevent_cancel = self.to_boolean(&prevent)?;
            let reader = self.acquire_stream_reader(stream, false)?;
            self.stack.push(Value::Object(reader));
            let prototype = self.globals["%ReadableStreamIterator%"];
            self.with_roots(|heap| {
                heap.alloc_web_stream(
                    WebStreamState::Iterator {
                        reader,
                        prevent_cancel,
                        finished: false,
                        ongoing: None,
                    },
                    prototype,
                )
            })
            .map(Value::Object)
        })();
        self.stack.truncate(base);
        result
    }
    pub(super) fn stream_iteration_call(
        &mut self,
        receiver: &Value,
        returned: bool,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        let value = if returned { value } else { Value::Undefined };
        let Some(iterator) = receiver.object_id().filter(|id| {
            matches!(
                self.heap.web_stream(*id),
                Ok(WebStreamState::Iterator { .. })
            )
        }) else {
            return self.stream_rejected("Invalid stream iterator receiver");
        };
        let base = self.stack.len();
        self.stack.extend([receiver.clone(), value.clone()]);
        let result = (|| {
            let target = self.new_promise()?;
            self.stack.push(Value::Object(target));
            let prototype = self.object_prototype;
            let request = self.with_roots(|heap| {
                heap.alloc_web_stream(
                    WebStreamState::Iteration {
                        iterator,
                        target,
                        returned,
                        value,
                    },
                    prototype,
                )
            })?;
            self.stack.push(Value::Object(request));
            let mut state = self.heap.web_stream(iterator)?.clone();
            let WebStreamState::Iterator { ongoing, .. } = &mut state else {
                unreachable!()
            };
            let previous = ongoing.replace(target);
            self.with_roots(|heap| heap.set_web_stream(iterator, state))?;
            if let Some(previous) = previous {
                let function = self.function_prototype()?;
                let start = self.with_roots(|heap| {
                    heap.alloc_native_function(
                        NativeFunction::StreamIteration {
                            request,
                            action: 0,
                            rejected: false,
                        },
                        "",
                        function,
                    )
                })?;
                self.stack.push(Value::Object(start));
                self.perform_promise_then(
                    previous,
                    Value::Object(start),
                    Value::Object(start),
                    ReactionTarget::Native(target),
                )?;
            } else {
                self.start_stream_iteration(request, Some(target))?;
            }
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        result
    }
    fn iteration_request(
        &self,
        request: ObjectId,
    ) -> Result<(ObjectId, ObjectId, bool, Value), RuntimeError> {
        match self.heap.web_stream(request)? {
            WebStreamState::Iteration {
                iterator,
                target,
                returned,
                value,
            } => Ok((*iterator, *target, *returned, value.clone())),
            _ => Err(RuntimeError::TypeError(
                "Invalid internal stream iteration".into(),
            )),
        }
    }
    fn start_stream_iteration(
        &mut self,
        request: ObjectId,
        target: Option<ObjectId>,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(Value::Object(request));
        let result = (|| {
            let (iterator, outward, returned, value) = self.iteration_request(request)?;
            self.stack.push(value.clone());
            let WebStreamState::Iterator {
                reader,
                prevent_cancel,
                finished,
                ..
            } = self.heap.web_stream(iterator)?.clone()
            else {
                unreachable!()
            };
            let target = match target {
                Some(target) => target,
                None => self.new_promise()?,
            };
            self.stack.push(Value::Object(target));
            if finished {
                let mut state = self.heap.web_stream(iterator)?.clone();
                let WebStreamState::Iterator { ongoing, .. } = &mut state else {
                    unreachable!()
                };
                if *ongoing == Some(outward) {
                    *ongoing = None;
                }
                self.with_roots(|heap| heap.set_web_stream(iterator, state))?;
                let value =
                    self.iterator_result(if returned { value } else { Value::Undefined }, true)?;
                self.stack.push(value.clone());
                self.resolve_promise(target, value)?;
                return Ok(Value::Object(target));
            }
            let (input, action) = if returned {
                let mut state = self.heap.web_stream(iterator)?.clone();
                let WebStreamState::Iterator { finished, .. } = &mut state else {
                    unreachable!()
                };
                *finished = true;
                self.with_roots(|heap| heap.set_web_stream(iterator, state))?;
                let input = if prevent_cancel {
                    self.promise_resolve(Value::Undefined)?
                } else {
                    let source = self
                        .stream_reader(reader)?
                        .stream
                        .expect("live iterator reader");
                    self.cancel_stream(source, value)?
                };
                self.stack.push(input.clone());
                self.release_stream_reader(reader)?;
                (input, 2)
            } else {
                (self.read_stream(reader, &[])?, 1)
            };
            self.stack.push(input.clone());
            let input = input.object_id().expect("stream operation returns Promise");
            let function = self.function_prototype()?;
            let mut callbacks = Vec::new();
            for rejected in [false, true] {
                let callback = self.with_roots(|heap| {
                    heap.alloc_native_function(
                        NativeFunction::StreamIteration {
                            request,
                            action,
                            rejected,
                        },
                        "",
                        function,
                    )
                })?;
                self.stack.push(Value::Object(callback));
                callbacks.push(Value::Object(callback));
            }
            self.perform_promise_then(
                input,
                callbacks[0].clone(),
                callbacks[1].clone(),
                ReactionTarget::Native(target),
            )?;
            Ok(Value::Object(target))
        })();
        self.stack.truncate(base);
        result
    }
    pub(in super::super) fn stream_iteration_reaction(
        &mut self,
        request: ObjectId,
        action: u8,
        rejected: bool,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        if action == 0 {
            return self.start_stream_iteration(request, None);
        }
        let base = self.stack.len();
        self.stack.extend([Value::Object(request), value.clone()]);
        let result = (|| {
            let (iterator, target, _, returned_value) = self.iteration_request(request)?;
            let mut state = self.heap.web_stream(iterator)?.clone();
            let WebStreamState::Iterator {
                reader,
                finished,
                ongoing,
                ..
            } = &mut state
            else {
                unreachable!()
            };
            if *ongoing == Some(target) {
                *ongoing = None;
            }
            let done = if action == 2 || rejected {
                true
            } else {
                let result = value.object_id().expect("owned stream read result");
                self.heap.get_own(result, "done")? == Some(Value::Bool(true))
            };
            if done {
                *finished = true;
            }
            let reader = *reader;
            self.with_roots(|heap| heap.set_web_stream(iterator, state))?;
            if action == 1 && done {
                self.release_stream_reader(reader)?;
            }
            if rejected {
                return Err(RuntimeError::Thrown(value));
            }
            if action == 2 {
                self.iterator_result(returned_value, true)
            } else {
                Ok(value)
            }
        })();
        self.stack.truncate(base);
        result
    }
}
