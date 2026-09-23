// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The reverse Test262 membrane: a *child* realm's own code calling back
//! into a live object owned by the Test262 *parent* realm that created it
//! (`$262.createRealm()`), the mirror image of `foreign.rs`'s forward
//! direction (parent code reaching into a child's real objects).
//!
//! `foreign.rs`'s `test262_transport_value` allocates a facade in the child
//! heap for an opaque parent value and registers it in
//! `Vm::test262_reverse_values` (see that struct's own documentation, and
//! `Test262ReverseValue`, both in `vm.rs`). This module is what makes that
//! facade *live*: every function here mirrors one of `foreign.rs`'s own
//! forward-direction functions, but resolves its target realm dynamically,
//! through [`realm_reentrancy::resolve_active`], rather than through a
//! `test262_realms` map entry -- a child realm has no map back to its own
//! parent, so the parent is only reachable while a call chain that started
//! there is still mid-flight on this thread. Do not reinvent this
//! mechanism: it is the exact one `shadow_realm.rs` already established for
//! an analogous problem (`ShadowRealm`'s `WrappedFunctionCreate` facades),
//! extracted into `realm_reentrancy` so both can share it.
//!
//! # Soundness: `Box<Vm>`, not `Rc<RefCell<Vm>>`
//!
//! `ShadowRealmRecord` shares its child via `Rc<RefCell<Vm>>`, so a
//! reentrant call back into an already-borrowed child is caught at runtime
//! (`RefCell::try_borrow_mut` returning `Err`, handled by falling back to
//! `resolve_active`). `Test262Realm` instead owns its child with a plain
//! `Box<Vm>` -- there is no equivalent runtime check here. Every function
//! below therefore follows a stricter discipline than the `RefCell` case
//! needs: read everything required from `self` (the child) *before*
//! resolving and dereferencing the parent pointer, then never touch `self`
//! again for the rest of the function. As long as that holds, the parent
//! pointer this module dereferences is used in a call that is always
//! strictly *nested* inside -- never overlapping -- any use of the `self`
//! reference these functions were called with, which is the same
//! non-overlapping-borrow discipline `register_active`/`resolve_active`'s
//! own documentation describes. The complementary half of this discipline,
//! on the forward side (never holding a `self.test262_realms`-derived safe
//! reference alive across a call that might reenter through this module),
//! lives in `foreign.rs`'s `test262_foreign_call` -- see its own comments.

use super::super::realm_reentrancy::{register_active, resolve_active};
use super::*;

impl Vm {
    /// The reverse-membrane counterpart of [`Vm::test262_foreign_reference`]
    /// (`foreign.rs`): looks up a local facade, living in *this* realm,
    /// that stands in for a value owned by a Test262 *parent* realm.
    /// Returns `(home_heap, target, callable, constructible)`, mirroring
    /// the forward direction's `(realm, target, callable, constructible)`
    /// shape except that the first element identifies the parent
    /// dynamically (a heap tag, resolved through
    /// [`realm_reentrancy::resolve_active`]) rather than by a
    /// `test262_realms` key.
    pub(in super::super) fn test262_reverse_reference(
        &self,
        wrapper: ObjectId,
    ) -> Option<(u64, ObjectId, bool, bool)> {
        self.test262_reverse_values.get(&wrapper).map(|value| {
            (
                value.home_heap,
                value.target,
                value.callable,
                value.constructible,
            )
        })
    }

    /// Resolves a reverse facade's `home_heap` to the live parent `Vm`
    /// currently mid-call on this thread's cross-realm call chain --
    /// exactly the two checks `shadow_call_wrapped` established a
    /// precedent for: the creating frame having already returned
    /// (`resolve_active` finds nothing, a catchable error rather than a
    /// dangling dereference), and a value somehow looping all the way back
    /// to its own origin realm within one call chain (which would alias
    /// `self` with itself).
    fn resolve_reverse_parent(&self, home_heap: u64) -> Result<*mut Vm, RuntimeError> {
        let Some(ptr) = resolve_active(home_heap) else {
            return Err(RuntimeError::TypeError(
                "the Test262 realm this value belongs to is no longer reachable".into(),
            ));
        };
        if std::ptr::eq(ptr as *const Vm, self as *const Vm) {
            return Err(RuntimeError::TypeError(
                "a Test262 reverse membrane facade cannot resolve to its own realm".into(),
            ));
        }
        Ok(ptr)
    }

