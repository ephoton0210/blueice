// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral lifecycle management for page-owned BlueJS realms.
//!
//! A core or out-of-process host supplies already-authorized tab, origin, and
//! source identities. This module never opens a URL, reads a file, grants a
//! capability, or exposes DOM bindings. It gives that host one isolated VM per
//! tab, generation-bound program ownership, bounded bytecode retention, and
//! fail-closed navigation/reload invalidation.

use crate::{
    BlueJsAstNodeKind, BlueJsProgramDebugError, BlueJsProgramHandle, BlueJsProgramRegistry,
    BlueJsProgramV1, BlueJsSafePoint, BlueJsSourceIdentity, HeapError, HeapStats, RuntimeError,
    Value, Vm, VmConfig,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

/// Public identity for the first host-neutral page realm manager.
pub const BLUEJS_PAGE_RUNTIME_ABI_V1: &str = "bluejs-page-runtime-v1";

/// A caller-authorized, canonical page origin. It is intentionally opaque to
/// BlueJS: policy parsing and URL authorization remain core/host work.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlueJsPageOrigin(String);

impl BlueJsPageOrigin {
    /// Creates a non-empty origin identity without silently normalizing it.
    pub fn new(value: impl Into<String>) -> Result<Self, BlueJsPageRuntimeError> {
        let value = value.into();
        if value.is_empty() || value.contains('\0') {
            return Err(BlueJsPageRuntimeError::InvalidOrigin);
        }
        Ok(Self(value))
    }

    /// The exact host-authorized origin identity.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Limits and VM policy for every realm owned by one page runtime.
#[derive(Debug, Clone, Copy)]
pub struct BlueJsPageRuntimeConfig {
    /// VM limits copied into each newly created tab realm.
    pub vm: VmConfig,
    /// Maximum concurrently live tab realms.
    pub max_realms: usize,
    /// Maximum compiled structured programs retained by one tab realm.
    pub max_programs_per_realm: usize,
    /// Maximum retained root-bytecode bytes for one tab realm. Compilation may
    /// fail this admission check, but an over-budget program is never run.
    pub max_bytecode_bytes_per_realm: usize,
}

impl Default for BlueJsPageRuntimeConfig {
    fn default() -> Self {
        Self {
            vm: VmConfig::default(),
            max_realms: 128,
            max_programs_per_realm: 256,
            max_bytecode_bytes_per_realm: 8 * 1024 * 1024,
        }
    }
}

/// Observable per-tab resource/accounting state. This does not expose a VM,
/// object ID, source text, or program bytecode to a caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlueJsPageRealmStats {
    pub tab_id: u64,
    pub origin: BlueJsPageOrigin,
    pub program_count: usize,
    pub bytecode_bytes: usize,
    pub heap: HeapStats,
}

struct PageRealm {
    origin: BlueJsPageOrigin,
    vm: Vm,
    programs: BTreeSet<BlueJsProgramHandle>,
    module_programs: BTreeMap<String, BlueJsProgramHandle>,
    linked_module_ids: BTreeSet<String>,
    bytecode_bytes: usize,
}

/// A long-lived collection of independent tab realms and their compiled
/// structured programs. Program handles are valid only in the tab that owns
/// them and only until navigation/reload/close invalidates that realm.
pub struct BlueJsPageRuntime {
    config: BlueJsPageRuntimeConfig,
    registry: BlueJsProgramRegistry,
    realms: BTreeMap<u64, PageRealm>,
}

impl Default for BlueJsPageRuntime {
    fn default() -> Self {
        Self::new(BlueJsPageRuntimeConfig::default())
            .expect("the default BlueJS page runtime configuration is valid")
    }
}

impl BlueJsPageRuntime {
    /// Creates an empty page runtime. VM configuration is checked while a
    /// realm is created, which lets a host construct a manager before any tab
    /// exists while still receiving a defined failure at admission time.
    pub fn new(config: BlueJsPageRuntimeConfig) -> Result<Self, BlueJsPageRuntimeError> {
        if config.max_realms == 0
            || config.max_programs_per_realm == 0
            || config.max_bytecode_bytes_per_realm == 0
        {
            return Err(BlueJsPageRuntimeError::InvalidConfiguration);
        }
        Ok(Self {
            config,
            registry: BlueJsProgramRegistry::default(),
            realms: BTreeMap::new(),
        })
    }

    /// Opens one newly authorized tab realm. The caller chooses and retains
    /// the tab identifier; BlueJS never selects a default tab.
    pub fn open_realm(
        &mut self,
        tab_id: u64,
        origin: BlueJsPageOrigin,
    ) -> Result<(), BlueJsPageRuntimeError> {
        if self.realms.contains_key(&tab_id) {
            return Err(BlueJsPageRuntimeError::RealmAlreadyExists(tab_id));
        }
        if self.realms.len() >= self.config.max_realms {
            return Err(BlueJsPageRuntimeError::RealmLimit {
                limit: self.config.max_realms,
            });
        }
        let vm = Vm::new(self.config.vm).map_err(BlueJsPageRuntimeError::VmInitialization)?;
        self.realms.insert(
            tab_id,
            PageRealm {
                origin,
                vm,
                programs: BTreeSet::new(),
                module_programs: BTreeMap::new(),
                linked_module_ids: BTreeSet::new(),
                bytecode_bytes: 0,
            },
        );
        Ok(())
    }

    /// Replaces a tab's realm after an authorized navigation or reload. The
    /// new VM is created before old handles are invalidated, so a VM admission
    /// failure preserves the previous live realm without partial navigation.
    pub fn navigate(
        &mut self,
        tab_id: u64,
        origin: BlueJsPageOrigin,
    ) -> Result<(), BlueJsPageRuntimeError> {
        if !self.realms.contains_key(&tab_id) {
            return Err(BlueJsPageRuntimeError::UnknownRealm(tab_id));
        }
        let vm = Vm::new(self.config.vm).map_err(BlueJsPageRuntimeError::VmInitialization)?;
        let previous = self
            .realms
            .insert(
                tab_id,
                PageRealm {
                    origin,
                    vm,
                    programs: BTreeSet::new(),
                    module_programs: BTreeMap::new(),
                    linked_module_ids: BTreeSet::new(),
                    bytecode_bytes: 0,
                },
            )
            .expect("the checked realm exists");
        for handle in previous.programs {
            self.registry.invalidate(handle);
        }
        Ok(())
    }

    /// Closes a tab realm and invalidates every program handle it owned.
    pub fn close_realm(&mut self, tab_id: u64) -> bool {
        let Some(realm) = self.realms.remove(&tab_id) else {
            return false;
        };
        for handle in realm.programs {
            self.registry.invalidate(handle);
        }
        true
    }

    /// Compiles and retains one caller-authorized structured program. The
    /// source identity is carried unchanged into the generation registry, and
    /// an origin mismatch or resource admission failure executes nothing.
    pub fn install_program(
        &mut self,
        tab_id: u64,
        origin: &BlueJsPageOrigin,
        source: BlueJsSourceIdentity,
        program: &BlueJsProgramV1,
    ) -> Result<BlueJsProgramHandle, BlueJsPageRuntimeError> {
        let realm = self
            .realms
            .get(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?;
        if &realm.origin != origin {
            return Err(BlueJsPageRuntimeError::OriginMismatch);
        }
        if realm.programs.len() >= self.config.max_programs_per_realm {
            return Err(BlueJsPageRuntimeError::ProgramLimit {
                tab_id,
                limit: self.config.max_programs_per_realm,
            });
        }
        let module_id = matches!(program, BlueJsProgramV1::Module(_))
            .then(|| source.canonical_module_id().to_string());
        if let Some(module_id) = module_id.as_deref() {
            if realm.module_programs.contains_key(module_id)
                || realm.linked_module_ids.contains(module_id)
            {
                return Err(BlueJsPageRuntimeError::DuplicateModuleIdentity(
                    module_id.to_string(),
                ));
            }
        }
        let handle = self
            .registry
            .install(source, program)
            .map_err(BlueJsPageRuntimeError::ProgramRegistry)?;
        let bytecode_bytes = self
            .registry
            .get(handle)
            .expect("the just-installed generation is live")
            .bytecode()
            .bytes()
            .len();
        let admitted = realm
            .bytecode_bytes
            .checked_add(bytecode_bytes)
            .is_some_and(|total| total <= self.config.max_bytecode_bytes_per_realm);
        if !admitted {
            self.registry.invalidate(handle);
            return Err(BlueJsPageRuntimeError::BytecodeLimit {
                tab_id,
                limit: self.config.max_bytecode_bytes_per_realm,
            });
        }
        let realm = self
            .realms
            .get_mut(&tab_id)
            .expect("the realm remains live during one synchronous install");
        realm.programs.insert(handle);
        if let Some(module_id) = module_id {
            let previous = realm.module_programs.insert(module_id, handle);
            debug_assert!(
                previous.is_none(),
                "duplicate page module was checked before install"
            );
        }
        realm.bytecode_bytes += bytecode_bytes;
        Ok(handle)
    }

    /// Drops one installed program before it executes. The runtime verifies
    /// tab ownership, releases its root-bytecode charge, and invalidates the
    /// generation together so a failed higher-level attachment cannot leave an
    /// unpaired program runnable in a page realm.
    pub fn discard_program(
        &mut self,
        tab_id: u64,
        handle: BlueJsProgramHandle,
    ) -> Result<(), BlueJsPageRuntimeError> {
        let owned = self
            .realms
            .get(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?
            .programs
            .contains(&handle);
        if !owned {
            return Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id, handle });
        }
        let compiled = self
            .registry
            .get(handle)
            .map_err(BlueJsPageRuntimeError::ProgramRegistry)?;
        let bytecode_bytes = compiled.bytecode().bytes().len();
        let module_id = matches!(
            compiled.ast_nodes().first().map(|node| node.kind()),
            Some(BlueJsAstNodeKind::Module)
        )
        .then(|| compiled.source().canonical_module_id().to_string());
        let realm = self
            .realms
            .get_mut(&tab_id)
            .expect("the checked realm remains live during one synchronous discard");
        realm.programs.remove(&handle);
        if let Some(module_id) = module_id {
            let removed = realm.module_programs.remove(&module_id);
            debug_assert_eq!(removed, Some(handle));
        }
        realm.bytecode_bytes = realm
            .bytecode_bytes
            .checked_sub(bytecode_bytes)
            .expect("every retained handle has exactly one accounted bytecode charge");
        let invalidated = self.registry.invalidate(handle);
        debug_assert!(
            invalidated,
            "a realm-owned handle must be live in its registry"
        );
        Ok(())
    }

    /// Executes one retained script or module in its owning tab realm. The
    /// program's structured root selects classic-script or module evaluation;
    /// a handle from another tab can never be executed here.
    pub fn execute_program(
        &mut self,
        tab_id: u64,
        handle: BlueJsProgramHandle,
    ) -> Result<Value, BlueJsPageRuntimeError> {
        let realm = self
            .realms
            .get(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?;
        if !realm.programs.contains(&handle) {
            return Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id, handle });
        }
        let (root, bytecode) = {
            let compiled = self
                .registry
                .get(handle)
                .map_err(BlueJsPageRuntimeError::ProgramRegistry)?;
            let root = compiled
                .ast_nodes()
                .first()
                .map(|node| node.kind())
                .ok_or(BlueJsPageRuntimeError::ProgramShape)?;
            (root, compiled.bytecode().clone())
        };
        let realm = self
            .realms
            .get_mut(&tab_id)
            .expect("realm ownership was checked before the registry lookup");
        match root {
            BlueJsAstNodeKind::Script => realm
                .vm
                .execute_script(&bytecode)
                .map_err(BlueJsPageRuntimeError::Runtime),
            BlueJsAstNodeKind::Module => realm
                .vm
                .execute_module(&bytecode)
                .map_err(BlueJsPageRuntimeError::Runtime),
            BlueJsAstNodeKind::Statement | BlueJsAstNodeKind::Expression => {
                Err(BlueJsPageRuntimeError::ProgramShape)
            }
        }
    }

    /// Executes an already-admitted ESM module graph in one tab realm. Every
    /// handle must belong to that realm, name a module root, and have a unique
    /// canonical module identity; BlueJS never re-resolves an import specifier
    /// at this boundary.
    pub fn execute_module_graph(
        &mut self,
        tab_id: u64,
        entry: BlueJsProgramHandle,
        modules: impl IntoIterator<Item = BlueJsProgramHandle>,
    ) -> Result<Value, BlueJsPageRuntimeError> {
        let mut module_bytecode = HashMap::new();
        let mut entry_module = None;
        for handle in modules {
            let owns = self
                .realms
                .get(&tab_id)
                .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?
                .programs
                .contains(&handle);
            if !owns {
                return Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id, handle });
            }
            let compiled = self
                .registry
                .get(handle)
                .map_err(BlueJsPageRuntimeError::ProgramRegistry)?;
            if !matches!(
                compiled.ast_nodes().first().map(|node| node.kind()),
                Some(BlueJsAstNodeKind::Module)
            ) {
                return Err(BlueJsPageRuntimeError::ProgramShape);
            }
            let module_id = compiled.source().canonical_module_id().to_string();
            if module_bytecode
                .insert(module_id.clone(), compiled.bytecode().clone())
                .is_some()
            {
                return Err(BlueJsPageRuntimeError::DuplicateModuleIdentity(module_id));
            }
            if handle == entry {
                entry_module = Some(module_id);
            }
        }
        let entry_module = entry_module.ok_or(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
            tab_id,
            handle: entry,
        })?;
        let realm = self
            .realms
            .get_mut(&tab_id)
            .expect("every graph module was checked against this live realm");
        // BlueJS retains linked module cells by canonical ID. Once evaluation
        // is attempted, reserve every ID for the realm lifetime even if it
        // later fails, so a replacement artifact cannot be paired with the
        // previous graph's cells. Navigation/reload creates a fresh set.
        realm
            .linked_module_ids
            .extend(module_bytecode.keys().cloned());
        realm
            .vm
            .execute_module_graph(&entry_module, &module_bytecode)
            .map_err(BlueJsPageRuntimeError::Runtime)
    }

    /// Returns resource accounting for one live tab realm.
    pub fn realm_stats(&self, tab_id: u64) -> Result<BlueJsPageRealmStats, BlueJsPageRuntimeError> {
        let realm = self
            .realms
            .get(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?;
        Ok(BlueJsPageRealmStats {
            tab_id,
            origin: realm.origin.clone(),
            program_count: realm.programs.len(),
            bytecode_bytes: realm.bytecode_bytes,
            heap: realm.vm.heap().stats(),
        })
    }

    /// Validates a safe point only for the exact current program generation.
    pub fn validate_safe_point(
        &self,
        tab_id: u64,
        handle: BlueJsProgramHandle,
        safe_point: BlueJsSafePoint,
    ) -> Result<(), BlueJsPageRuntimeError> {
        let realm = self
            .realms
            .get(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?;
        if !realm.programs.contains(&handle) {
            return Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id, handle });
        }
        self.registry
            .validate_safe_point(handle, safe_point)
            .map_err(BlueJsPageRuntimeError::ProgramRegistry)
    }

    /// The validation-only program registry for debugger and metadata hosts.
    /// Program admission, execution, and invalidation remain owned by this
    /// page runtime rather than callers mutating the registry directly.
    pub fn program_registry(&self) -> &BlueJsProgramRegistry {
        &self.registry
    }
}

