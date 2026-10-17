// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Read-only module walks own admitted scratch and borrow canonical keys.

use super::host_scheduling::SharedSchedulingCharge;
use super::promise_queue_storage::PreparedVec;
use super::*;

fn fail(scope: Option<&SchedulingScope>) -> RuntimeError {
    scope.map_or(RuntimeError::SchedulingLimit, SchedulingScope::fail)
}
fn live(scope: Option<&SchedulingScope>) -> Result<(), RuntimeError> {
    if let Some(scope) = scope {
        scope.shared_charge(0)?;
    }
    Ok(())
}

pub(super) struct Storage<T> {
    pub(super) data: Vec<T>,
    charge: Option<SharedSchedulingCharge>,
}
impl<T> Default for Storage<T> {
    fn default() -> Self {
        Self {
            data: Vec::new(),
            charge: None,
        }
    }
}
impl<T> Storage<T> {
    pub(super) fn reserve(
        &mut self,
        additional: usize,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        let candidate = PreparedVec::for_additional(&self.data, additional, scope)?;
        let replaces = candidate.staging_bytes() != 0;
        let charge = candidate.commit_owned(&mut self.data);
        if replaces {
            self.charge = charge;
        }
        Ok(())
    }
    pub(super) fn push(
        &mut self,
        value: T,
        scope: Option<&SchedulingScope>,
    ) -> Result<(), RuntimeError> {
        self.reserve(1, scope)?;
        self.data.push(value);
        Ok(())
    }
    #[cfg(any(test, coverage))]
    pub(super) fn bytes(&self) -> usize {
        host_scheduling::slots(&self.data)
    }
}

#[derive(Default)]
pub(super) struct Visited<'a> {
    data: HashSet<&'a str>,
    charge: Option<SharedSchedulingCharge>,
}
impl<'a> Visited<'a> {
    pub(super) fn insert(
        &mut self,
        name: &'a str,
        scope: Option<&SchedulingScope>,
    ) -> Result<bool, RuntimeError> {
        if self.data.contains(name) {
            return Ok(false);
        }
        if self.data.len() == self.data.capacity() {
            let count = self.data.len().checked_add(1).ok_or_else(|| fail(scope))?;
            let mut charge = scope
                .map(|scope| scope.shared_charge(count.saturating_mul(std::mem::size_of::<&str>())))
                .transpose()?;
            let mut replacement = HashSet::new();
            replacement.try_reserve(count).map_err(|_| fail(scope))?;
            if let Some(charge) = &mut charge {
                charge.resize(
                    replacement
                        .capacity()
                        .saturating_mul(std::mem::size_of::<&str>()),
                )?;
            }
            replacement.extend(self.data.drain());
            self.data = replacement;
            self.charge = charge;
        }
        self.data.insert(name);
        Ok(true)
    }
    #[cfg(any(test, coverage))]
    fn bytes(&self) -> usize {
        self.data.capacity() * std::mem::size_of::<&str>()
    }
}