    /// Writes `bytes` directly into the *real* parent-owned buffer that
    /// `local_result` (a value in *this*, the child, realm) is a
    /// same-realm-of-`constructor_wrapper` stand-in for, bypassing
    /// `test262_import_foreign_value`'s round-trip identity cache
    /// entirely rather than working around it.
    ///
    /// Why this is needed: when a reverse-facade constructor (a parent
    /// constructor observed from inside a child, e.g.
    /// `TypedArraySpeciesCreate` reaching a parent's own `%TypedArray%`
    /// subclass) is `Construct`ed via [`Vm::test262_reverse_call`], the
    /// result is a genuine object in the *parent's* heap. `test262_
    /// reverse_call` exports that result back into this realm via the
    /// ordinary forward-direction `test262_export_foreign_value`, which
    /// -- for a TypedArray/Array/ArrayBuffer result -- takes `test262_
    /// transport_value`'s eager snapshot path (`foreign.rs`) rather than a
    /// live facade: `local_result` is therefore a fresh, independent local
    /// object, registered as a round-trip stand-in for the real parent
    /// object in the parent's own `imported_values`/`imported_sources`
    /// maps (`Test262Realm`, `vm.rs`). Mutating `local_result` after the
    /// fact -- by any means, bitwise or ordinary `[[Set]]` -- has no
    /// observable effect: once this value crosses back out to whichever
    /// realm dispatched into this one, `test262_import_foreign_value`'s
    /// round-trip cache checks `imported_values` first and unconditionally
    /// returns the *original, pristine* parent object instead of
    /// `local_result`, discarding any mutation (see
    /// `TEST262_ANALYSIS_REPORT.md`'s "Reverse-membrane round-trip cache"
    /// writeup for the full trace). Writing directly into the real
    /// object's own buffer -- reached here via the exact same
    /// `resolve_active`/raw-pointer discipline every other function in
    /// this module uses -- sidesteps that cache instead of fighting it:
    /// there is nothing left to discard.
    ///
    /// `constructor_wrapper` must be the reverse facade that was
    /// `Construct`ed to produce `local_result` (so this can resolve the
    /// exact parent realm and child-realm identity the round trip used);
    /// any other value there will not find `local_result` registered and
    /// returns `Ok(false)` (not an error -- an ordinary "nothing to do"
    /// outcome for a caller that only takes this path defensively).
    pub(in super::super) fn test262_reverse_write_into_real_construction_result(
        &self,
        constructor_wrapper: ObjectId,
        local_result: ObjectId,
        byte_offset: usize,
        bytes: &[u8],
    ) -> Result<bool, RuntimeError> {
        let Some((home_heap, ..)) = self.test262_reverse_reference(constructor_wrapper) else {
            return Ok(false);
        };
        let self_heap = self.object_prototype.heap;
        let ptr = self.resolve_reverse_parent(home_heap)?;
        // SAFETY: see this module's top-level "Soundness" note. This
        // function makes no nested calls into JavaScript (only direct heap
        // reads/writes), so there is no reentrancy concern beyond the
        // dereference itself, which is sound for the same reason every
        // other function in this module's dereference is: `ptr` is only
        // present in `ACTIVE` for the dynamic extent of a `&mut Vm` call
        // currently suspended further up the Rust call stack, and we have
        // already confirmed (via `resolve_reverse_parent`) it is not
        // `self`.
        let parent = unsafe { &mut *ptr };
        let child_realm_id = reverse_child_realm_id(parent, self_heap);
        let Some(real_target) = parent
            .test262_realms
            .get(&child_realm_id)
            .expect("a reverse facade's home realm always owns this child directly")
            .imported_values
            .get(&local_result)
            .and_then(|imported| imported.value.object_id())
        else {
            return Ok(false);
        };
        let (real_buffer, real_offset, ..) = parent.heap.typed_array_info(real_target)?;
        parent.with_roots(|heap| {
            heap.array_buffer_write(real_buffer, real_offset + byte_offset, bytes)
        })?;
        Ok(true)
    }

