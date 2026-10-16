// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admit binding/name storage before allocating or replacing a live frame.

use super::*;

pub(super) enum FrameBindingKind {
    Script,
    Function,
    ResumedGenerator,
}

type FrameBindingParts = (Vec<Option<Value>>, Vec<Binding>, Vec<String>);

pub(super) struct PreparedFrameBindings {
    bindings: Vec<Option<Value>>,
    metadata: Vec<Binding>,
    lexicals: Vec<String>,
    _charge: execution_payload::ExecutionPayloadCharge,
}

impl PreparedFrameBindings {
    // The lease remains with this staging owner until every part is installed.
    pub(super) fn take_parts(&mut self) -> FrameBindingParts {
        (
            std::mem::take(&mut self.bindings),
            std::mem::take(&mut self.metadata),
            std::mem::take(&mut self.lexicals),
        )
    }
}

impl Vm {
    pub(super) fn prepare_frame_bindings(
        &mut self,
        code: &Bytecode,
        kind: FrameBindingKind,
    ) -> Result<PreparedFrameBindings, RuntimeError> {
        let initialize = !matches!(kind, FrameBindingKind::ResumedGenerator);
        let lexicals = !matches!(kind, FrameBindingKind::Script);
        let lexical_bindings = || {
            code.scopes
                .get(code.variable_scope as usize)
                .into_iter()
                .flatten()
                .map(|slot| &code.bindings[*slot as usize])
                .filter(|binding| binding.lexical)
        };
        let count = if initialize { code.bindings.len() } else { 0 };
        let mut bytes = count
            .saturating_mul(std::mem::size_of::<Option<Value>>())
            .saturating_add(
                code.bindings
                    .len()
                    .saturating_mul(std::mem::size_of::<Binding>()),
            );
        for binding in &code.bindings {
            bytes = bytes.saturating_add(binding.name.len());
        }
        let lexical_count = if lexicals {
            lexical_bindings().count()
        } else {
            0
        };
        bytes = bytes.saturating_add(lexical_count.saturating_mul(std::mem::size_of::<String>()));
        if lexicals {
            for binding in lexical_bindings() {
                bytes = bytes.saturating_add(binding.name.len());
            }
        }
        self.admit_scheduling_work(HostSchedulingUsage {
            payload_bytes: bytes,
            ..Default::default()
        })?;
        let bindings = vec![None; count];
        let metadata = code.bindings.clone();
        let mut names = Vec::with_capacity(lexical_count);
        if lexicals {
            names.extend(lexical_bindings().map(|binding| binding.name.clone()));
        }
        let mut actual = host_scheduling::slots(&bindings)
            .saturating_add(host_scheduling::slots(&metadata))
            .saturating_add(host_scheduling::slots(&names));
        for name in metadata
            .iter()
            .map(|binding| &binding.name)
            .chain(names.iter())
        {
            actual = actual.saturating_add(name.capacity());
        }
        let prepared = PreparedFrameBindings {
            bindings,
            metadata,
            lexicals: names,
            _charge: self.charge_execution_control(actual),
        };
        if let Err(error) = self.synchronize_scheduling_usage() {
            drop(prepared);
            self.reconcile_scheduling_ownership();
            return Err(error);
        }
        Ok(prepared)
    }
}