/// A page-runtime rejection. Every resource or identity failure occurs before
/// the candidate script has executed in a tab realm.
#[derive(Debug, Clone, PartialEq)]
pub enum BlueJsPageRuntimeError {
    InvalidConfiguration,
    InvalidOrigin,
    RealmAlreadyExists(u64),
    UnknownRealm(u64),
    RealmLimit {
        limit: usize,
    },
    OriginMismatch,
    ProgramLimit {
        tab_id: u64,
        limit: usize,
    },
    BytecodeLimit {
        tab_id: u64,
        limit: usize,
    },
    ProgramNotOwnedByRealm {
        tab_id: u64,
        handle: BlueJsProgramHandle,
    },
    ProgramShape,
    DuplicateModuleIdentity(String),
    VmInitialization(HeapError),
    ProgramRegistry(BlueJsProgramDebugError),
    Runtime(RuntimeError),
}

impl fmt::Display for BlueJsPageRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration => {
                formatter.write_str("invalid BlueJS page runtime configuration")
            }
            Self::InvalidOrigin => {
                formatter.write_str("page origin identity is empty or contains NUL")
            }
            Self::RealmAlreadyExists(tab_id) => {
                write!(formatter, "page realm for tab {tab_id} already exists")
            }
            Self::UnknownRealm(tab_id) => {
                write!(formatter, "page realm for tab {tab_id} is unavailable")
            }
            Self::RealmLimit { limit } => write!(formatter, "page realm limit {limit} reached"),
            Self::OriginMismatch => {
                formatter.write_str("script origin does not match its live page realm")
            }
            Self::ProgramLimit { tab_id, limit } => {
                write!(formatter, "tab {tab_id} reached program limit {limit}")
            }
            Self::BytecodeLimit { tab_id, limit } => {
                write!(formatter, "tab {tab_id} exceeds bytecode limit {limit}")
            }
            Self::ProgramNotOwnedByRealm { tab_id, .. } => {
                write!(formatter, "program is not owned by tab {tab_id}")
            }
            Self::ProgramShape => {
                formatter.write_str("compiled page program has no script or module root")
            }
            Self::DuplicateModuleIdentity(module) => {
                write!(
                    formatter,
                    "page realm already owns or linked canonical module `{module}`"
                )
            }
            Self::VmInitialization(error) => {
                write!(formatter, "cannot initialize page VM: {error}")
            }
            Self::ProgramRegistry(error) => write!(
                formatter,
                "BlueJS program registry rejected page program: {error}"
            ),
            Self::Runtime(error) => write!(formatter, "BlueJS page program failed: {error}"),
        }
    }
}

