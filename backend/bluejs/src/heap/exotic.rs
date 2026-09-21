// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Heap {
    /// Allocates a Proxy exotic object. Trap dispatch stays in the VM so it
    /// can call JavaScript functions while preserving interpreter roots.
    pub(crate) fn alloc_proxy(
        &mut self,
        target: ObjectId,
        handler: ObjectId,
        prototype: Option<ObjectId>,
        callable: bool,
        constructible: bool,
    ) -> Result<ObjectId, HeapError> {
        self.object(target)?;
        self.object(handler)?;
        self.alloc(
            ObjectKind::Proxy {
                target: Some(target),
                handler: Some(handler),
                callable,
                constructible,
            },
            prototype,
        )
    }

    pub(crate) fn proxy(
        &self,
        object: ObjectId,
    ) -> Result<Option<(ObjectId, ObjectId)>, HeapError> {
        Ok(match self.object(object)?.kind {
            ObjectKind::Proxy {
                target: Some(target),
                handler: Some(handler),
                ..
            } => Some((target, handler)),
            ObjectKind::Proxy { .. } => return Err(HeapError::RevokedProxy),
            _ => None,
        })
    }

    pub(crate) fn proxy_capabilities(
        &self,
        object: ObjectId,
    ) -> Result<Option<(bool, bool)>, HeapError> {
        Ok(match self.object(object)?.kind {
            ObjectKind::Proxy {
                callable,
                constructible,
                ..
            } => Some((callable, constructible)),
            _ => None,
        })
    }

    pub(crate) fn revoke_proxy(&mut self, object: ObjectId) -> Result<(), HeapError> {
        let ObjectKind::Proxy {
            target, handler, ..
        } = &mut self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?
            .kind
        else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        *target = None;
        *handler = None;
        Ok(())
    }

    /// Allocates the storage for an arguments object. A non-empty parameter
    /// map makes it an arguments exotic object; an empty map is the ordinary
    /// unmapped variant but retains the same internal-slot representation.
    pub(crate) fn alloc_arguments(
        &mut self,
        parameter_map: HashMap<PropertyName, ObjectId>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::Arguments { parameter_map }, Some(prototype))
    }

    /// A boxed String with read-only, non-configurable virtual indices
    /// and length. The string payload is charged to the managed budget.
    pub fn alloc_string(
        &mut self,
        string: JsString,
        prototype: Option<ObjectId>,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::String(string), prototype)
    }

    pub(crate) fn alloc_native_function(
        &mut self,
        function: NativeFunction,
        name: &str,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::NativeFunction {
                function,
                initial_name: name.into(),
            },
            Some(prototype),
        )
    }

    pub(crate) fn native_function(
        &self,
        object: ObjectId,
    ) -> Result<Option<NativeFunction>, HeapError> {
        Ok(match self.object(object)?.kind {
            ObjectKind::NativeFunction { function, .. } => Some(function),
            _ => None,
        })
    }

    pub(crate) fn function_initial_name(
        &self,
        object: ObjectId,
    ) -> Result<Option<&JsString>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::NativeFunction { initial_name, .. } => Some(initial_name),
            // HostHasSourceTextAvailable is false for compiled functions.
            // Anonymous NativeFunction syntax is valid for every callable.
            _ => None,
        })
    }

    pub(crate) fn boxed_string(&self, object: ObjectId) -> Result<Option<&JsString>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::String(string) => Some(string),
            _ => None,
        })
    }

    pub(crate) fn alloc_closure(
        &mut self,
        code: Rc<Bytecode>,
        captures: Vec<ObjectId>,
        this: Value,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::Closure {
                code,
                captures,
                this,
            },
            Some(prototype),
        )
    }

    pub(crate) fn alloc_generator(
        &mut self,
        state: GeneratorState,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        let state_bytes = state.managed_bytes();
        self.alloc(
            ObjectKind::Generator {
                state: Box::new(state),
                state_bytes,
                async_control: None,
                async_control_bytes: 0,
            },
            Some(prototype),
        )
    }

    pub(crate) fn take_generator_state(
        &mut self,
        object: ObjectId,
    ) -> Result<GeneratorState, HeapError> {
        let entry = self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?;
        let ObjectKind::Generator { state, .. } = &mut entry.kind else {
            return Err(HeapError::InvalidObject(object));
        };
        Ok(*std::mem::replace(state, Box::new(GeneratorState::Done)))
    }

    /// Whether `object` carries generator internal slots (sync or async).
    pub(crate) fn is_generator(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(matches!(
            self.object(object)?.kind,
            ObjectKind::Generator { .. }
        ))
    }

    pub(crate) fn generator_state_is_done(&self, object: ObjectId) -> Result<bool, HeapError> {
        let ObjectKind::Generator { state, .. } = &self.object(object)?.kind else {
            return Err(HeapError::InvalidObject(object));
        };
        Ok(matches!(**state, GeneratorState::Done))
    }

    pub(crate) fn set_generator_state(
        &mut self,
        object: ObjectId,
        state: GeneratorState,
    ) -> Result<(), HeapError> {
        // A generator may have been promoted while it was running. Restoring
        // a suspended frame can then install young bindings/cells into an old
        // generator object, so this internal-slot write needs the same
        // remembered-set barrier as an ordinary property write.
        let references = state.references();
        let state_bytes = state.managed_bytes();
        let old_state_bytes = match &self
            .objects
            .get(&object)
            .ok_or(HeapError::InvalidObject(object))?
            .kind
        {
            ObjectKind::Generator { state_bytes, .. } => *state_bytes,
            _ => return Err(HeapError::InvalidObject(object)),
        };
        // Collection may be needed before a suspended frame is restored.
        // Keep both the generator and its incoming references alive across it.
        let protected: Vec<_> = std::iter::once(object)
            .chain(references.iter().copied())
            .collect();
        self.ensure_room(state_bytes.saturating_sub(old_state_bytes), &protected)?;
        {
            let entry = self
                .objects
                .get_mut(&object)
                .ok_or(HeapError::InvalidObject(object))?;
            let ObjectKind::Generator {
                state: current,
                state_bytes: current_bytes,
                ..
            } = &mut entry.kind
            else {
                return Err(HeapError::InvalidObject(object));
            };
            **current = state;
            *current_bytes = state_bytes;
            entry.bytes = entry.bytes - old_state_bytes + state_bytes;
        }
        self.managed_bytes = self.managed_bytes - old_state_bytes + state_bytes;
        for reference in references {
            self.write_barrier(object, Some(reference));
        }
        Ok(())
    }

    /// Marks a generator object as async before it becomes observable to
    /// JavaScript. The queue starts empty and therefore cannot allocate or
    /// create collector edges.
    pub(crate) fn enable_async_generator(&mut self, object: ObjectId) -> Result<(), HeapError> {
        let entry = self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?;
        let ObjectKind::Generator { async_control, .. } = &mut entry.kind else {
            return Err(HeapError::InvalidObject(object));
        };
        if async_control.is_none() {
            *async_control = Some(AsyncGeneratorControl::default());
        }
        Ok(())
    }

    pub(crate) fn async_generator_control(
        &self,
        object: ObjectId,
    ) -> Result<Option<AsyncGeneratorControl>, HeapError> {
        let ObjectKind::Generator { async_control, .. } = &self.object(object)?.kind else {
            return Err(HeapError::InvalidObject(object));
        };
        Ok(async_control.clone())
    }

    /// Replaces the async-generator queue and performs the same accounting
    /// and old-to-young barriers as a suspended frame restoration.
    pub(crate) fn set_async_generator_control(
        &mut self,
        object: ObjectId,
        control: AsyncGeneratorControl,
    ) -> Result<(), HeapError> {
        let references = control.references();
        let control_bytes = control.managed_bytes();
        let (old_bytes, old_references) = match &self
            .objects
            .get(&object)
            .ok_or(HeapError::InvalidObject(object))?
            .kind
        {
            ObjectKind::Generator {
                async_control,
                async_control_bytes,
                ..
            } => (
                *async_control_bytes,
                async_control
                    .iter()
                    .flat_map(AsyncGeneratorControl::references)
                    .collect::<Vec<_>>(),
            ),
            _ => return Err(HeapError::InvalidObject(object)),
        };
        let protected: Vec<_> = std::iter::once(object)
            .chain(references.iter().copied())
            .chain(old_references)
            .collect();
        self.ensure_room(control_bytes.saturating_sub(old_bytes), &protected)?;
        {
            let entry = self
                .objects
                .get_mut(&object)
                .ok_or(HeapError::InvalidObject(object))?;
            let ObjectKind::Generator {
                async_control,
                async_control_bytes,
                ..
            } = &mut entry.kind
            else {
                return Err(HeapError::InvalidObject(object));
            };
            *async_control = Some(control);
            *async_control_bytes = control_bytes;
            entry.bytes = entry.bytes - old_bytes + control_bytes;
        }
        self.managed_bytes = self.managed_bytes - old_bytes + control_bytes;
        for reference in references {
            self.write_barrier(object, Some(reference));
        }
        Ok(())
    }

    pub(crate) fn alloc_bound_function(
        &mut self,
        bound: BoundFunction,
        prototype: Option<ObjectId>,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::BoundFunction(bound), prototype)
    }

    pub(crate) fn bound_function(
        &self,
        object: ObjectId,
    ) -> Result<Option<&BoundFunction>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::BoundFunction(bound) => Some(bound),
            _ => None,
        })
    }

    pub(crate) fn alloc_collator(
        &mut self,
        data: Rc<crate::intl::Collator>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::Collator {
                data,
                compare: None,
            },
            Some(prototype),
        )
    }
    pub(crate) fn collator(
        &self,
        object: ObjectId,
    ) -> Result<Option<Rc<crate::intl::Collator>>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::Collator { data, .. } => Some(data.clone()),
            _ => None,
        })
    }
    pub(crate) fn collator_compare(&self, object: ObjectId) -> Option<ObjectId> {
        let ObjectKind::Collator { compare, .. } = &self.objects.get(&object).unwrap().kind else {
            unreachable!("VM checks the Collator brand")
        };
        *compare
    }
    pub(crate) fn set_collator_compare(&mut self, object: ObjectId, function: ObjectId) {
        if let ObjectKind::Collator { compare, .. } =
            &mut self.objects.get_mut(&object).unwrap().kind
        {
            *compare = Some(function);
        }
        self.write_barrier(object, Some(function));
    }

    pub(crate) fn alloc_number_format(
        &mut self,
        data: Rc<crate::intl::NumberFormat>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::NumberFormat { data, format: None },
            Some(prototype),
        )
    }

    pub(crate) fn number_format(
        &self,
        object: ObjectId,
    ) -> Result<Option<Rc<crate::intl::NumberFormat>>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::NumberFormat { data, .. } => Some(data.clone()),
            _ => None,
        })
    }

    pub(crate) fn number_format_format(&self, object: ObjectId) -> Option<ObjectId> {
        let ObjectKind::NumberFormat { format, .. } = &self.objects.get(&object).unwrap().kind
        else {
            unreachable!("VM checks the NumberFormat brand")
        };
        *format
    }

    pub(crate) fn set_number_format_format(&mut self, object: ObjectId, function: ObjectId) {
        if let ObjectKind::NumberFormat { format, .. } =
            &mut self.objects.get_mut(&object).unwrap().kind
        {
            *format = Some(function);
        }
        self.write_barrier(object, Some(function));
    }

    pub(crate) fn alloc_date_time_format(
        &mut self,
        data: Rc<crate::intl::DateTimeFormat>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::DateTimeFormat { data, format: None },
            Some(prototype),
        )
    }

    pub(crate) fn date_time_format(
        &self,
        object: ObjectId,
    ) -> Result<Option<Rc<crate::intl::DateTimeFormat>>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::DateTimeFormat { data, .. } => Some(data.clone()),
            _ => None,
        })
    }

    pub(crate) fn date_time_format_format(&self, object: ObjectId) -> Option<ObjectId> {
        let ObjectKind::DateTimeFormat { format, .. } = &self.objects.get(&object).unwrap().kind
        else {
            unreachable!("VM checks the DateTimeFormat brand")
        };
        *format
    }

    pub(crate) fn set_date_time_format_format(&mut self, object: ObjectId, function: ObjectId) {
        if let ObjectKind::DateTimeFormat { format, .. } =
            &mut self.objects.get_mut(&object).unwrap().kind
        {
            *format = Some(function);
        }
        self.write_barrier(object, Some(function));
    }

    pub(crate) fn alloc_display_names(
        &mut self,
        data: Rc<crate::intl::DisplayNames>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::DisplayNames(data), Some(prototype))
    }

    pub(crate) fn display_names(
        &self,
        object: ObjectId,
    ) -> Result<Option<Rc<crate::intl::DisplayNames>>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::DisplayNames(data) => Some(data.clone()),
            _ => None,
        })
    }

    pub(crate) fn alloc_duration_format(
        &mut self,
        data: Rc<crate::intl::DurationFormat>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::DurationFormat(data), Some(prototype))
    }

    pub(crate) fn duration_format(
        &self,
        object: ObjectId,
    ) -> Result<Option<Rc<crate::intl::DurationFormat>>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::DurationFormat(data) => Some(data.clone()),
            _ => None,
        })
    }

    pub(crate) fn alloc_list_format(
        &mut self,
        data: Rc<crate::intl::ListFormat>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::ListFormat(data), Some(prototype))
    }

    pub(crate) fn list_format(
        &self,
        object: ObjectId,
    ) -> Result<Option<Rc<crate::intl::ListFormat>>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::ListFormat(data) => Some(data.clone()),
            _ => None,
        })
    }

    pub(crate) fn alloc_plural_rules(
        &mut self,
        data: Rc<crate::intl::PluralRules>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::PluralRules(data), Some(prototype))
    }

    pub(crate) fn plural_rules(
        &self,
        object: ObjectId,
    ) -> Result<Option<Rc<crate::intl::PluralRules>>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::PluralRules(data) => Some(data.clone()),
            _ => None,
        })
    }

    pub(crate) fn alloc_relative_time_format(
        &mut self,
        data: Rc<crate::intl::RelativeTimeFormat>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::RelativeTimeFormat(data), Some(prototype))
    }

    pub(crate) fn relative_time_format(
        &self,
        object: ObjectId,
    ) -> Result<Option<Rc<crate::intl::RelativeTimeFormat>>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::RelativeTimeFormat(data) => Some(data.clone()),
            _ => None,
        })
    }

    pub(crate) fn alloc_segmenter(
        &mut self,
        data: Rc<crate::intl::Segmenter>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::Segmenter(data), Some(prototype))
    }

    pub(crate) fn segmenter(
        &self,
        object: ObjectId,
    ) -> Result<Option<Rc<crate::intl::Segmenter>>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::Segmenter(data) => Some(data.clone()),
            _ => None,
        })
    }

    pub(crate) fn alloc_segments(
        &mut self,
        data: Rc<crate::intl::Segments>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::Segments(data), Some(prototype))
    }

    pub(crate) fn segments(
        &self,
        object: ObjectId,
    ) -> Result<Option<Rc<crate::intl::Segments>>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::Segments(data) => Some(data.clone()),
            _ => None,
        })
    }

    pub(crate) fn alloc_segment_iterator(
        &mut self,
        data: Rc<crate::intl::Segments>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::SegmentIterator { data, next: 0 },
            Some(prototype),
        )
    }

    pub(crate) fn segment_iterator_next(
        &mut self,
        object: ObjectId,
    ) -> Result<Option<(Rc<crate::intl::Segments>, usize)>, HeapError> {
        let Some(entry) = self.objects.get_mut(&object) else {
            return Err(HeapError::InvalidObject(object));
        };
        let ObjectKind::SegmentIterator { data, next } = &mut entry.kind else {
            return Ok(None);
        };
        let index = *next;
        *next += 1;
        Ok((index < data.records.len()).then(|| (data.clone(), index)))
    }

    pub(crate) fn is_segment_iterator(&self, object: ObjectId) -> Result<bool, HeapError> {
        Ok(matches!(
            self.object(object)?.kind,
            ObjectKind::SegmentIterator { .. }
        ))
    }

    pub(crate) fn alloc_intl_locale(
        &mut self,
        data: Rc<crate::intl::Locale>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::IntlLocale(data), Some(prototype))
    }

    pub(crate) fn intl_locale(
        &self,
        object: ObjectId,
    ) -> Result<Option<Rc<crate::intl::Locale>>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::IntlLocale(data) => Some(data.clone()),
            _ => None,
        })
    }

    pub(crate) fn alloc_regexp(
        &mut self,
        regexp: Rc<crate::regexp::RegExp>,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::RegExp(regexp), Some(prototype))
    }
    pub(crate) fn alloc_boxed_primitive(
        &mut self,
        value: Value,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(ObjectKind::BoxedPrimitive(value), Some(prototype))
    }
    pub(crate) fn boxed_primitive(&self, object: ObjectId) -> Result<Option<Value>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::BoxedPrimitive(value) => Some(value.clone()),
            _ => None,
        })
    }
    pub(crate) fn alloc_array_iterator(
        &mut self,
        object: ObjectId,
        kind: ArrayIteratorKind,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::ArrayIterator {
                object,
                index: 0,
                done: false,
                kind,
            },
            Some(prototype),
        )
    }
    pub(crate) fn array_iterator(
        &self,
        id: ObjectId,
    ) -> Result<Option<(ObjectId, u64, bool, ArrayIteratorKind)>, HeapError> {
        Ok(match self.object(id)?.kind {
            ObjectKind::ArrayIterator {
                object,
                index,
                done,
                kind,
            } => Some((object, index, done, kind)),
            _ => None,
        })
    }
    pub(crate) fn advance_array_iterator(&mut self, id: ObjectId, done: bool) {
        if let ObjectKind::ArrayIterator {
            index,
            done: finished,
            ..
        } = &mut self.objects.get_mut(&id).unwrap().kind
        {
            *index += 1;
            *finished = done;
        }
    }
    pub(crate) fn alloc_iterator_wrapper(
        &mut self,
        iterator: ObjectId,
        next: Value,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::IteratorWrapper { iterator, next },
            Some(prototype),
        )
    }
    pub(crate) fn iterator_wrapper(
        &self,
        id: ObjectId,
    ) -> Result<Option<(ObjectId, Value)>, HeapError> {
        Ok(match &self.object(id)?.kind {
            ObjectKind::IteratorWrapper { iterator, next } => Some((*iterator, next.clone())),
            _ => None,
        })
    }
    pub(crate) fn alloc_iterator_helper(
        &mut self,
        record: ObjectId,
        callback: Value,
        kind: IteratorHelperKind,
        index: u64,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::IteratorHelper {
                record,
                callback,
                index,
                done: false,
                executing: false,
                kind,
            },
            Some(prototype),
        )
    }
    pub(crate) fn iterator_helper(
        &self,
        id: ObjectId,
    ) -> Result<Option<IteratorHelperState>, HeapError> {
        Ok(match &self.object(id)?.kind {
            ObjectKind::IteratorHelper {
                record,
                callback,
                index,
                done,
                executing,
                kind,
            } => Some(IteratorHelperState {
                record: *record,
                callback: callback.clone(),
                index: *index,
                done: *done,
                executing: *executing,
                kind: *kind,
            }),
            _ => None,
        })
    }
    pub(crate) fn begin_iterator_helper(&mut self, id: ObjectId) -> Result<(), HeapError> {
        let object = self
            .objects
            .get_mut(&id)
            .ok_or(HeapError::InvalidObject(id))?;
        let ObjectKind::IteratorHelper { executing, .. } = &mut object.kind else {
            return Err(HeapError::InvalidInternalSlot(id));
        };
        *executing = true;
        Ok(())
    }
    pub(crate) fn leave_iterator_helper(&mut self, id: ObjectId) -> Result<(), HeapError> {
        let object = self
            .objects
            .get_mut(&id)
            .ok_or(HeapError::InvalidObject(id))?;
        let ObjectKind::IteratorHelper { executing, .. } = &mut object.kind else {
            return Err(HeapError::InvalidInternalSlot(id));
        };
        *executing = false;
        Ok(())
    }
    pub(crate) fn finish_iterator_helper(&mut self, id: ObjectId) -> Result<(), HeapError> {
        let object = self
            .objects
            .get_mut(&id)
            .ok_or(HeapError::InvalidObject(id))?;
        let ObjectKind::IteratorHelper {
            done, executing, ..
        } = &mut object.kind
        else {
            return Err(HeapError::InvalidInternalSlot(id));
        };
        *done = true;
        *executing = false;
        Ok(())
    }
    pub(crate) fn advance_iterator_helper(&mut self, id: ObjectId) -> Result<(), HeapError> {
        let object = self
            .objects
            .get_mut(&id)
            .ok_or(HeapError::InvalidObject(id))?;
        let ObjectKind::IteratorHelper { index, .. } = &mut object.kind else {
            return Err(HeapError::InvalidInternalSlot(id));
        };
        *index = index
            .checked_add(1)
            .expect("Iterator helper index is checked before incrementing");
        Ok(())
    }
    pub(crate) fn consume_iterator_helper_take(&mut self, id: ObjectId) -> Result<(), HeapError> {
        let object = self
            .objects
            .get_mut(&id)
            .ok_or(HeapError::InvalidObject(id))?;
        let ObjectKind::IteratorHelper { index, .. } = &mut object.kind else {
            return Err(HeapError::InvalidInternalSlot(id));
        };
        *index = index
            .checked_sub(1)
            .expect("take helper is consumed only with a positive remainder");
        Ok(())
    }
    /// Replaces a RegExp object's [[RegExpMatcher]], [[OriginalSource]] and
    /// [[OriginalFlags]] in place (Annex B `RegExp.prototype.compile`).
    pub(crate) fn set_regexp(
        &mut self,
        object: ObjectId,
        regexp: Rc<crate::regexp::RegExp>,
    ) -> Result<(), HeapError> {
        let ObjectKind::RegExp(current) = &self.object(object)?.kind else {
            return Err(HeapError::InvalidInternalSlot(object));
        };
        let old_bytes = regexp_bytes(current);
        let new_bytes = regexp_bytes(&regexp);
        self.ensure_room(new_bytes.saturating_sub(old_bytes), &[object])?;
        let entry = self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?;
        entry.kind = ObjectKind::RegExp(regexp);
        entry.bytes = entry.bytes - old_bytes + new_bytes;
        self.managed_bytes = self.managed_bytes - old_bytes + new_bytes;
        Ok(())
    }
    pub(crate) fn regexp(
        &self,
        object: ObjectId,
    ) -> Result<Option<Rc<crate::regexp::RegExp>>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::RegExp(regexp) => Some(regexp.clone()),
            _ => None,
        })
    }
    pub(crate) fn alloc_regexp_iterator(
        &mut self,
        matcher: ObjectId,
        string: JsString,
        global: bool,
        unicode: bool,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::RegExpIterator {
                matcher,
                string,
                global,
                unicode,
                done: false,
            },
            Some(prototype),
        )
    }
    pub(crate) fn regexp_iterator(
        &self,
        object: ObjectId,
    ) -> Result<Option<RegExpIteratorState>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::RegExpIterator {
                matcher,
                string,
                global,
                unicode,
                done,
            } => Some((*matcher, string.clone(), *global, *unicode, *done)),
            _ => None,
        })
    }
    pub(crate) fn finish_regexp_iterator(&mut self, object: ObjectId) {
        if let ObjectKind::RegExpIterator { done, .. } =
            &mut self.objects.get_mut(&object).unwrap().kind
        {
            *done = true;
        }
    }

    pub(crate) fn closure(&self, object: ObjectId) -> Result<Option<ClosureState>, HeapError> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::Closure {
                code,
                captures,
                this,
            } => {
                let metadata = self.closure_metadata.get(&object);
                Some((
                    code.clone(),
                    captures.clone(),
                    this.clone(),
                    metadata.and_then(|metadata| metadata.home),
                    metadata.and_then(|metadata| metadata.class_base.clone()),
                ))
            }
            _ => None,
        })
    }

    pub(crate) fn set_closure_home(
        &mut self,
        object: ObjectId,
        home: ObjectId,
    ) -> Result<(), HeapError> {
        self.object(home)?;
        if !matches!(self.object(object)?.kind, ObjectKind::Closure { .. }) {
            return Err(HeapError::InvalidObject(object));
        }
        self.ensure_closure_metadata(object, &[home])?;
        self.write_barrier(object, Some(home));
        self.closure_metadata
            .get_mut(&object)
            .expect("metadata was installed")
            .home = Some(home);
        Ok(())
    }

    pub(crate) fn set_class_base(
        &mut self,
        object: ObjectId,
        base: Value,
    ) -> Result<(), HeapError> {
        if !matches!(self.object(object)?.kind, ObjectKind::Closure { .. }) {
            return Err(HeapError::InvalidObject(object));
        }
        if let Some(target) = base.object_id() {
            self.ensure_closure_metadata(object, &[target])?;
        } else {
            self.ensure_closure_metadata(object, &[])?;
        }
        self.write_barrier(object, base.object_id());
        self.closure_metadata
            .get_mut(&object)
            .expect("metadata was installed")
            .class_base = Some(base);
        Ok(())
    }

    pub(crate) fn class_base(&self, object: ObjectId) -> Result<Option<Value>, HeapError> {
        match &self.object(object)?.kind {
            ObjectKind::Closure { .. } => Ok(self
                .closure_metadata
                .get(&object)
                .and_then(|metadata| metadata.class_base.clone())),
            _ => Err(HeapError::InvalidObject(object)),
        }
    }

    fn ensure_closure_metadata(
        &mut self,
        object: ObjectId,
        protected: &[ObjectId],
    ) -> Result<(), HeapError> {
        if self.closure_metadata.contains_key(&object) {
            return Ok(());
        }
        let protected: Vec<_> = std::iter::once(object)
            .chain(protected.iter().copied())
            .collect();
        self.ensure_room(CLOSURE_METADATA_BYTES, &protected)?;
        self.closure_metadata
            .insert(object, ClosureMetadata::default());
        self.managed_bytes += CLOSURE_METADATA_BYTES;
        Ok(())
    }

    pub(crate) fn alloc_string_iterator(
        &mut self,
        string: JsString,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::StringIterator {
                string,
                position: 0,
            },
            Some(prototype),
        )
    }

    pub(crate) fn string_iterator_next(
        &mut self,
        object: ObjectId,
    ) -> Result<Option<Option<JsString>>, HeapError> {
        let obj = self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?;
        let ObjectKind::StringIterator { string, position } = &mut obj.kind else {
            return Ok(None);
        };
        if *position == string.len() {
            return Ok(Some(None));
        }
        let start = *position;
        let units = string.as_code_units();
        *position += if (0xd800..=0xdbff).contains(&units[start])
            && units
                .get(start + 1)
                .is_some_and(|c| (0xdc00..=0xdfff).contains(c))
        {
            2
        } else {
            1
        };
        Ok(Some(Some(JsString::from_code_units(
            units[start..*position].to_vec(),
        ))))
    }

    pub fn get_own_property_descriptor(
        &self,
        object: ObjectId,
        key: impl Into<PropertyName>,
    ) -> Result<Option<PropertyDescriptor>, HeapError> {
        self.get_own_property_descriptor_key(object, key.into())
    }
}

/// Managed bytes a RegExp object's matcher data accounts for.
pub(super) fn regexp_bytes(regexp: &crate::regexp::RegExp) -> usize {
    regexp.source.byte_len()
        + regexp.flags.len()
        + regexp
            .capture_names
            .iter()
            .map(|(name, _)| name.len() + size_of::<(String, usize)>())
            .sum::<usize>()
}