struct Frame<'a> {
    name: &'a str,
    next_request: usize,
}
enum Node<'a> {
    Known(&'a str),
    Missing(ModuleName),
}
impl Node<'_> {
    fn as_str(&self) -> &str {
        match self {
            Self::Known(name) => name,
            Self::Missing(name) => name,
        }
    }
}
struct Dfs<'a> {
    modules: &'a HashMap<String, Bytecode>,
    scope: Option<SchedulingScope>,
    visited: Visited<'a>,
    frames: Storage<Frame<'a>>,
    root: Option<&'a str>,
    eager_only: bool,
}
impl<'a> Dfs<'a> {
    fn new(
        modules: &'a HashMap<String, Bytecode>,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        live(scope)?;
        Ok(Self {
            modules,
            scope: scope.map(SchedulingScope::child),
            visited: Visited::default(),
            frames: Storage::default(),
            root: None,
            eager_only: false,
        })
    }
    fn start(&mut self, from: &str, eager_only: bool) -> bool {
        self.visited.data.clear();
        self.frames.data.clear();
        self.eager_only = eager_only;
        self.root = self
            .modules
            .get_key_value(from)
            .map(|(key, _)| key.as_str());
        self.root.is_some()
    }
    fn enter(&mut self, name: &'a str) -> Result<bool, RuntimeError> {
        if !self.visited.insert(name, self.scope.as_ref())? {
            return Ok(false);
        }
        self.frames.push(
            Frame {
                name,
                next_request: 0,
            },
            self.scope.as_ref(),
        )?;
        Ok(true)
    }
    fn prune(&mut self) {
        self.frames.data.pop();
    }
    fn next(&mut self) -> Result<Option<Node<'a>>, RuntimeError> {
        if let Some(root) = self.root.take() {
            return Ok(Some(Node::Known(root)));
        }
        while let Some(frame) = self.frames.data.last_mut() {
            let code = &self.modules[frame.name];
            let Some(request) = code.module_requests.get(frame.next_request) else {
                self.frames.data.pop();
                continue;
            };
            frame.next_request += 1;
            if self.eager_only && request.deferred {
                continue;
            }
            let target = ModuleName::resolve(
                frame.name,
                &request.module_request,
                request.module_type,
                self.scope.as_ref(),
            )?;
            return Ok(Some(self.node(target)));
        }
        Ok(None)
    }
    fn node(&self, target: ModuleName) -> Node<'a> {
        if let Some((key, _)) = self.modules.get_key_value(target.as_str()) {
            Node::Known(key.as_str())
        } else {
            Node::Missing(target)
        }
    }
    fn reaches(&mut self, from: &str, goal: &str) -> Result<bool, RuntimeError> {
        live(self.scope.as_ref())?;
        if from == goal {
            return Ok(true);
        }
        if !self.start(from, true) {
            return Err(RuntimeError::ModuleResolution(format!(
                "module {from} was not linked"
            )));
        }
        while let Some(node) = self.next()? {
            if node.as_str() == goal {
                return Ok(true);
            }
            match node {
                Node::Known(name) => {
                    self.enter(name)?;
                }
                Node::Missing(name) => {
                    return Err(RuntimeError::ModuleResolution(format!(
                        "module {} was not linked",
                        name.as_str()
                    )))
                }
            }
        }
        Ok(false)
    }
}

