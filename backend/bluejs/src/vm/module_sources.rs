// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Immutable host sources share one document charge until their last owner drops.

use super::host_scheduling::SharedSchedulingCharge;
use super::*;

pub(super) trait SourceData: Default {
    fn buffer_bytes(&self) -> usize;
}

impl SourceData for HashMap<String, String> {
    fn buffer_bytes(&self) -> usize {
        self.iter()
            .fold(host_scheduling::map_slots(self), |bytes, (name, text)| {
                bytes
                    .saturating_add(name.capacity())
                    .saturating_add(text.capacity())
            })
    }
}

impl SourceData for HashMap<String, Vec<u8>> {
    fn buffer_bytes(&self) -> usize {
        self.iter()
            .fold(host_scheduling::map_slots(self), |bytes, (name, data)| {
                bytes
                    .saturating_add(name.capacity())
                    .saturating_add(data.capacity())
            })
    }
}

impl SourceData for HashSet<String> {
    fn buffer_bytes(&self) -> usize {
        self.iter().fold(
            self.capacity()
                .saturating_mul(std::mem::size_of::<String>()),
            |bytes, name| bytes.saturating_add(name.capacity()),
        )
    }
}

struct SourceOwner<T> {
    data: T,
    bytes: usize,
    charge: RefCell<Option<SharedSchedulingCharge>>,
}

enum SourceState<T> {
    Empty(T),
    Shared(Rc<SourceOwner<T>>),
}

pub(super) struct ModuleSources<T: SourceData>(SourceState<T>);
pub(super) type TextSources = ModuleSources<HashMap<String, String>>;
pub(super) type ByteSources = ModuleSources<HashMap<String, Vec<u8>>>;
pub(super) type SourceNames = ModuleSources<HashSet<String>>;

impl<T: SourceData> Default for ModuleSources<T> {
    fn default() -> Self {
        Self(SourceState::Empty(T::default()))
    }
}

impl<T: SourceData> Clone for ModuleSources<T> {
    fn clone(&self) -> Self {
        match &self.0 {
            SourceState::Empty(_) => Self::default(),
            SourceState::Shared(owner) => Self(SourceState::Shared(owner.clone())),
        }
    }
}

impl<T: SourceData> std::ops::Deref for ModuleSources<T> {
    type Target = T;
    fn deref(&self) -> &T {
        match &self.0 {
            SourceState::Empty(data) => data,
            SourceState::Shared(owner) => &owner.data,
        }
    }
}

impl<T: SourceData> ModuleSources<T> {
    pub(super) fn retained_bytes(&self) -> usize {
        match &self.0 {
            SourceState::Empty(_) => 0,
            SourceState::Shared(owner) => owner.bytes,
        }
    }

    fn new(data: T, scope: Option<&SchedulingScope>) -> Result<Self, RuntimeError> {
        let buffers = data.buffer_bytes();
        if buffers == 0 {
            return Ok(Self::default());
        }
        let bytes = buffers.saturating_add(std::mem::size_of::<SourceOwner<T>>());
        let charge = scope.map(|scope| scope.shared_charge(bytes)).transpose()?;
        Ok(Self(SourceState::Shared(Rc::new(SourceOwner {
            data,
            bytes,
            charge: RefCell::new(charge),
        }))))
    }

    fn attach(&self, scope: &SchedulingScope) -> Result<(), RuntimeError> {
        let SourceState::Shared(owner) = &self.0 else {
            return Ok(());
        };
        let mut charge = owner.charge.borrow_mut();
        if let Some(charge) = charge.as_ref() {
            if !charge.belongs_to(scope) {
                scope.fail();
                return Err(RuntimeError::TypeError(
                    "module sources already have a document owner".into(),
                ));
            }
        } else {
            *charge = Some(scope.shared_charge(self.retained_bytes())?);
        }
        Ok(())
    }
}

impl SourceNames {
    fn from_names(
        mut names: Vec<String>,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        // Membership has no source-order semantics. Deduplicate before
        // admission without allocating another metadata table.
        names.sort_unstable();
        names.dedup();
        if names.is_empty() {
            return Ok(Self::default());
        }
        let incoming = host_scheduling::slots(&names);
        let payload = names
            .iter()
            .fold(0usize, |bytes, name| bytes.saturating_add(name.capacity()));
        let header = std::mem::size_of::<SourceOwner<HashSet<String>>>();
        let minimum = header
            .saturating_add(incoming)
            .saturating_add(payload)
            .saturating_add(names.len().saturating_mul(std::mem::size_of::<String>()));
        let mut charge = scope
            .map(|scope| scope.shared_charge(minimum))
            .transpose()?;
        let mut data = HashSet::new();
        data.try_reserve(names.len())
            .map_err(|_| scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail))?;
        let bytes = header
            .saturating_add(
                data.capacity()
                    .saturating_mul(std::mem::size_of::<String>()),
            )
            .saturating_add(payload);
        if let Some(charge) = charge.as_mut() {
            charge.resize(bytes.saturating_add(incoming))?;
        }
        data.extend(names);
        if let Some(charge) = charge.as_mut() {
            charge.resize(bytes)?;
        }
        Ok(Self(SourceState::Shared(Rc::new(SourceOwner {
            data,
            bytes,
            charge: RefCell::new(charge),
        }))))
    }
}

impl Vm {
    pub(super) fn install_module_source_data<T: SourceData>(
        &mut self,
        data: T,
    ) -> Option<ModuleSources<T>> {
        self.reconcile_scheduling_ownership();
        ModuleSources::new(data, self.host_scheduling.as_ref()).ok()
    }

    pub(super) fn install_module_source_names(
        &mut self,
        names: Vec<String>,
    ) -> Option<SourceNames> {
        self.reconcile_scheduling_ownership();
        SourceNames::from_names(names, self.host_scheduling.as_ref()).ok()
    }

    pub(super) fn attach_module_sources(
        &self,
        scope: &SchedulingScope,
    ) -> Result<(), RuntimeError> {
        self.dynamic_module_sources.attach(scope)?;
        self.json_module_sources.attach(scope)?;
        self.text_module_sources.attach(scope)?;
        self.bytes_module_sources.attach(scope)?;
        self.module_source_registry.attach(scope)?;
        self.module_registry.attach(scope)
    }
}
