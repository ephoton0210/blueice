// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Prepare function-eval records before publishing cells or environment edges.

use super::host_scheduling::value_payload_bytes;
use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/dynamic_binding_admission.rs"]
mod dynamic_binding_tests;

enum Declarations<'a> {
    Eval(&'a Bytecode),
    Recreate {
        slot: usize,
        name: &'a str,
        cell: ObjectId,
    },
}

struct Declaration<'a> {
    slot: usize,
    name: &'a str,
    binding: bool,
    cell: Option<ObjectId>,
    code: Option<&'a Bytecode>,
}

impl<'a> Declarations<'a> {
    fn visit(&self, vm: &Vm, mut visit: impl FnMut(Declaration<'a>)) {
        match *self {
            Self::Eval(code) => {
                // Captured eval vars need an alias only when this eval frame
                // has not already recorded it. No name copy for a no-op.
                for slot in 0..code.captures.len() {
                    let name = code.bindings[slot].name.as_str();
                    if vm
                        .dynamic_eval_bindings
                        .get(name)
                        .is_some_and(|binding| vm.cells.get(&slot) == Some(&binding.cell))
                        && vm.eval_dynamic_slots.get(&slot).map(String::as_str) != Some(name)
                    {
                        visit(Declaration {
                            slot,
                            name,
                            binding: false,
                            cell: None,
                            code: None,
                        });
                    }
                }
                // compile_eval excludes existing VariableEnvironment names.
                for &slot in &code.dynamic_eval_slots {
                    let slot = slot as usize;
                    visit(Declaration {
                        slot,
                        name: &code.bindings[slot].name,
                        binding: true,
                        cell: None,
                        code: Some(code),
                    });
                }
            }
            Self::Recreate { slot, name, cell } => {
                visit(Declaration {
                    slot,
                    name,
                    binding: true,
                    cell: Some(cell),
                    code: None,
                });
            }
        }
    }
}

impl Declaration<'_> {
    fn shadowed<'b>(&'b self, vm: &'b Vm) -> impl Iterator<Item = ObjectId> + 'b {
        // Parameter-eval environments resolve names through their object.
        // Only object-free environments need to redirect static captures.
        self.code.into_iter().flat_map(move |code| {
            (0..code.captures.len()).filter_map(move |slot| {
                (vm.parameter_eval_env.is_none() && code.bindings[slot].name == self.name)
                    .then(|| vm.cells.get(&slot).copied())
                    .flatten()
            })
        })
    }
}

struct PreparedDeclaration {
    slot: usize,
    name: Option<String>,
    alias: Option<String>,
    shadowed: Vec<ObjectId>,
    cell: Option<ObjectId>,
    previous_env_cell: Option<ObjectId>,
    recreated: bool,
    initialized: bool,
    published_env: bool,
}

fn minimum_table_bytes<K, V>(table: &HashMap<K, V>, added: usize) -> usize {
    let required = table.len().saturating_add(added);
    if required > table.capacity() {
        required.saturating_mul(std::mem::size_of::<(K, V)>())
    } else {
        0
    }
}

impl Vm {
    pub(super) fn publish_function_eval_declarations(
        &mut self,
        code: &Bytecode,
    ) -> Result<(), RuntimeError> {
        self.publish_function_eval_bindings(Declarations::Eval(code), Value::Undefined)
    }

    pub(super) fn recreate_function_eval_var(
        &mut self,
        slot: usize,
        name: &str,
        cell: ObjectId,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.publish_function_eval_bindings(Declarations::Recreate { slot, name, cell }, value)
    }

    fn publish_function_eval_bindings(
        &mut self,
        declarations: Declarations<'_>,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let result = self.publish_function_eval_bindings_inner(declarations, &value);
        drop(value);
        // All temporary leases have dropped, including on a latched refusal.
        self.reconcile_scheduling_ownership();
        result
    }

