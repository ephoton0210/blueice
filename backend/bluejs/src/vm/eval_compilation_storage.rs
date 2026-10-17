// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Eval inputs, compiler snapshots and root code keep independent native owners.

use super::compilation_input_storage::{parsing_charge, CompilationInput};
use super::host_scheduling::{owned_code_bytes, SharedSchedulingCharge};
use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/eval_environment_storage.rs"]
mod environment_storage_tests;

pub(super) type ParsedEvalInput = CompilationInput<crate::Program>;

pub(super) struct PreparedEvalCode {
    code: Bytecode,
    _charge: Option<SharedSchedulingCharge>,
}
impl std::ops::Deref for PreparedEvalCode {
    type Target = Bytecode;
    fn deref(&self) -> &Bytecode {
        &self.code
    }
}
impl PreparedEvalCode {
    /// A host-neutral caller can enter an already bounded child. Bind all
    /// surviving code before any child execution context is displaced.
    pub(super) fn attach(&mut self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        scope.shared_charge(0)?;
        if let Some(owner) = &self._charge {
            if !owner.belongs_to(scope) {
                return Err(scope.fail());
            }
        } else {
            self._charge = Some(scope.shared_charge(owned_code_bytes(&self.code))?);
        }
        self.code
            .attach_runtime_sources(&scope.heap_native_budget())
    }
}
impl ParsedEvalInput {
    pub(super) fn parse_eval(
        source: &str,
        strict: bool,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let charge = parsing_charge(source, scope)?;
        let ast = crate::parse_eval(source, strict).map_err(|error| {
            error
                .resource
                .unwrap_or(RuntimeError::SyntaxError(error.message))
        })?;
        Self::from_parsed(ast, charge, scope)
    }
    pub(super) fn parse_script(
        source: &str,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let charge = parsing_charge(source, scope)?;
        let ast = crate::parser::parse_encoded(source).map_err(|error| {
            error
                .resource
                .unwrap_or(RuntimeError::SyntaxError(error.message))
        })?;
        Self::from_parsed(ast, charge, scope)
    }
    pub(super) fn compile_with(
        mut self,
        scope: Option<&SchedulingScope>,
        compile: impl FnOnce(&crate::Program) -> Result<Bytecode, crate::CompileError>,
    ) -> Result<PreparedEvalCode, RuntimeError> {
        self.prepare(scope)?;
        let mut code =
            compile(&self.ast).map_err(|error| RuntimeError::SyntaxError(error.to_string()))?;
        self.install(&mut code, scope)?;
        let charge = scope
            .map(|scope| scope.shared_charge(owned_code_bytes(&code)))
            .transpose()?;
        drop(self);
        Ok(PreparedEvalCode {
            code,
            _charge: charge,
        })
    }
}

/// Borrowed candidates and returned compiler snapshots each retain admission.
pub(super) struct EvalCompileEnvironment {
    pub(super) visible: Vec<(String, Binding, u32)>,
    pub(super) dynamic_captures: HashMap<u32, ObjectId>,
    pub(super) variables: Vec<String>,
    pub(super) lexical: Vec<String>,
    _charge: Option<SharedSchedulingCharge>,
}
fn names_bytes(names: &Vec<String>) -> usize {
    names
        .capacity()
        .saturating_mul(std::mem::size_of::<String>())
        .saturating_add(
            names
                .iter()
                .map(String::capacity)
                .fold(0usize, usize::saturating_add),
        )
}

