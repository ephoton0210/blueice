// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module request/name producers retain independent native allocation owners.

use super::host_scheduling::SharedSchedulingCharge;
use super::promise_queue_storage::PreparedVec;
use super::*;

fn fail(scope: Option<&SchedulingScope>) -> RuntimeError {
    scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
}
fn attach(
    charge: &mut Option<SharedSchedulingCharge>,
    bytes: usize,
    scope: Option<&SchedulingScope>,
) -> Result<(), RuntimeError> {
    match (charge.as_mut(), scope) {
        (Some(owner), Some(scope)) if !owner.belongs_to(scope) => Err(scope.fail()),
        (Some(owner), Some(_)) => owner.resize(bytes),
        (Some(owner), None) => Err(owner.fail()),
        (None, Some(scope)) => {
            *charge = Some(scope.shared_charge(bytes)?);
            Ok(())
        }
        _ => Ok(()),
    }
}
fn merge(owner: &mut Option<SharedSchedulingCharge>, other: Option<SharedSchedulingCharge>) {
    if let Some(other) = other {
        if let Some(owner) = owner {
            owner
                .absorb(other)
                .expect("prepared owners share a document");
        } else {
            *owner = Some(other);
        }
    }
}

pub(super) struct ModuleName {
    name: String,
    charge: Option<SharedSchedulingCharge>,
}
impl std::ops::Deref for ModuleName {
    type Target = str;
    fn deref(&self) -> &str {
        &self.name
    }
}
impl ModuleName {
    fn buffer(bytes: usize, scope: Option<&SchedulingScope>) -> Result<Self, RuntimeError> {
        let mut value = Self {
            name: String::new(),
            charge: scope.map(|scope| scope.shared_charge(bytes)).transpose()?,
        };
        value
            .name
            .try_reserve_exact(bytes)
            .map_err(|_| fail(scope))?;
        if let Some(charge) = &mut value.charge {
            charge.resize(value.name.capacity())?;
        }
        Ok(value)
    }
    pub(super) fn copy(name: &str, scope: Option<&SchedulingScope>) -> Result<Self, RuntimeError> {
        let mut value = Self::buffer(name.len(), scope)?;
        value.name.push_str(name);
        Ok(value)
    }
    pub(super) fn resolve(
        referrer: &str,
        request: &str,
        module_type: ModuleType,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        if request.contains('\0') {
            return Err(RuntimeError::ModuleResolution(
                "a module specifier cannot contain a NUL character".into(),
            ));
        }
        let relative = request.starts_with("./") || request.starts_with("../");
        let directory = relative.then(|| referrer.rsplit_once('/')).flatten();
        let prefix = directory.map_or("", |(prefix, _)| prefix);
        let suffix = match module_type {
            ModuleType::JavaScript => "",
            ModuleType::Json => "\0json",
            ModuleType::Text => "\0text",
            ModuleType::Bytes => "\0bytes",
        };
        // One admitted work buffer handles normalization and the type suffix.
        // It covers the largest possible intermediate path; no segment Vec or
        // second qualified-key String is produced.
        let requested = prefix
            .len()
            .checked_add(request.len())
            .and_then(|bytes| bytes.checked_add(usize::from(directory.is_some())))
            .and_then(|bytes| bytes.checked_add(suffix.len()))
            .ok_or_else(|| fail(scope))?;
        let mut value = Self::buffer(requested, scope)?;
        if relative {
            value.name.push_str(prefix);
            let mut parts = directory.map_or(0, |_| prefix.split('/').count());
            for part in request.split('/') {
                match part {
                    "" | "." => {}
                    ".." => {
                        if parts == 0 {
                            return Err(RuntimeError::ModuleResolution(format!(
                                "relative module request {request} escapes its host root"
                            )));
                        }
                        parts -= 1;
                        value.name.truncate(value.name.rfind('/').unwrap_or(0));
                    }
                    part => {
                        if parts != 0 {
                            value.name.push('/');
                        }
                        value.name.push_str(part);
                        parts += 1;
                    }
                }
            }
        } else {
            value.name.push_str(request);
        }
        value.name.push_str(suffix);
        Ok(value)
    }
    pub(super) fn as_str(&self) -> &str {
        &self.name
    }
    pub(super) fn bytes(&self) -> usize {
        self.name.capacity()
    }
    /// The immediate recipient admits this already allocated input before any
    /// callback or allocation; avoid checking a transient duplicate owner.
    pub(super) fn into_unleased(self) -> String {
        let Self { name, charge } = self;
        drop(charge);
        name
    }
}

#[derive(Default)]
pub(super) struct ModuleNames {
    names: Vec<String>,
    charge: Option<SharedSchedulingCharge>,
    name_bytes: usize,
}
impl std::ops::Deref for ModuleNames {
    type Target = Vec<String>;
    fn deref(&self) -> &Vec<String> {
        &self.names
    }
}
impl ModuleNames {
    pub(super) fn bytes(&self) -> usize {
        host_scheduling::slots(&self.names).saturating_add(self.name_bytes)
    }
    pub(super) fn singleton(
        name: ModuleName,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        let mut names = Self::default();
        names.push_owned(name, scope)?;
        Ok(names)
    }
    pub(super) fn push_copy(
        &mut self,
        name: &str,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        let bytes = self.bytes();
        attach(&mut self.charge, bytes, scope)?;
        self.push_owned(ModuleName::copy(name, scope)?, scope)
    }
    pub(super) fn push_owned(
        &mut self,
        mut name: ModuleName,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        let bytes = self.bytes();
        attach(&mut self.charge, bytes, scope)?;
        let name_bytes = name.bytes();
        attach(&mut name.charge, name_bytes, scope)?;
        let growth = PreparedVec::for_push(&self.names, scope)?;
        let replaces = growth.staging_bytes() != 0;
        let candidate = growth.commit_owned(&mut self.names);
        if replaces {
            if let Some(charge) = &mut self.charge {
                charge.release_to(self.name_bytes);
            }
            merge(&mut self.charge, candidate);
        }
        self.name_bytes = self.name_bytes.saturating_add(name.name.capacity());
        let ModuleName { name, charge } = name;
        self.names.push(name);
        merge(&mut self.charge, charge);
        Ok(())
    }
    pub(super) fn retain(&mut self, keep: impl FnMut(&String) -> bool) {
        self.names.retain(keep);
        self.name_bytes = self.names.iter().map(String::capacity).sum();
        let bytes = self.bytes();
        if let Some(charge) = &mut self.charge {
            charge.release_to(bytes);
        }
    }
    pub(super) fn into_unleased(self) -> Vec<String> {
        let Self { names, charge, .. } = self;
        drop(charge);
        names
    }
}
pub(super) struct ModuleNameIterator {
    names: std::vec::IntoIter<String>,
    charge: Option<SharedSchedulingCharge>,
}
impl Iterator for ModuleNameIterator {
    type Item = ModuleName;
    fn next(&mut self) -> Option<Self::Item> {
        let name = self.names.next()?;
        let charge = self
            .charge
            .as_mut()
            .map(|charge| charge.split_off(name.capacity()));
        Some(ModuleName { name, charge })
    }
}
impl IntoIterator for ModuleNames {
    type Item = ModuleName;
    type IntoIter = ModuleNameIterator;
    fn into_iter(self) -> Self::IntoIter {
        let Self { names, charge, .. } = self;
        ModuleNameIterator {
            names: names.into_iter(),
            charge,
        }
    }
}