    fn publish_function_eval_bindings_inner(
        &mut self,
        declarations: Declarations<'_>,
        value: &Value,
    ) -> Result<(), RuntimeError> {
        let payload_charge = self.charge_execution_payload(value_payload_bytes(value));
        let mut count = 0usize;
        let mut dynamic_count = 0usize;
        let mut dynamic_added = 0usize;
        let mut alias_added = 0usize;
        let mut bytes = 0usize;
        declarations.visit(self, |row| {
            count += 1;
            let alias =
                self.eval_dynamic_slots.get(&row.slot).map(String::as_str) != Some(row.name);
            dynamic_count += usize::from(row.binding);
            dynamic_added +=
                usize::from(row.binding && !self.dynamic_eval_bindings.contains_key(row.name));
            alias_added += usize::from(alias && !self.eval_dynamic_slots.contains_key(&row.slot));
            bytes = bytes
                .saturating_add(
                    row.name
                        .len()
                        .saturating_mul(usize::from(row.binding) + usize::from(alias)),
                )
                .saturating_add(
                    row.shadowed(self)
                        .count()
                        .saturating_mul(std::mem::size_of::<ObjectId>()),
                );
        });
        if count == 0 {
            return self.synchronize_scheduling_usage();
        }
        // New cells and any replaced environment cells remain roots until the
        // batch commits. Only object values need an extra root, not primitives.
        let roots = dynamic_count
            .saturating_mul(2)
            .saturating_add(usize::from(value.object_id().is_some()));
        let root_capacity = self.stack.len().saturating_add(roots);
        let root_growth = root_capacity
            .saturating_sub(self.stack.capacity())
            .saturating_mul(std::mem::size_of::<Value>());
        let minimum = bytes
            .saturating_add(count.saturating_mul(std::mem::size_of::<PreparedDeclaration>()))
            .saturating_add(minimum_table_bytes(
                &self.dynamic_eval_bindings,
                dynamic_added,
            ))
            .saturating_add(minimum_table_bytes(&self.eval_dynamic_slots, alias_added))
            .saturating_add(root_growth);
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: minimum,
            ..Default::default()
        })?;
        let dynamic = SchedulingMapCapacity::for_additional(
            &self.dynamic_eval_bindings,
            dynamic_added,
            self.host_scheduling.as_ref(),
        )?;
        let aliases = SchedulingMapCapacity::for_additional(
            &self.eval_dynamic_slots,
            alias_added,
            self.host_scheduling.as_ref(),
        )?;
        let mut rows = Vec::with_capacity(count);
        declarations.visit(self, |row| {
            let alias = (self.eval_dynamic_slots.get(&row.slot).map(String::as_str)
                != Some(row.name))
            .then(|| row.name.to_owned());
            let mut shadowed = Vec::with_capacity(row.shadowed(self).count());
            shadowed.extend(row.shadowed(self));
            rows.push(PreparedDeclaration {
                slot: row.slot,
                name: row.binding.then(|| row.name.to_owned()),
                alias,
                shadowed,
                cell: row.cell,
                previous_env_cell: None,
                recreated: row.binding && row.cell.is_some(),
                initialized: false,
                published_env: false,
            });
        });
        let actual = rows
            .iter()
            .fold(host_scheduling::slots(&rows), |bytes, row| {
                bytes
                    .saturating_add(row.name.as_ref().map_or(0, String::capacity))
                    .saturating_add(row.alias.as_ref().map_or(0, String::capacity))
                    .saturating_add(host_scheduling::slots(&row.shadowed))
            })
            .saturating_add(dynamic.staging_bytes())
            .saturating_add(aliases.staging_bytes());
        let staging_charge = self.charge_execution_control(actual);
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: root_growth,
            ..Default::default()
        })?;
        if self
            .stack
            .try_reserve_exact(root_capacity.saturating_sub(self.stack.len()))
            .is_err()
        {
            return Err(self
                .host_scheduling
                .as_ref()
                .map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail));
        }
        self.synchronize_scheduling_usage()?;
        let base = self.stack.len();
        if let Some(id) = value.object_id() {
            self.stack.push(Value::Object(id));
        }
        let result = (|| {
            // Capture old environment edges before allocations can collect.
            for row in &mut rows {
                if let (Some(env), Some(name)) = (self.parameter_eval_env, row.name.as_ref()) {
                    row.previous_env_cell = self.heap.get_own(env, name.as_str())?.map(|value| {
                        value
                            .object_id()
                            .expect("eval environment names hold ordinary cells")
                    });
                    if let Some(id) = row.previous_env_cell {
                        self.stack.push(Value::Object(id));
                    }
                }
            }
            // Initialize all cells before publishing any environment property.
            for row in &mut rows {
                if row.name.is_none() {
                    continue;
                }
                let cell = if let Some(cell) = row.cell {
                    cell
                } else {
                    let cell = self.with_roots(|heap| heap.alloc_object(None))?;
                    row.cell = Some(cell);
                    cell
                };
                self.stack.push(Value::Object(cell));
                self.with_roots(|heap| heap.set(cell, "value", value.clone()))?;
                row.initialized = true;
            }
            if let Some(env) = self.parameter_eval_env {
                for row in &mut rows {
                    if let Some(name) = &row.name {
                        self.with_roots(|heap| {
                            heap.set(env, name.as_str(), Value::Object(row.cell.unwrap()))
                        })?;
                        row.published_env = true;
                    }
                }
            }
            Ok(())
        })();
        if result.is_err() {
            for row in rows.iter().rev() {
                if row.published_env {
                    self.heap.restore_eval_environment_cell(
                        self.parameter_eval_env.unwrap(),
                        row.name.as_deref().unwrap(),
                        row.previous_env_cell,
                    );
                }
                if row.recreated && row.initialized {
                    self.heap
                        .delete(row.cell.unwrap(), "value")
                        .expect("rollback of an ordinary cell cannot allocate");
                }
            }
        } else {
            dynamic.commit(&mut self.dynamic_eval_bindings);
            aliases.commit(&mut self.eval_dynamic_slots);
            for row in rows.drain(..) {
                if let Some(name) = row.name {
                    self.dynamic_eval_bindings.insert(
                        name,
                        DynamicEvalBinding {
                            cell: row.cell.unwrap(),
                            shadowed_cells: row.shadowed,
                        },
                    );
                }
                if let Some(name) = row.alias {
                    self.eval_dynamic_slots.insert(row.slot, name);
                }
            }
        }
        self.stack.truncate(base);
        drop(rows);
        drop(staging_charge);
        drop(payload_charge);
        result
    }
}