struct Scratch<T> {
    values: Vec<T>,
    _charge: Option<SharedSchedulingCharge>,
}
fn allocation_error(scope: Option<&SchedulingScope>) -> RuntimeError {
    scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
}
impl<T> Scratch<T> {
    fn new(count: usize, scope: Option<&SchedulingScope>) -> Result<Self, RuntimeError> {
        let bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .ok_or_else(|| allocation_error(scope))?;
        let mut charge = scope.map(|scope| scope.shared_charge(bytes)).transpose()?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| allocation_error(scope))?;
        if let Some(charge) = &mut charge {
            charge.resize(values.capacity().saturating_mul(std::mem::size_of::<T>()))?;
        }
        Ok(Self {
            values,
            _charge: charge,
        })
    }
}
struct VisibleRef<'a> {
    binding: &'a Binding,
    slot: u32,
    category: u8,
    order: usize,
}
struct NameList {
    values: Vec<String>,
    charge: Option<SharedSchedulingCharge>,
}
fn copy_name(name: &str, scope: Option<&SchedulingScope>) -> Result<String, RuntimeError> {
    let mut result = String::new();
    result
        .try_reserve_exact(name.len())
        .map_err(|_| allocation_error(scope))?;
    result.push_str(name);
    Ok(result)
}
fn copy_names(
    references: &[&str],
    scope: Option<&SchedulingScope>,
) -> Result<NameList, RuntimeError> {
    let mut remaining = references
        .iter()
        .try_fold(0usize, |bytes, name| bytes.checked_add(name.len()))
        .ok_or_else(|| allocation_error(scope))?;
    let minimum = references
        .len()
        .checked_mul(std::mem::size_of::<String>())
        .and_then(|bytes| bytes.checked_add(remaining))
        .ok_or_else(|| allocation_error(scope))?;
    let charge = scope
        .map(|scope| scope.shared_charge(minimum))
        .transpose()?;
    let mut result = NameList {
        values: Vec::new(),
        charge,
    };
    result
        .values
        .try_reserve_exact(references.len())
        .map_err(|_| allocation_error(scope))?;
    let slots = result
        .values
        .capacity()
        .saturating_mul(std::mem::size_of::<String>());
    if let Some(charge) = &mut result.charge {
        charge.resize(slots.saturating_add(remaining))?;
    }
    let mut payload = 0usize;
    for name in references {
        let copy = copy_name(name, scope)?;
        remaining -= name.len();
        payload = payload.saturating_add(copy.capacity());
        result.values.push(copy);
        if let Some(charge) = &mut result.charge {
            charge.resize(slots.saturating_add(payload).saturating_add(remaining))?;
        }
    }
    Ok(result)
}
impl EvalCompileEnvironment {
    fn retained_bytes(&self) -> usize {
        self.visible
            .capacity()
            .saturating_mul(std::mem::size_of::<(String, Binding, u32)>())
            .saturating_add(
                self.visible
                    .iter()
                    .fold(0usize, |bytes, (name, binding, _)| {
                        bytes
                            .saturating_add(name.capacity())
                            .saturating_add(binding.name.capacity())
                    }),
            )
            .saturating_add(
                self.dynamic_captures
                    .capacity()
                    .saturating_mul(std::mem::size_of::<(u32, ObjectId)>()),
            )
            .saturating_add(names_bytes(&self.variables))
            .saturating_add(names_bytes(&self.lexical))
    }
    pub(super) fn visible(vm: &Vm) -> Result<Self, RuntimeError> {
        let scope = vm.host_scheduling.as_ref();
        let count = vm
            .active_scope_slots
            .iter()
            .try_fold(vm.cells.len(), |count, slots| {
                count.checked_add(slots.len())
            })
            .and_then(|count| {
                count.checked_add(vm.bindings.iter().filter(|value| value.is_some()).count())
            })
            .ok_or_else(|| allocation_error(scope))?;
        let mut scratch = Scratch::<VisibleRef<'_>>::new(count, scope)?;
        let mut ordinal = 0usize;
        for slots in &vm.active_scope_slots {
            for &slot in slots {
                scratch.values.push(VisibleRef {
                    binding: &vm.binding_metadata[slot as usize],
                    slot,
                    category: 0,
                    order: usize::MAX - ordinal,
                });
                ordinal += 1;
            }
        }
        for (order, &slot) in vm.cells.keys().enumerate() {
            scratch.values.push(VisibleRef {
                binding: &vm.binding_metadata[slot],
                slot: slot as u32,
                category: 1,
                order,
            });
        }
        for (slot, value) in vm.bindings.iter().enumerate() {
            if value.is_some() {
                scratch.values.push(VisibleRef {
                    binding: &vm.binding_metadata[slot],
                    slot: slot as u32,
                    category: 2,
                    order: slot,
                });
            }
        }
        scratch.values.sort_unstable_by(|left, right| {
            left.binding
                .name
                .cmp(&right.binding.name)
                .then(left.category.cmp(&right.category))
                .then(left.order.cmp(&right.order))
        });
        // Last active declaration wins; captured and live fallback candidates
        // retain their original first-match order. Only winners copy names.
        scratch
            .values
            .dedup_by(|left, right| left.binding.name == right.binding.name);
        let dynamic_cell = |candidate: &VisibleRef<'_>| {
            (vm.parameter_eval_env.is_some()
                && !vm
                    .active_scope_slots
                    .iter()
                    .any(|slots| slots.contains(&candidate.slot)))
            .then(|| {
                vm.dynamic_eval_bindings
                    .get(&candidate.binding.name)
                    .map(|binding| binding.cell)
            })
            .flatten()
        };
        let dynamic_count = scratch
            .values
            .iter()
            .filter(|candidate| dynamic_cell(candidate).is_some())
            .count();
        let mut remaining = scratch
            .values
            .iter()
            .try_fold(0usize, |bytes, candidate| {
                candidate
                    .binding
                    .name
                    .len()
                    .checked_mul(2)
                    .and_then(|bytes2| bytes.checked_add(bytes2))
            })
            .ok_or_else(|| allocation_error(scope))?;
        let visible_min = scratch
            .values
            .len()
            .checked_mul(std::mem::size_of::<(String, Binding, u32)>())
            .ok_or_else(|| allocation_error(scope))?;
        let dynamic_min = dynamic_count
            .checked_mul(std::mem::size_of::<(u32, ObjectId)>())
            .ok_or_else(|| allocation_error(scope))?;
        let minimum = visible_min
            .checked_add(dynamic_min)
            .and_then(|bytes| bytes.checked_add(remaining))
            .ok_or_else(|| allocation_error(scope))?;
        let mut result = Self {
            visible: Vec::new(),
            dynamic_captures: HashMap::new(),
            variables: Vec::new(),
            lexical: Vec::new(),
            _charge: scope
                .map(|scope| scope.shared_charge(minimum))
                .transpose()?,
        };
        result
            .visible
            .try_reserve_exact(scratch.values.len())
            .map_err(|_| allocation_error(scope))?;
        let visible_slots = result
            .visible
            .capacity()
            .saturating_mul(std::mem::size_of::<(String, Binding, u32)>());
        if let Some(charge) = &mut result._charge {
            charge.resize(
                visible_slots
                    .saturating_add(dynamic_min)
                    .saturating_add(remaining),
            )?;
        }
        result
            .dynamic_captures
            .try_reserve(dynamic_count)
            .map_err(|_| allocation_error(scope))?;
        let slots = visible_slots.saturating_add(
            result
                .dynamic_captures
                .capacity()
                .saturating_mul(std::mem::size_of::<(u32, ObjectId)>()),
        );
        if let Some(charge) = &mut result._charge {
            charge.resize(slots.saturating_add(remaining))?;
        }
        let mut payload = 0usize;
        for candidate in &scratch.values {
            let original = candidate.binding;
            let name = copy_name(&original.name, scope)?;
            remaining -= original.name.len();
            payload = payload.saturating_add(name.capacity());
            if let Some(charge) = &mut result._charge {
                charge.resize(slots.saturating_add(payload).saturating_add(remaining))?;
            }
            let mut binding = Binding {
                name: copy_name(&original.name, scope)?,
                mutable: original.mutable,
                strict_immutable: original.strict_immutable,
                lexical: original.lexical,
                catch_parameter: original.catch_parameter,
                eval_var: original.eval_var,
            };
            if let Some(cell) = dynamic_cell(candidate) {
                binding.mutable = true;
                binding.strict_immutable = false;
                binding.lexical = false;
                binding.catch_parameter = false;
                binding.eval_var = true;
                result.dynamic_captures.insert(candidate.slot, cell);
            }
            remaining -= original.name.len();
            payload = payload.saturating_add(binding.name.capacity());
            result.visible.push((name, binding, candidate.slot));
            if let Some(charge) = &mut result._charge {
                charge.resize(slots.saturating_add(payload).saturating_add(remaining))?;
            }
        }
        Ok(result)
    }
    pub(super) fn finish_names(&mut self, vm: &Vm, global: bool) -> Result<(), RuntimeError> {
        let scope = vm.host_scheduling.as_ref();
        match (&self._charge, scope) {
            (Some(owner), Some(scope)) if !owner.belongs_to(scope) => return Err(scope.fail()),
            (Some(owner), None) => return Err(owner.fail()),
            (None, Some(scope)) => self._charge = Some(scope.shared_charge(self.retained_bytes())?),
            _ => {}
        }
        if let Some(scope) = scope {
            scope.shared_charge(0)?;
        }
        let variables = if global {
            let mut references = Scratch::<&str>::new(self.visible.len(), scope)?;
            references
                .values
                .extend(self.visible.iter().map(|(name, _, _)| name.as_str()));
            copy_names(&references.values, scope)?
        } else {
            let position = vm
                .active_scopes
                .iter()
                .position(|scope| *scope == vm.variable_scope);
            let slots = position.map_or(&[][..], |position| {
                vm.active_scope_slots[position].as_slice()
            });
            let source = slots
                .iter()
                .filter_map(|slot| {
                    let binding = &vm.binding_metadata[*slot as usize];
                    (!binding.lexical).then_some(binding.name.as_str())
                })
                .chain(vm.dynamic_eval_bindings.keys().map(String::as_str));
            let mut references = Scratch::<&str>::new(source.clone().count(), scope)?;
            references.values.extend(source);
            references.values.sort_unstable();
            references.values.dedup();
            copy_names(&references.values, scope)?
        };
        let position = vm
            .active_scopes
            .iter()
            .position(|scope| *scope == vm.variable_scope);
        let start = position.unwrap_or(0);
        let ordinary = vm
            .callee
            .object_id()
            .and_then(|callee| vm.heap.closure_code(callee).ok().flatten())
            .is_some_and(|code| !code.arrow);
        let body = if position.is_none() && ordinary {
            vm.variable_scope_lexicals.as_slice()
        } else {
            &[]
        };
        let source = vm.active_scope_slots[start..]
            .iter()
            .flat_map(|slots| slots.iter().copied())
            .filter_map(|slot| {
                let binding = &vm.binding_metadata[slot as usize];
                (binding.lexical && !binding.catch_parameter).then_some(binding.name.as_str())
            })
            .chain(body.iter().map(String::as_str));
        let global_names = vm
            .global_bindings
            .iter()
            .filter(|(_, binding)| !binding.property)
            .map(|(name, _)| name.as_str());
        let globals = if global {
            global_names.clone().count()
        } else {
            0
        };
        let count = source
            .clone()
            .count()
            .checked_add(globals)
            .ok_or_else(|| allocation_error(scope))?;
        let mut references = Scratch::<&str>::new(count, scope)?;
        references.values.extend(source);
        references.values.sort_unstable();
        references.values.dedup();
        if global {
            references.values.extend(global_names);
        }
        let lexical = copy_names(&references.values, scope)?;
        drop(references);
        let old = names_bytes(&self.variables).saturating_add(names_bytes(&self.lexical));
        // Both complete candidates coexist with the previous snapshot. Destroy
        // old data before refunding it, then transfer already-admitted leases.
        let NameList {
            values: variable_values,
            charge: variable_charge,
        } = variables;
        let NameList {
            values: lexical_values,
            charge: lexical_charge,
        } = lexical;
        self.variables = variable_values;
        self.lexical = lexical_values;
        if let Some(charge) = &mut self._charge {
            charge.release_to(charge.bytes() - old);
        }
        for charge in [variable_charge, lexical_charge].into_iter().flatten() {
            if let Some(owner) = &mut self._charge {
                owner.absorb(charge)?;
            } else {
                self._charge = Some(charge);
            }
        }
        Ok(())
    }
    #[cfg(any(test, coverage))]
    pub(super) fn adopt(
        visible: Vec<(String, Binding, u32)>,
        dynamic_captures: HashMap<u32, ObjectId>,
        variables: Vec<String>,
        lexical: Vec<String>,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let bytes = visible
            .capacity()
            .saturating_mul(std::mem::size_of::<(String, Binding, u32)>())
            .saturating_add(visible.iter().fold(0usize, |bytes, (name, binding, _)| {
                bytes
                    .saturating_add(name.capacity())
                    .saturating_add(binding.name.capacity())
            }))
            .saturating_add(
                dynamic_captures
                    .capacity()
                    .saturating_mul(std::mem::size_of::<(u32, ObjectId)>()),
            )
            .saturating_add(names_bytes(&variables))
            .saturating_add(names_bytes(&lexical));
        let charge = scope.map(|scope| scope.shared_charge(bytes)).transpose()?;
        Ok(Self {
            visible,
            dynamic_captures,
            variables,
            lexical,
            _charge: charge,
        })
    }
}
