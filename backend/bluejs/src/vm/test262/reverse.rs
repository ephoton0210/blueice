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
        let (real_buffer, real_offset, ..) = parent
            .heap
            .typed_array_info(real_target)
            .expect("an imported round-trip value is a live typed array");
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
        // Nothing sets a reverse facade's override (only the forward
        // direction's facades ever get one), so its [[Prototype]] is always
        // the parent's.
        debug_assert!(override_prototype.is_none());
        let self_heap = self.object_prototype.heap;
        let ptr = self.resolve_reverse_parent(home_heap)?;
        let _guard = register_active(self);
        // SAFETY: see this module's top-level "Soundness" note.
        let parent = unsafe { &mut *ptr };
        let child_realm_id = reverse_child_realm_id(parent, self_heap);
        parent.remaining_instructions = parent.config.instruction_budget;
        let prototype = parent
            .object_get_prototype(target)
            .map_err(|error| test262_reverse_export_error(parent, child_realm_id, error))?;
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
        parent
            .object_set_prototype(target, prototype)
            .map_err(|error| test262_reverse_export_error(parent, child_realm_id, error))
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
        let self_heap = self.object_prototype.heap;
        let ptr = self.resolve_reverse_parent(home_heap)?;
        let _guard = register_active(self);
        // SAFETY: see this module's top-level "Soundness" note.
        let parent = unsafe { &mut *ptr };
        let child_realm_id = reverse_child_realm_id(parent, self_heap);
        parent.remaining_instructions = parent.config.instruction_budget;
        parent
            .object_own_property_keys(target)
            .map_err(|error| test262_reverse_export_error(parent, child_realm_id, error))
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
        let descriptor = parent
            .object_get_own_property(target, key)
            .map_err(|error| test262_reverse_export_error(parent, child_realm_id, error))?;
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
        parent
            .ordinary_set_with_receiver(target, &receiver, key, &value)
            .map_err(|error| test262_reverse_export_error(parent, child_realm_id, error))
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
        parent
            .set_property(&Value::Object(target), key, &value)
            .map_err(|error| test262_reverse_export_error(parent, child_realm_id, error))
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
        Err(error) => Err(test262_reverse_export_error(parent, child_realm_id, error)),
    }
}