pub(super) struct ModuleQueries<'a> {
    walk: Dfs<'a>,
    candidates: Storage<&'a str>,
}
impl<'a> ModuleQueries<'a> {
    pub(super) fn new(
        modules: &'a HashMap<String, Bytecode>,
        scope: Option<&SchedulingScope>,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            walk: Dfs::new(modules, scope)?,
            candidates: Storage::default(),
        })
    }
    pub(super) fn reaches(&mut self, from: &str, goal: &str) -> Result<bool, RuntimeError> {
        self.walk.reaches(from, goal)
    }
    fn select(
        &mut self,
        linked: &'a HashMap<String, LinkedModule>,
        keep: impl Fn(&str, &LinkedModule) -> bool,
    ) -> Result<(), RuntimeError> {
        self.candidates.data.clear();
        let count = linked
            .iter()
            .filter(|(name, record)| keep(name, record))
            .count();
        self.candidates.reserve(count, self.walk.scope.as_ref())?;
        for (name, record) in linked {
            if keep(name, record) {
                self.candidates.data.push(name.as_str());
            }
        }
        self.candidates.data.sort_unstable();
        Ok(())
    }
    pub(super) fn scc_evaluated(
        &mut self,
        module: &str,
        linked: &'a HashMap<String, LinkedModule>,
    ) -> Result<bool, RuntimeError> {
        live(self.walk.scope.as_ref())?;
        if !linked.get(module).is_none_or(|record| record.evaluated) {
            return Ok(false);
        }
        self.select(linked, |name, record| name != module && !record.evaluated)?;
        for index in 0..self.candidates.data.len() {
            let other = self.candidates.data[index];
            if self.reaches(module, other)? && self.reaches(other, module)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    pub(super) fn cycle_error(
        &mut self,
        module: &str,
        linked: &'a HashMap<String, LinkedModule>,
    ) -> Result<Option<ModuleEvaluationError>, RuntimeError> {
        live(self.walk.scope.as_ref())?;
        self.select(linked, |name, record| {
            name != module && record.error.is_some()
        })?;
        for index in 0..self.candidates.data.len() {
            let other = self.candidates.data[index];
            if self.reaches(module, other)? && self.reaches(other, module)? {
                return Ok(linked[other].error.clone());
            }
        }
        Ok(None)
    }
    pub(super) fn frontier(
        &mut self,
        module: &str,
        linked: &'a HashMap<String, LinkedModule>,
    ) -> Result<ModuleNames, RuntimeError> {
        let mut result = ModuleNames::default();
        let mut outer = Dfs::new(self.walk.modules, self.walk.scope.as_ref())?;
        if !outer.start(module, false) {
            return Ok(result);
        }
        while let Some(node) = outer.next()? {
            let Node::Known(name) = node else {
                continue;
            };
            if !outer.enter(name)? {
                continue;
            }
            let Some(record) = linked.get(name) else {
                outer.prune();
                continue;
            };
            if (record.evaluating && !record.suspended) || self.scc_evaluated(name, linked)? {
                outer.prune();
                continue;
            }
            if Vm::module_has_tla(&outer.modules[name]) {
                result.push_copy(name, outer.scope.as_ref())?;
                outer.prune();
            }
        }
        Ok(result)
    }
    pub(super) fn ready(
        &mut self,
        module: &str,
        linked: &'a HashMap<String, LinkedModule>,
    ) -> Result<bool, RuntimeError> {
        let mut outer = Dfs::new(self.walk.modules, self.walk.scope.as_ref())?;
        if !outer.start(module, false) {
            return Ok(true);
        }
        while let Some(node) = outer.next()? {
            let Node::Known(name) = node else {
                continue;
            };
            if !outer.enter(name)? {
                continue;
            }
            let Some(record) = linked.get(name) else {
                outer.prune();
                continue;
            };
            if self.scc_evaluated(name, linked)? {
                outer.prune();
                continue;
            }
            if record.evaluating || Vm::module_has_tla(&outer.modules[name]) {
                return Ok(false);
            }
        }
        Ok(true)
    }
    pub(super) fn check_loaded(&mut self, module: &str) -> Result<(), RuntimeError> {
        live(self.walk.scope.as_ref())?;
        self.walk.visited.data.clear();
        self.walk.frames.data.clear();
        let Some((key, _)) = self.walk.modules.get_key_value(module) else {
            return Err(RuntimeError::ModuleResolution(format!(
                "module {module} was not supplied by the host"
            )));
        };
        let mut pending = Storage::default();
        pending.push(Node::Known(key.as_str()), self.walk.scope.as_ref())?;
        while let Some(node) = pending.data.pop() {
            let Node::Known(name) = node else {
                return Err(RuntimeError::ModuleResolution(format!(
                    "module {} was not supplied by the host",
                    node.as_str()
                )));
            };
            if !self.walk.visited.insert(name, self.walk.scope.as_ref())? {
                continue;
            }
            for request in &self.walk.modules[name].module_requests {
                let target = ModuleName::resolve(
                    name,
                    &request.module_request,
                    request.module_type,
                    self.walk.scope.as_ref(),
                )?;
                pending.push(self.walk.node(target), self.walk.scope.as_ref())?;
            }
        }
        Ok(())
    }
    #[cfg(any(test, coverage))]
    pub(super) fn bytes(&self) -> usize {
        self.walk.visited.bytes() + self.walk.frames.bytes() + self.candidates.bytes()
    }
    #[cfg(any(test, coverage))]
    pub(super) fn visited(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.walk.visited.data.iter().copied()
    }
    #[cfg(any(test, coverage))]
    pub(super) fn frame_bytes(&self) -> usize {
        self.walk.frames.bytes()
    }
}