    /// `[[Get]]` on a reverse facade: forwards into the parent realm,
    /// mirroring `test262_foreign_get`.
    pub(in super::super) fn test262_reverse_get(
        &mut self,
        wrapper: ObjectId,
        receiver: &Value,
        key: &PropertyName,
    ) -> Result<Value, RuntimeError> {
        let (home_heap, target, _, _) = self
            .test262_reverse_reference(wrapper)
            .expect("reverse get has a membrane record");
        let self_heap = self.object_prototype.heap;
        let ptr = self.resolve_reverse_parent(home_heap)?;
        let _guard = register_active(self);
        // SAFETY: see `realm_reentrancy::ACTIVE`'s documentation and this
        // module's own top-level "Soundness" note. `ptr` was just confirmed
        // distinct from `self`, and `self` is not dereferenced again for
        // the rest of this function.
        let parent = unsafe { &mut *ptr };
        let child_realm_id = reverse_child_realm_id(parent, self_heap);
        let receiver = parent.test262_import_foreign_value(child_realm_id, receiver.clone())?;
        parent.remaining_instructions = parent.config.instruction_budget;
        let result = parent.get_object_property(target, &receiver, key);
        test262_reverse_export_result(parent, child_realm_id, result)
    }

    /// `[[GetPrototypeOf]]` on a reverse facade, mirroring
    /// `test262_foreign_get_prototype`.
    pub(in super::super) fn test262_reverse_get_prototype(
        &mut self,
        wrapper: ObjectId,
    ) -> Result<Option<ObjectId>, RuntimeError> {
        let (home_heap, target, override_prototype) = {
            let value = self
                .test262_reverse_values
                .get(&wrapper)
                .expect("reverse prototype has a membrane record");
            (value.home_heap, value.target, value.prototype_override)
        };
        if override_prototype.is_some() {
            return Ok(override_prototype);
        }
        let self_heap = self.object_prototype.heap;
        let ptr = self.resolve_reverse_parent(home_heap)?;
        let _guard = register_active(self);
        // SAFETY: see this module's top-level "Soundness" note.
        let parent = unsafe { &mut *ptr };
        let child_realm_id = reverse_child_realm_id(parent, self_heap);
        parent.remaining_instructions = parent.config.instruction_budget;
        let prototype = parent.object_get_prototype(target)?;
        prototype
            .map(|prototype| {
                parent.test262_export_foreign_value(child_realm_id, &Value::Object(prototype))
            })
            .transpose()
            .map(|prototype| prototype.and_then(|prototype| prototype.object_id()))
    }

    /// `[[SetPrototypeOf]]` on a reverse facade, mirroring
    /// `test262_foreign_set_prototype`.
    pub(in super::super) fn test262_reverse_set_prototype(
        &mut self,
        wrapper: ObjectId,
        prototype: Option<ObjectId>,
    ) -> Result<bool, RuntimeError> {
        let (home_heap, target, _, _) = self
            .test262_reverse_reference(wrapper)
            .expect("reverse set-prototype has a membrane record");
        let self_heap = self.object_prototype.heap;
        let ptr = self.resolve_reverse_parent(home_heap)?;
        let _guard = register_active(self);
        // SAFETY: see this module's top-level "Soundness" note.
        let parent = unsafe { &mut *ptr };
        let child_realm_id = reverse_child_realm_id(parent, self_heap);
        let prototype = prototype
            .map(|prototype| {
                parent.test262_import_foreign_value(child_realm_id, Value::Object(prototype))
            })
            .transpose()?
            .map(|prototype| {
                prototype
                    .object_id()
                    .expect("an imported [[Prototype]] value is always an object")
            });
        parent.remaining_instructions = parent.config.instruction_budget;
        parent.object_set_prototype(target, prototype)
    }