impl std::error::Error for BlueJsPageRuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::VmInitialization(error) => Some(error),
            Self::ProgramRegistry(error) => Some(error),
            Self::Runtime(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse, parse_module, BlueJsProgramV1, BlueJsSafePoint};

    fn origin() -> BlueJsPageOrigin {
        BlueJsPageOrigin::new("https://example.test").unwrap()
    }

    fn source(name: &str) -> BlueJsSourceIdentity {
        BlueJsSourceIdentity::new(name, format!("sha256:{name}")).unwrap()
    }

    #[test]
    fn executes_classic_and_module_programs_in_one_tab_realm() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        let classic = runtime
            .install_program(
                7,
                &origin(),
                source("page:///main.js"),
                &BlueJsProgramV1::Script(parse("var answer = 40 + 2; answer;").unwrap()),
            )
            .unwrap();
        assert_eq!(
            runtime.execute_program(7, classic).unwrap(),
            Value::Number(42.0)
        );

        let module = runtime
            .install_program(
                7,
                &origin(),
                source("page:///module.js"),
                &BlueJsProgramV1::Module(parse_module("export const answer = 6 * 7;").unwrap()),
            )
            .unwrap();
        assert_eq!(
            runtime.execute_program(7, module).unwrap(),
            Value::Undefined
        );
        assert_eq!(runtime.realm_stats(7).unwrap().program_count, 2);
    }

    #[test]
    fn a_linked_module_identity_cannot_be_replaced_before_navigation() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        let module_id = "page:///module.js";
        let first = runtime
            .install_program(
                7,
                &origin(),
                source(module_id),
                &BlueJsProgramV1::Module(parse_module("export const answer = 41;").unwrap()),
            )
            .unwrap();
        runtime.execute_module_graph(7, first, [first]).unwrap();
        runtime.discard_program(7, first).unwrap();

        let replacement =
            BlueJsProgramV1::Module(parse_module("export const answer = 42;").unwrap());
        assert_eq!(
            runtime.install_program(7, &origin(), source(module_id), &replacement),
            Err(BlueJsPageRuntimeError::DuplicateModuleIdentity(
                module_id.to_string()
            ))
        );

        runtime.navigate(7, origin()).unwrap();
        assert!(runtime
            .install_program(7, &origin(), source(module_id), &replacement)
            .is_ok());
    }

    #[test]
    fn navigation_invalidates_old_handles_before_the_replacement_realm_runs() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(3, origin()).unwrap();
        let handle = runtime
            .install_program(
                3,
                &origin(),
                source("page:///old.js"),
                &BlueJsProgramV1::Script(parse("1 + 2").unwrap()),
            )
            .unwrap();
        let safe_point = runtime
            .program_registry()
            .get(handle)
            .unwrap()
            .safe_points()
            .next()
            .unwrap();
        runtime.navigate(3, origin()).unwrap();
        assert_eq!(runtime.realm_stats(3).unwrap().program_count, 0);
        assert_eq!(
            runtime.execute_program(3, handle),
            Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 3, handle })
        );
        assert_eq!(
            runtime.validate_safe_point(3, handle, safe_point),
            Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 3, handle })
        );
        assert!(matches!(
            runtime.program_registry().get(handle),
            Err(BlueJsProgramDebugError::UnknownProgram)
        ));
    }

    #[test]
    fn closing_a_realm_invalidates_every_program_it_owned() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(3, origin()).unwrap();
        let handle = runtime
            .install_program(
                3,
                &origin(),
                source("page:///close.js"),
                &BlueJsProgramV1::Script(parse("1 + 2").unwrap()),
            )
            .unwrap();

        assert!(runtime.close_realm(3));
        assert!(!runtime.close_realm(3));
        assert!(matches!(
            runtime.program_registry().get(handle),
            Err(BlueJsProgramDebugError::UnknownProgram)
        ));
    }

    #[test]
    fn origin_and_bytecode_limits_fail_without_program_admission() {
        let mut runtime = BlueJsPageRuntime::new(BlueJsPageRuntimeConfig {
            max_bytecode_bytes_per_realm: 1,
            ..BlueJsPageRuntimeConfig::default()
        })
        .unwrap();
        runtime.open_realm(1, origin()).unwrap();
        let other_origin = BlueJsPageOrigin::new("https://other.test").unwrap();
        let program = BlueJsProgramV1::Script(parse("40 + 2").unwrap());
        assert_eq!(
            runtime.install_program(1, &other_origin, source("page:///wrong.js"), &program),
            Err(BlueJsPageRuntimeError::OriginMismatch)
        );
        assert_eq!(
            runtime.install_program(1, &origin(), source("page:///large.js"), &program),
            Err(BlueJsPageRuntimeError::BytecodeLimit {
                tab_id: 1,
                limit: 1
            })
        );
        assert_eq!(runtime.realm_stats(1).unwrap().program_count, 0);
    }

    #[test]
    fn discarding_a_program_releases_ownership_and_bytecode_accounting() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(1, origin()).unwrap();
        let handle = runtime
            .install_program(
                1,
                &origin(),
                source("page:///discard.js"),
                &BlueJsProgramV1::Script(parse("42").unwrap()),
            )
            .unwrap();
        assert!(runtime.realm_stats(1).unwrap().bytecode_bytes > 0);

        runtime.discard_program(1, handle).unwrap();
        assert_eq!(runtime.realm_stats(1).unwrap().program_count, 0);
        assert_eq!(runtime.realm_stats(1).unwrap().bytecode_bytes, 0);
        assert!(matches!(
            runtime.program_registry().get(handle),
            Err(BlueJsProgramDebugError::UnknownProgram)
        ));
        assert_eq!(
            runtime.execute_program(1, handle),
            Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 1, handle })
        );
    }

    #[test]
    fn a_handle_cannot_cross_between_tab_realms() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(1, origin()).unwrap();
        runtime.open_realm(2, origin()).unwrap();
        let handle = runtime
            .install_program(
                1,
                &origin(),
                source("page:///first.js"),
                &BlueJsProgramV1::Script(parse("42").unwrap()),
            )
            .unwrap();
        assert_eq!(
            runtime.execute_program(2, handle),
            Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 2, handle })
        );
    }

    #[test]
    fn safe_points_remain_exactly_generation_validated() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(1, origin()).unwrap();
        let handle = runtime
            .install_program(
                1,
                &origin(),
                source("page:///safe.js"),
                &BlueJsProgramV1::Script(parse("42").unwrap()),
            )
            .unwrap();
        let safe_point = runtime
            .program_registry()
            .get(handle)
            .unwrap()
            .safe_points()
            .next()
            .unwrap();
        runtime.validate_safe_point(1, handle, safe_point).unwrap();
        let malformed = BlueJsSafePoint {
            code_unit: safe_point.code_unit,
            bytecode_offset: safe_point.bytecode_offset + 1,
        };
        assert_eq!(
            runtime.validate_safe_point(1, handle, malformed),
            Err(BlueJsPageRuntimeError::ProgramRegistry(
                BlueJsProgramDebugError::InvalidInstructionBoundary
            ))
        );
    }
}