/// The error a parent-side operation on behalf of `child_realm_id` reports
/// to that child: a thrown value is a parent-heap object (or a primitive),
/// so it crosses the membrane like any other result; every other error means
/// the same in either realm.
fn test262_reverse_export_error(
    parent: &mut Vm,
    child_realm_id: ObjectId,
    error: RuntimeError,
) -> RuntimeError {
    let RuntimeError::Thrown(value) = error else {
        return error;
    };
    match parent.test262_export_foreign_value(child_realm_id, &value) {
        Ok(value) => RuntimeError::Thrown(value),
        Err(error) => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compile, parse, HeapConfig, VmConfig};

    /// The realm whose heap ceiling a sweep raises in small steps.
    #[derive(Clone, Copy)]
    enum Limited {
        Parent,
        Child,
    }

    /// Runs `setup` on a Test262 host and returns the live managed bytes of
    /// the limited realm (its first child realm, for `Limited::Child`).
    fn live_bytes_after(setup: &str, limited: Limited) -> usize {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        vm.execute(&compile(&parse(setup).unwrap()).unwrap())
            .unwrap();
        let realm = match limited {
            Limited::Parent => &mut vm,
            Limited::Child => &mut *vm.test262_realms.values_mut().next().unwrap().vm,
        };
        realm.heap.collect_major();
        realm.heap().stats().managed_bytes
    }

    /// Runs `operation` after `setup` on hosts where one realm's heap
    /// ceiling rises in small steps from what `setup` needs while the other
    /// realm has the default (ample) ceiling, so that only the limited
    /// realm's allocations fail, one after another: the facades and results a
    /// value crossing the membrane needs are made in whichever realm it
    /// crosses into. Each run must succeed or hit the heap limit. Returns how
    /// many hit it.
    fn heap_limit_failures(setup: &str, operation: &str, limited: Limited) -> usize {
        let base = live_bytes_after(setup, limited);
        let setup = compile(&parse(setup).unwrap()).unwrap();
        let operation = compile(&parse(operation).unwrap()).unwrap();
        let mut failures = 0;
        for (nursery_capacity, throwaways) in [(1, 0), (2, 1)] {
            let mut successes_in_a_row = 0;
            for limit in (base.saturating_sub(64)..).step_by(8) {
                let tight = VmConfig {
                    heap: HeapConfig {
                        nursery_capacity,
                        major_threshold_bytes: limit,
                        max_heap_bytes: limit,
                    },
                    ..VmConfig::default()
                };
                let mut parent = match limited {
                    Limited::Parent => Vm::new(tight),
                    Limited::Child => Vm::new(VmConfig::default()),
                }
                .unwrap();
                // Child realms take their configuration from their creator.
                parent.config = match limited {
                    Limited::Parent => VmConfig::default(),
                    Limited::Child => tight,
                };
                if parent.install_test262_harness().is_err() || parent.execute(&setup).is_err() {
                    failures += 1;
                    continue;
                }
                for _ in 0..throwaways {
                    match limited {
                        Limited::Parent => {
                            let _ = parent.heap.alloc_object(None);
                        }
                        Limited::Child => {
                            let child = &mut *parent.test262_realms.values_mut().next().unwrap().vm;
                            let _ = child.heap.alloc_object(None);
                        }
                    }
                }
                let outcome = parent.execute(&operation);
                if let Err(RuntimeError::Heap(_)) = outcome {
                    failures += 1;
                    successes_in_a_row = 0;
                } else {
                    outcome.expect("only the heap limit may stop the operation");
                    successes_in_a_row += 1;
                }
                if successes_in_a_row == 16 {
                    break;
                }
            }
        }
        failures
    }

    /// A parent that shares a fresh `target` and a child-made `rcv` and
    /// `val` (never sent across) with a child realm. Each sweep below starts
    /// from a setup this small, because a heap ceiling fails an allocation
    /// only when nothing earlier in the run demanded as much: a big setup
    /// would hide the operation's own allocations behind its peak.
    const PARENT_TARGET: &str = "
        globalThis.other = $262.createRealm();
        globalThis.target = { x: 1 };
        other.global.target = target;
        other.evalScript('globalThis.rcv = {}; globalThis.val = {}; target.x; Reflect.get; Reflect.set;');";

    /// A parent function `fn` shared with the child, which has a receiver
    /// `rcv` of its own (with `fn` as a method) that never crossed.
    const PARENT_FUNCTION: &str = "
        globalThis.other = $262.createRealm();
        function fn() { return 1 }
        globalThis.fn = fn;
        other.global.fn = fn;
        other.evalScript('globalThis.rcv = { m: fn }; globalThis.rcv2 = {}; fn(); Reflect.apply;');";

    /// Operations of the child on parent-owned objects, each of which imports
    /// a child value into the parent's heap.
    #[test]
    fn values_crossing_into_the_parent_fail_cleanly_at_every_allocation() {
        for (setup, operation) in [
            (PARENT_TARGET, "Reflect.get(target, 'x', rcv)"),
            (PARENT_TARGET, "Reflect.set(target, 'x', 3, rcv)"),
            (PARENT_TARGET, "Reflect.set(target, 'x', val, target)"),
            (PARENT_TARGET, "target.x = val"),
            (PARENT_FUNCTION, "rcv.m()"),
            (PARENT_FUNCTION, "Reflect.apply(fn, rcv2, [])"),
            (PARENT_FUNCTION, "fn.call(rcv2)"),
            (
                "globalThis.other = $262.createRealm();
                 function bad() { null.x }
                 globalThis.bad = bad;
                 other.global.bad = bad;
                 other.evalScript('try { bad() } catch (e) {}');",
                "try { bad() } catch (e) {}",
            ),
            (
                "globalThis.other = $262.createRealm();
                 globalThis.target = { x: 1 };
                 other.global.target = target;
                 other.evalScript('globalThis.rcv = {}; Object.setPrototypeOf;');",
                "Object.setPrototypeOf(target, rcv)",
            ),
        ] {
            let operation = format!("other.evalScript(`{operation}`)");
            assert!(
                heap_limit_failures(setup, &operation, Limited::Parent) > 0,
                "{operation}"
            );
        }
    }

    /// Operations of the child on parent-owned objects, each of which
    /// exports a parent value into the child's heap.
    #[test]
    fn values_crossing_into_the_child_fail_cleanly_at_every_allocation() {
        let shared = "
            globalThis.other = $262.createRealm();
            globalThis.protoB = { b: 1 };
            globalThis.target2 = Object.create(protoB);
            globalThis.target3 = { o: {}, get g() { return 1 }, set g(v) {} };
            other.global.target2 = target2;
            other.global.target3 = target3;
            other.evalScript('Object.getPrototypeOf; Object.getOwnPropertyDescriptor;');";
        let thrower = "
            globalThis.other = $262.createRealm();
            function thrower() { throw new RangeError('parent') }
            globalThis.thrower = thrower;
            other.global.thrower = thrower;
            other.evalScript('try { thrower() } catch (e) {}');";
        for (setup, operation) in [
            (shared, "Object.getPrototypeOf(target2)"),
            (shared, "Object.getOwnPropertyDescriptor(target3, 'o')"),
            (shared, "Object.getOwnPropertyDescriptor(target3, 'g')"),
            (thrower, "try { thrower() } catch (e) {}"),
        ] {
            let operation = format!("other.evalScript(`{operation}`)");
            assert!(
                heap_limit_failures(setup, &operation, Limited::Child) > 0,
                "{operation}"
            );
        }
    }

    /// Runs `source` on a Test262 host.
    fn evaluate(source: &str) -> Result<Value, RuntimeError> {
        let mut vm = Vm::default();
        vm.install_test262_harness().unwrap();
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
    }

    fn assert_true(source: &str) {
        assert_eq!(evaluate(source), Ok(Value::Bool(true)), "{source}");
    }

    /// Enters the child directly, after the parent's export call returned.
    /// A forward getter keeps its parent active; bypass that boundary here
    /// to verify a facade really refuses an absent parent instead of using
    /// a stale pointer.
    fn operation_without_a_live_parent(operation: &str) -> Result<Value, RuntimeError> {
        let mut parent = Vm::default();
        parent.install_test262_harness().unwrap();
        parent
            .execute(
                &compile(
                    &parse(
                        "var other = $262.createRealm();
             var target = { x: 1, get y() { return 2 } };
             function fn() { return 1 }
             other.global.target = target; other.global.fn = fn;",
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
        let code = compile(
            &parse(&format!(
                "var caught = false;
             try {{ ({operation}); }} catch (e) {{
               caught = e instanceof TypeError && e.message.includes('no longer reachable');
             }} caught"
            ))
            .unwrap(),
        )
        .unwrap();
        parent
            .test262_realms
            .values_mut()
            .next()
            .unwrap()
            .vm
            .execute(&code)
    }

    #[test]
    fn every_reverse_operation_fails_safely_when_the_parent_is_not_on_the_call_chain() {
        for operation in [
            "target.x",
            "target.y",
            "target.x = 2",
            "Object.getPrototypeOf(target)",
            "Object.setPrototypeOf(target, null)",
            "Object.getOwnPropertyNames(target)",
            "Reflect.ownKeys(target)",
            "Object.getOwnPropertyDescriptor(target, 'x')",
            "Object.getOwnPropertyDescriptor(target, 'y')",
            "Reflect.set(target, 'x', 1, {})",
            "Reflect.set(target, 'x', 1)",
            "fn()",
            "new fn()",
            "fn.call({}, 1, 2)",
        ] {
            assert_eq!(
                operation_without_a_live_parent(operation),
                Ok(Value::Bool(true)),
                "{operation}"
            );
        }
    }

    #[test]
    fn reverse_operations_forward_into_the_active_parent() {
        for source in [
            // Own-property descriptors cross with their values and accessors.
            "(() => { var other = $262.createRealm();
                      var target = { x: 1, get y() { return 2 } };
                      other.global.target = target;
                      return other.evalScript(`
                        var dx = Object.getOwnPropertyDescriptor(target, 'x');
                        var dy = Object.getOwnPropertyDescriptor(target, 'y');
                        dx.value === 1 && dx.writable === true && typeof dy.get === 'function' && dy.set === undefined
                          && Object.getOwnPropertyDescriptor(target, 'missing') === undefined`) })()",
            // Prototype reads and writes.
            "(() => { var other = $262.createRealm();
                      var proto = { inherited: 1 }; var target = Object.create(proto);
                      other.global.target = target; other.global.proto = proto;
                      return other.evalScript(`
                        Object.getPrototypeOf(target) === proto && Object.setPrototypeOf(target, null) === target
                          && Object.getPrototypeOf(target) === null`) })()",
            // Own keys, get and set with and without an explicit receiver.
            "(() => { var other = $262.createRealm();
                      var target = { a: 1, b: 2 }; other.global.target = target;
                      var result = other.evalScript(`
                        Reflect.ownKeys(target).join() === 'a,b' && target.a === 1 && (target.b = 5, true)
                          && Reflect.set(target, 'c', 3, target) === true`);
                      return result && target.b === 5 && target.c === 3 })()",
            // Getters observe the receiver the child supplies.
            "(() => { var other = $262.createRealm();
                      var target = { get self() { return this } }; other.global.target = target;
                      var seen = other.evalScript('Reflect.get(target, \"self\", 7)');
                      return seen == 7 })()",
            // A parent function called and constructed from the child, with
            // results and thrown values crossing back.
            "(() => { var other = $262.createRealm();
                      function Box(v) { this.v = v } function thrower() { throw new RangeError('parent') }
                      other.global.Box = Box; other.global.thrower = thrower;
                      return other.evalScript(`
                        var b = new Box(4); var caught;
                        try { thrower() } catch (e) { caught = e }
                        b.v === 4 && caught.message === 'parent'`) })()",
            // A child that asks a parent Proxy for something its trap refuses
            // gets the error the trap threw.
            "(() => { var other = $262.createRealm();
                      var refuse = (what) => function () { throw new RangeError(what) };
                      var target = new Proxy({}, {
                        getPrototypeOf: refuse('getPrototypeOf'),
                        setPrototypeOf: refuse('setPrototypeOf'),
                        ownKeys: refuse('ownKeys'),
                        getOwnPropertyDescriptor: refuse('getOwnPropertyDescriptor'),
                        set: refuse('set'),
                      });
                      other.global.target = target;
                      return other.evalScript(`
                        var seen = [];
                        for (var attempt of [
                          () => Object.getPrototypeOf(target),
                          () => Object.setPrototypeOf(target, null),
                          () => Object.getOwnPropertyNames(target),
                          () => Object.getOwnPropertyDescriptor(target, 'x'),
                          () => { target.z = 1 },
                          () => Reflect.set(target, 'z', 1, {}),
                        ]) {
                          try { attempt() } catch (e) { seen.push(e.message) }
                        }
                        seen.join()`) === 'getPrototypeOf,setPrototypeOf,ownKeys,getOwnPropertyDescriptor,set,set' })()",
        ] {
            assert_true(source);
        }
    }

    /// The parent whose child realm holds a facade of the parent's
    /// constructor `Ctor` and the snapshot of the parent's Uint8Array `ta`,
    /// with the child, the constructor's facade and the snapshot's id.
    fn realm_with_a_constructor_and_a_snapshot() -> (Vm, *const Vm, ObjectId, ObjectId) {
        let mut parent = Vm::default();
        parent.install_test262_harness().unwrap();
        let setup = "globalThis.other = $262.createRealm();
                     function Ctor() {}
                     globalThis.ta = new Uint8Array(4);
                     other.global.Ctor = Ctor;
                     other.global.ta = ta;";
        parent
            .execute(&compile(&parse(setup).unwrap()).unwrap())
            .unwrap();
        let realm = parent.test262_realms.values().next().unwrap();
        let snapshot = *realm
            .imported_values
            .iter()
            .find(|(_, imported)| {
                imported
                    .value
                    .object_id()
                    .is_some_and(|real| parent.heap.typed_array_info(real).is_ok())
            })
            .expect("the typed array crossed as a snapshot")
            .0;
        let child: *const Vm = &*realm.vm;
        let facade = *realm
            .vm
            .test262_reverse_values
            .iter()
            .find(|(_, value)| value.constructible)
            .expect("the constructor crossed as a facade")
            .0;
        (parent, child, facade, snapshot)
    }

    #[test]
    fn writing_into_the_real_result_of_a_construction_needs_a_live_parent_and_a_facade() {
        let (mut parent, child, facade, snapshot) = realm_with_a_constructor_and_a_snapshot();
        // SAFETY: `child` is owned by `parent`, which outlives every use.
        let child = unsafe { &*child };
        // Without the parent's call in flight the write cannot find it.
        assert_eq!(
            child.test262_reverse_write_into_real_construction_result(facade, snapshot, 0, &[9]),
            Err(RuntimeError::TypeError(
                "the Test262 realm this value belongs to is no longer reachable".into()
            ))
        );
        let guard = register_active(&mut parent);
        // A wrapper that is no reverse facade has no parent to write into.
        assert_eq!(
            child.test262_reverse_write_into_real_construction_result(snapshot, snapshot, 0, &[9]),
            Ok(false)
        );
        // A result that is not a stand-in for a real parent object has none
        // either.
        assert_eq!(
            child.test262_reverse_write_into_real_construction_result(
                facade,
                child.object_prototype,
                0,
                &[9]
            ),
            Ok(false)
        );
        // The real object receives the bytes.
        assert_eq!(
            child.test262_reverse_write_into_real_construction_result(facade, snapshot, 1, &[9]),
            Ok(true)
        );
        drop(guard);
        let ta = compile(&parse("ta.join()").unwrap()).unwrap();
        assert_eq!(parent.execute(&ta), Ok(Value::String("0,9,0,0".into())));
        // A detached real buffer refuses the write.
        parent
            .execute(&compile(&parse("$262.detachArrayBuffer(ta.buffer)").unwrap()).unwrap())
            .unwrap();
        let _guard = register_active(&mut parent);
        assert_eq!(
            child.test262_reverse_write_into_real_construction_result(facade, snapshot, 0, &[9]),
            Err(RuntimeError::TypeError(
                "ArrayBuffer has been detached".into()
            ))
        );
    }

    #[test]
    fn a_facade_resolves_only_to_a_different_realm_with_a_call_in_flight() {
        let mut child = Vm::default();
        let mut parent = Vm::default();
        let parent_heap = parent.object_prototype.heap;
        // The parent has no call in flight.
        assert_eq!(
            child.resolve_reverse_parent(parent_heap),
            Err(RuntimeError::TypeError(
                "the Test262 realm this value belongs to is no longer reachable".into()
            ))
        );
        // Once it has, the facade reaches it.
        let guard = register_active(&mut parent);
        assert_eq!(
            child.resolve_reverse_parent(parent_heap),
            Ok(&mut parent as *mut Vm)
        );
        drop(guard);
        // A facade whose home is the realm asking would alias it.
        let own_heap = child.object_prototype.heap;
        let _guard = register_active(&mut child);
        assert_eq!(
            child.resolve_reverse_parent(own_heap),
            Err(RuntimeError::TypeError(
                "a Test262 reverse membrane facade cannot resolve to its own realm".into()
            ))
        );
    }
}