    /// `[[OwnPropertyKeys]]` on a reverse facade, mirroring
    /// `test262_foreign_own_property_keys`. Keys are primitives (strings or
    /// agent-wide registered Symbols), so no membrane transport is needed
    /// for the results themselves.
    pub(in super::super) fn test262_reverse_own_property_keys(
        &mut self,
        wrapper: ObjectId,
    ) -> Result<Vec<PropertyName>, RuntimeError> {
        let (home_heap, target, _, _) = self
            .test262_reverse_reference(wrapper)
            .expect("reverse ownKeys has a membrane record");
        let ptr = self.resolve_reverse_parent(home_heap)?;
        let _guard = register_active(self);
        // SAFETY: see this module's top-level "Soundness" note.
        let parent = unsafe { &mut *ptr };
        parent.remaining_instructions = parent.config.instruction_budget;
        parent.object_own_property_keys(target)
    }

    /// `[[GetOwnProperty]]` on a reverse facade, mirroring
    /// `test262_foreign_get_own_property`.
    pub(in super::super) fn test262_reverse_get_own_property(
        &mut self,
        wrapper: ObjectId,
        key: &PropertyName,
    ) -> Result<Option<PropertyDescriptor>, RuntimeError> {
        let (home_heap, target, _, _) = self
            .test262_reverse_reference(wrapper)
            .expect("reverse own-property has a membrane record");
        let self_heap = self.object_prototype.heap;
        let ptr = self.resolve_reverse_parent(home_heap)?;
        let _guard = register_active(self);
        // SAFETY: see this module's top-level "Soundness" note.
        let parent = unsafe { &mut *ptr };
        let child_realm_id = reverse_child_realm_id(parent, self_heap);
        parent.remaining_instructions = parent.config.instruction_budget;
        let descriptor = parent.object_get_own_property(target, key)?;
        let Some(descriptor) = descriptor else {
            return Ok(None);
        };
        Ok(Some(PropertyDescriptor {
            value: descriptor
                .value
                .map(|value| parent.test262_export_foreign_value(child_realm_id, &value))
                .transpose()?,
            writable: descriptor.writable,
            get: descriptor
                .get
                .map(|value| parent.test262_export_foreign_value(child_realm_id, &value))
                .transpose()?,
            set: descriptor
                .set
                .map(|value| parent.test262_export_foreign_value(child_realm_id, &value))
                .transpose()?,
            enumerable: descriptor.enumerable,
            configurable: descriptor.configurable,
        }))
    }

    /// `[[Set]]` on a reverse facade with an explicit receiver, mirroring
    /// `test262_foreign_set_with_receiver`.
    pub(in super::super) fn test262_reverse_set_with_receiver(
        &mut self,
        wrapper: ObjectId,
        receiver: &Value,
        key: &PropertyName,
        value: &Value,
    ) -> Result<bool, RuntimeError> {
        let (home_heap, target, _, _) = self
            .test262_reverse_reference(wrapper)
            .expect("reverse set has a membrane record");
        let self_heap = self.object_prototype.heap;
        let ptr = self.resolve_reverse_parent(home_heap)?;
        let _guard = register_active(self);
        // SAFETY: see this module's top-level "Soundness" note.
        let parent = unsafe { &mut *ptr };
        let child_realm_id = reverse_child_realm_id(parent, self_heap);
        let receiver = parent.test262_import_foreign_value(child_realm_id, receiver.clone())?;
        let value = parent.test262_import_foreign_value(child_realm_id, value.clone())?;
        parent.remaining_instructions = parent.config.instruction_budget;
        parent.ordinary_set_with_receiver(target, &receiver, key, &value)
    }

    /// `[[Set]]` on a reverse facade with an implicit receiver (the facade
    /// itself), mirroring `test262_foreign_set`. Unlike its forward
    /// counterpart, this does not special-case a TypedArray buffer mirror
    /// or an equivalent-native-function short circuit: neither concern
    /// applies to the opaque, property-forwarding case the reverse
    /// membrane represents (see `test262_transport_value`).
    pub(in super::super) fn test262_reverse_set(
        &mut self,
        wrapper: ObjectId,
        key: &PropertyName,
        value: &Value,
    ) -> Result<(), RuntimeError> {
        let (home_heap, target, _, _) = self
            .test262_reverse_reference(wrapper)
            .expect("reverse set has a membrane record");
        let self_heap = self.object_prototype.heap;
        let ptr = self.resolve_reverse_parent(home_heap)?;
        let _guard = register_active(self);
        // SAFETY: see this module's top-level "Soundness" note.
        let parent = unsafe { &mut *ptr };
        let child_realm_id = reverse_child_realm_id(parent, self_heap);
        let value = parent.test262_import_foreign_value(child_realm_id, value.clone())?;
        parent.remaining_instructions = parent.config.instruction_budget;
        parent.set_property(&Value::Object(target), key, &value)
    }

    /// `[[Call]]`/`[[Construct]]` on a reverse facade, mirroring
    /// `test262_foreign_call`'s own generic-forwarding path. Deliberately
    /// does not reproduce that function's many TypedArray/Atomics/
    /// ArrayBuffer-detach special cases -- those exist for the forward
    /// direction's buffer-mirror concerns (`test262_foreign_buffer_mirrors`),
    /// which a reverse facade never participates in (only the opaque,
    /// property-forwarding case reaches this module at all; Arrays,
    /// TypedArrays and ArrayBuffers keep `test262_transport_value`'s
    /// existing eager-snapshot transport).
    pub(in super::super) fn test262_reverse_call(
        &mut self,
        wrapper: ObjectId,
        receiver: Value,
        args: Vec<Value>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let (home_heap, target, _, _) = self
            .test262_reverse_reference(wrapper)
            .expect("reverse call has a membrane record");
        let self_heap = self.object_prototype.heap;
        let ptr = self.resolve_reverse_parent(home_heap)?;
        let _guard = register_active(self);
        // SAFETY: see this module's top-level "Soundness" note. `parent`'s
        // own nested calls (`call_native` below, and everything it in turn
        // invokes) manage their own reentrancy discipline; this function
        // never touches `self` again once `parent` is in hand.
        let parent = unsafe { &mut *ptr };
        let child_realm_id = reverse_child_realm_id(parent, self_heap);
        let receiver = parent.test262_import_foreign_value(child_realm_id, receiver)?;
        let args = args
            .into_iter()
            .map(|arg| parent.test262_import_foreign_value(child_realm_id, arg))
            .collect::<Result<Vec<_>, _>>()?;
        parent.remaining_instructions = parent.config.instruction_budget;
        let result = parent.call_native(Value::Object(target), receiver, args, construct);
        let result = match result {
            Ok(value) => Ok(value),
            // RuntimeError represents spec throws until they cross a VM
            // boundary, matching `test262_foreign_call`'s identical
            // handling for its own (opposite-direction) result.
            Err(error) => match parent.error_value(error) {
                Ok(error) => Err(RuntimeError::Thrown(error)),
                Err(error) => Err(error),
            },
        };
        test262_reverse_export_result(parent, child_realm_id, result)
    }
}

/// Finds the key in `parent.test262_realms` whose own child `Vm` is
/// `child_heap` -- i.e. the realm identity `parent` itself would use to
/// address *this* child through the ordinary forward-direction membrane
/// (`test262_import_foreign_value`/`test262_export_foreign_value`). A
/// reverse facade's `home_heap` always names the *direct* parent that
/// created it (`test262_transport_value` only ever builds one for a value
/// crossing from `self` into one of `self`'s own direct `test262_realms`
/// children), so this lookup always succeeds for a live reverse facade.
fn reverse_child_realm_id(parent: &Vm, child_heap: u64) -> ObjectId {
    parent
        .test262_realms
        .iter()
        .find_map(|(&id, realm)| (realm.vm.object_prototype.heap == child_heap).then_some(id))
        .expect("a reverse facade's home realm always owns this child directly")
}

/// `test262_import_foreign_result`'s mirror image: brings a *parent-owned*
/// `Result` (an ordinary value, or a thrown one) into `child_realm_id`'s own
/// heap via the ordinary forward-direction export, rather than importing a
/// child-owned one into the parent.
fn test262_reverse_export_result(
    parent: &mut Vm,
    child_realm_id: ObjectId,
    result: Result<Value, RuntimeError>,
) -> Result<Value, RuntimeError> {
    match result {
        Ok(value) => parent.test262_export_foreign_value(child_realm_id, &value),
        Err(RuntimeError::Thrown(value)) => Err(RuntimeError::Thrown(
            parent.test262_export_foreign_value(child_realm_id, &value)?,
        )),
        Err(error) => Err(error),
    }
}
