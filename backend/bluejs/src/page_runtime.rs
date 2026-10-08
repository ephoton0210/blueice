// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral lifecycle management for page-owned BlueJS realms.
//!
//! A core or out-of-process host supplies already-authorized tab, origin, and
//! source identities. This module never opens a URL, reads a file, grants a
//! capability, or supplies DOM bindings. It gives that host one isolated VM per
//! tab, generation-bound program ownership, bounded bytecode retention, a
//! restricted callback-binding registrar, and fail-closed navigation/reload
//! invalidation.

use crate::{
    BlueJsAstNodeKind, BlueJsProgramDebugError, BlueJsProgramHandle, BlueJsProgramRegistry,
    BlueJsProgramV1, BlueJsSafePoint, BlueJsSourceIdentity, Bytecode, HeapError, HeapStats,
    HostFunction, HostObject, HostObjectFactory, HostObjectFamily, HostObjectKey, HostObjectMethod,
    HostObjectPairMethod, RuntimeError, Value, Vm, VmConfig, VmDebuggerExecutionState,
    VmDebuggerLinkedPauseTarget, VmDebuggerNestedExecutionState, VmDebuggerScopeEntry,
    VmDebuggerStackSnapshot, VmDebuggerThrowSite, VmDebuggerValuePreview,
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

/// Source-free state returned by the bounded native-debugger root-frame
/// execution seam. It deliberately contains neither a VM value nor source or
/// bytecode data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlueJsPageDebuggerExecutionState {
    Paused { bytecode_offset: u32 },
    Completed,
}

/// One actually paused nested invocation, rather than a static code-unit
/// location. The embedding host must retain the entire identity for a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlueJsPageDebuggerFrame {
    tab_id: u64,
    program: BlueJsProgramHandle,
    code_unit_ordinal: u32,
    invocation_serial: u64,
}

/// A paused dependency invocation whose suspended caller belongs to a
/// different installed entry program. This identity is distinct from the
/// same-program nested frame and never authorizes that route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlueJsPageDebuggerLinkedFrame {
    tab_id: u64,
    entry_program: BlueJsProgramHandle,
    dependency_program: BlueJsProgramHandle,
    code_unit_ordinal: u32,
    invocation_serial: u64,
}

impl BlueJsPageDebuggerLinkedFrame {
    pub fn tab_id(self) -> u64 {
        self.tab_id
    }

    pub fn entry_program(self) -> BlueJsProgramHandle {
        self.entry_program
    }

    pub fn dependency_program(self) -> BlueJsProgramHandle {
        self.dependency_program
    }

    pub fn code_unit_ordinal(self) -> u32 {
        self.code_unit_ordinal
    }

    pub fn invocation_serial(self) -> u64 {
        self.invocation_serial
    }
}

/// One source-free, exact paused-frame lexical-slot selector. The page
/// runtime rechecks ownership and the active VM stack before returning data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlueJsPageDebuggerValueTarget {
    pub frame_index: u32,
    pub code_unit_ordinal: u32,
    pub bytecode_offset: u32,
    pub scope_entry: VmDebuggerScopeEntry,
}

impl BlueJsPageDebuggerFrame {
    pub fn tab_id(self) -> u64 {
        self.tab_id
    }

    pub fn program(self) -> BlueJsProgramHandle {
        self.program
    }

    pub fn code_unit_ordinal(self) -> u32 {
        self.code_unit_ordinal
    }

    pub fn invocation_serial(self) -> u64 {
        self.invocation_serial
    }
}

/// Source-free result of starting or stepping one nested invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlueJsPageDebuggerNestedExecutionState {
    Paused {
        frame: BlueJsPageDebuggerFrame,
        bytecode_offset: u32,
    },
    FrameReturned {
        root_bytecode_offset: u32,
    },
    Completed,
}

/// Source-free state of a cross-program dependency child and entry caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlueJsPageDebuggerLinkedExecutionState {
    Paused {
        frame: BlueJsPageDebuggerLinkedFrame,
        bytecode_offset: u32,
    },
    FrameReturned {
        root_bytecode_offset: u32,
    },
    Completed,
}

struct PageRealm {
    origin: BlueJsPageOrigin,
    vm: Vm,
    programs: BTreeSet<BlueJsProgramHandle>,
    module_programs: BTreeMap<String, BlueJsProgramHandle>,
    linked_module_ids: BTreeSet<String>,
    bytecode_bytes: usize,
    debugger_nested_frame: Option<BlueJsPageDebuggerFrame>,
    debugger_linked_frame: Option<BlueJsPageDebuggerLinkedFrame>,
}

/// A restricted, temporary view for installing host callbacks into one live
/// page realm. It intentionally exposes no VM execution, heap, source, or
/// object-inspection API, so a page host cannot bypass page-runtime program
/// admission while registering its own bindings.
pub struct BlueJsHostBindingRegistrar<'vm> {
    vm: &'vm mut Vm,
}

impl BlueJsHostBindingRegistrar<'_> {
    /// Installs one non-constructable host function as a global in this
    /// realm.
    pub fn install_global_function(
        &mut self,
        name: &str,
        length: u32,
        function: impl HostFunction,
    ) -> Result<(), RuntimeError> {
        self.vm.install_host_function(name, length, function)
    }

    /// Installs one opaque host object as a global in this realm.
    pub fn install_global_object(&mut self, name: &str) -> Result<HostObject, RuntimeError> {
        self.vm.install_host_object(name)
    }

    /// Creates a private, collector-rooted wrapper family for this realm.
    pub fn create_host_object_family(&mut self) -> Result<HostObjectFamily, RuntimeError> {
        self.vm.create_host_object_family()
    }

    /// Installs a global factory that turns child-private keys into stable JS
    /// wrapper objects. No key or VM object handle crosses to page code.
    pub fn install_global_object_factory(
        &mut self,
        name: &str,
        length: u32,
        family: HostObjectFamily,
        factory: impl HostObjectFactory,
    ) -> Result<(), RuntimeError> {
        self.vm
            .install_host_object_factory(name, length, family, factory)
    }

    /// Installs a realm-local wrapper-returning method on a host object.
    pub fn install_host_object_factory_method(
        &mut self,
        owner: HostObject,
        name: &str,
        length: u32,
        family: HostObjectFamily,
        factory: impl HostObjectFactory,
    ) -> Result<(), RuntimeError> {
        self.vm
            .install_host_object_factory_method(owner, name, length, family, factory)
    }

    /// Installs an operation whose receiver is an exact wrapper from this
    /// realm-local family. The callback receives only its private key.
    pub fn install_host_object_method(
        &mut self,
        family: HostObjectFamily,
        name: &str,
        length: u32,
        method: impl HostObjectMethod,
    ) -> Result<(), RuntimeError> {
        self.vm
            .install_host_object_method(family, name, length, method)
    }

    /// Installs an exact two-wrapper method such as `appendChild` without
    /// allowing arbitrary JavaScript objects into the host callback.
    pub fn install_host_object_pair_method(
        &mut self,
        family: HostObjectFamily,
        name: &str,
        length: u32,
        method: impl HostObjectPairMethod,
    ) -> Result<(), RuntimeError> {
        self.vm
            .install_host_object_pair_method(family, name, length, method)
    }

    /// Installs VM-owned click listener registration on exact wrappers in
    /// this realm. Callback functions are rooted inside BlueJS and never
    /// cross the primitive-only embedding callback ABI.
    pub fn install_host_click_event_methods(
        &mut self,
        family: HostObjectFamily,
    ) -> Result<(), RuntimeError> {
        self.vm.install_host_click_event_methods(family)
    }

    /// Installs a private wrapper getter/setter pair with exact receiver
    /// checks. The accessors remain in the creating realm only.
    pub fn install_host_object_accessor(
        &mut self,
        family: HostObjectFamily,
        name: &str,
        getter: impl HostObjectMethod,
        setter: impl HostObjectMethod,
    ) -> Result<(), RuntimeError> {
        self.vm
            .install_host_object_accessor(family, name, getter, setter)
    }

    /// Installs one non-constructable callback on a host object created by
    /// [`Self::install_global_object`] for this same realm.
    pub fn install_method(
        &mut self,
        owner: HostObject,
        name: &str,
        length: u32,
        function: impl HostFunction,
    ) -> Result<(), RuntimeError> {
        self.vm.install_host_method(owner, name, length, function)
    }
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
                debugger_nested_frame: None,
                debugger_linked_frame: None,
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
        // The configuration already created this tab's realm, so it is valid.
        let vm = Vm::new(self.config.vm).expect("the realm's own VM configuration is valid");
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
                    debugger_nested_frame: None,
                    debugger_linked_frame: None,
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

    /// Gives an embedding host a temporary, restricted registrar for one live
    /// realm. Bindings are scoped to the realm VM and therefore disappear on
    /// [`Self::navigate`] or [`Self::close_realm`]. The caller cannot access
    /// bytecode execution or heap operations through this API.
    pub fn configure_realm_bindings(
        &mut self,
        tab_id: u64,
        configure: impl FnOnce(&mut BlueJsHostBindingRegistrar<'_>) -> Result<(), RuntimeError>,
    ) -> Result<(), BlueJsPageRuntimeError> {
        let realm = self
            .realms
            .get_mut(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?;
        configure(&mut BlueJsHostBindingRegistrar { vm: &mut realm.vm })
            .map_err(BlueJsPageRuntimeError::HostBinding)
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
            .expect("an owned handle is live in the registry");
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
                .expect("an owned handle is live in the registry");
            let root = compiled
                .ast_nodes()
                .first()
                .map(|node| node.kind())
                .expect("a program has a root node");
            (root, compiled.bytecode().clone())
        };
        let realm = self
            .realms
            .get_mut(&tab_id)
            .expect("realm ownership was checked before the registry lookup");
        // An installed program's root is a script or a module.
        if root == BlueJsAstNodeKind::Module {
            realm
                .vm
                .execute_module(&bytecode)
                .map_err(BlueJsPageRuntimeError::Runtime)
        } else {
            realm
                .vm
                .execute_script(&bytecode)
                .map_err(BlueJsPageRuntimeError::Runtime)
        }
    }

    /// Executes one exact classic page program until a verified instruction
    /// boundary in its root code unit. This is the only page-runtime path
    /// that creates a resumable native-debugger continuation. It refuses
    /// modules and child code units rather than claiming that their frame
    /// state can be resumed by this synchronous root-frame implementation.
    pub fn execute_program_until_debugger_pause(
        &mut self,
        tab_id: u64,
        handle: BlueJsProgramHandle,
        safe_point: BlueJsSafePoint,
    ) -> Result<BlueJsPageDebuggerExecutionState, BlueJsPageRuntimeError> {
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
                .expect("an owned handle is live in the registry");
            self.registry
                .validate_safe_point(handle, safe_point)
                .map_err(BlueJsPageRuntimeError::ProgramRegistry)?;
            let root = compiled
                .ast_nodes()
                .first()
                .map(|node| node.kind())
                .expect("a program has a root node");
            (root, compiled.bytecode().clone())
        };
        if root != BlueJsAstNodeKind::Script {
            return Err(BlueJsPageRuntimeError::DebuggerRootScriptOnly);
        }
        if safe_point.code_unit.ordinal() != 0 {
            return Err(BlueJsPageRuntimeError::DebuggerRootCodeUnitOnly);
        }
        let realm = self
            .realms
            .get_mut(&tab_id)
            .expect("realm ownership was checked before the registry lookup");
        realm
            .vm
            .execute_script_until_debugger_pause(&bytecode, safe_point.bytecode_offset)
            .map(page_debugger_execution_state)
            .map_err(BlueJsPageRuntimeError::Runtime)
    }

    /// Internal-friendly form of the root continuation seam for hosts that
    /// retain only an opaque program handle and source-free byte offset. It
    /// still resolves that tuple through the generation-bound safe-point
    /// inventory before starting any script instruction.
    pub fn execute_program_until_debugger_pause_at_root_offset(
        &mut self,
        tab_id: u64,
        handle: BlueJsProgramHandle,
        bytecode_offset: u32,
    ) -> Result<BlueJsPageDebuggerExecutionState, BlueJsPageRuntimeError> {
        let realm = self
            .realms
            .get(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?;
        if !realm.programs.contains(&handle) {
            return Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id, handle });
        }
        let safe_point = self
            .registry
            .get(handle)
            .expect("an owned handle is live in the registry")
            .safe_points()
            .find(|safe_point| {
                safe_point.code_unit.ordinal() == 0 && safe_point.bytecode_offset == bytecode_offset
            })
            .ok_or(BlueJsPageRuntimeError::DebuggerRootCodeUnitOnly)?;
        self.execute_program_until_debugger_pause(tab_id, handle, safe_point)
    }

    /// Starts one classic script and pauses in its exact direct synchronous
    /// child invocation. A safe point is validated before any script work;
    /// the returned frame is minted only after the VM actually parks it.
    pub fn execute_program_until_nested_debugger_pause(
        &mut self,
        tab_id: u64,
        handle: BlueJsProgramHandle,
        safe_point: BlueJsSafePoint,
    ) -> Result<BlueJsPageDebuggerNestedExecutionState, BlueJsPageRuntimeError> {
        self.validate_safe_point(tab_id, handle, safe_point)?;
        if safe_point.code_unit.ordinal() == 0 {
            return Err(BlueJsPageRuntimeError::DebuggerNestedCodeUnitOnly);
        }
        let compiled = self
            .registry
            .get(handle)
            .expect("safe-point validation retained the live installed program");
        if !matches!(
            compiled.ast_nodes().first().map(|node| node.kind()),
            Some(BlueJsAstNodeKind::Script)
        ) {
            return Err(BlueJsPageRuntimeError::DebuggerRootScriptOnly);
        }
        let code = compiled.bytecode().clone();
        let realm = self.realms.get_mut(&tab_id).expect("validated live realm");
        let state = realm.vm.execute_script_until_nested_debugger_pause(
            &code,
            safe_point.code_unit.ordinal(),
            safe_point.bytecode_offset,
        );
        let state = state.map_err(BlueJsPageRuntimeError::Runtime)?;
        Ok(page_debugger_nested_execution_state(
            realm, tab_id, handle, state,
        ))
    }

    /// Steps only the paused invocation named by the entire live page frame
    /// identity. A code-unit safe point alone cannot authorize this action.
    pub fn step_debugger_nested_instruction(
        &mut self,
        frame: BlueJsPageDebuggerFrame,
    ) -> Result<BlueJsPageDebuggerNestedExecutionState, BlueJsPageRuntimeError> {
        self.continue_debugger_nested_execution(frame, true)
    }

    /// Completes only the live nested invocation named by this exact frame;
    /// the original classic or module root remains paused for its own resume.
    pub fn resume_debugger_nested_execution(
        &mut self,
        frame: BlueJsPageDebuggerFrame,
    ) -> Result<BlueJsPageDebuggerNestedExecutionState, BlueJsPageRuntimeError> {
        self.continue_debugger_nested_execution(frame, false)
    }

    fn continue_debugger_nested_execution(
        &mut self,
        frame: BlueJsPageDebuggerFrame,
        single_instruction: bool,
    ) -> Result<BlueJsPageDebuggerNestedExecutionState, BlueJsPageRuntimeError> {
        let realm = self
            .realms
            .get_mut(&frame.tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(frame.tab_id))?;
        if !realm.programs.contains(&frame.program) || realm.debugger_nested_frame != Some(frame) {
            return Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable);
        }
        let state = if single_instruction {
            realm
                .vm
                .step_debugger_nested_instruction(frame.invocation_serial)
        } else {
            realm
                .vm
                .resume_debugger_nested_execution(frame.invocation_serial)
        };
        if state.is_err() {
            realm.debugger_nested_frame = None;
        }
        let state = state.map_err(BlueJsPageRuntimeError::Runtime)?;
        Ok(page_debugger_nested_execution_state(
            realm,
            frame.tab_id,
            frame.program,
            state,
        ))
    }

    /// Resumes only the exact linked dependency child. The suspended entry
    /// root remains separately resumable after the child returns.
    pub fn resume_debugger_linked_nested_execution(
        &mut self,
        frame: BlueJsPageDebuggerLinkedFrame,
    ) -> Result<BlueJsPageDebuggerLinkedExecutionState, BlueJsPageRuntimeError> {
        let realm = self
            .realms
            .get_mut(&frame.tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(frame.tab_id))?;
        if realm.debugger_linked_frame != Some(frame)
            || !realm.programs.contains(&frame.entry_program)
            || !realm.programs.contains(&frame.dependency_program)
        {
            return Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable);
        }
        let state = realm
            .vm
            .resume_debugger_nested_execution(frame.invocation_serial);
        if state.is_err() {
            realm.debugger_linked_frame = None;
        }
        let state = state.map_err(BlueJsPageRuntimeError::Runtime)?;
        Ok(page_debugger_linked_execution_state(
            realm,
            frame.tab_id,
            frame.entry_program,
            frame.dependency_program,
            state,
        ))
    }

    /// Reads exactly the dependency child and suspended entry caller. Every
    /// native frame generation must match the independently owned page handle.
    pub fn debugger_linked_stack_snapshot(
        &self,
        tab_id: u64,
        frame: BlueJsPageDebuggerLinkedFrame,
        max_frames: u32,
        max_scope_entries: u32,
    ) -> Result<VmDebuggerStackSnapshot, BlueJsPageRuntimeError> {
        let realm = self
            .realms
            .get(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?;
        if frame.tab_id != tab_id
            || realm.debugger_linked_frame != Some(frame)
            || !realm.programs.contains(&frame.entry_program)
            || !realm.programs.contains(&frame.dependency_program)
        {
            return Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable);
        }
        let snapshot = realm
            .vm
            .debugger_linked_stack_snapshot(frame.invocation_serial, max_frames, max_scope_entries)
            .map_err(BlueJsPageRuntimeError::Runtime)?;
        if snapshot.program_generation != frame.entry_program.generation().as_u64()
            || snapshot.frames[0].program_generation
                != frame.dependency_program.generation().as_u64()
            || snapshot.frames[0].code_unit_ordinal != frame.code_unit_ordinal
            || snapshot.frames[1].program_generation != frame.entry_program.generation().as_u64()
        {
            return Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable);
        }
        Ok(snapshot)
    }

    /// Copies one active entry-root binding while its dependency child is
    /// paused. Both page-owned programs and the complete retained stack must
    /// still match the caller's snapshot before the native VM reads the slot.
    pub fn debugger_linked_value_preview(
        &self,
        tab_id: u64,
        frame: BlueJsPageDebuggerLinkedFrame,
        expected_stack: &VmDebuggerStackSnapshot,
        entry: VmDebuggerScopeEntry,
    ) -> Result<VmDebuggerValuePreview, BlueJsPageRuntimeError> {
        let current = self.debugger_linked_stack_snapshot(tab_id, frame, 2, 256)?;
        if &current != expected_stack {
            return Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable);
        }
        let realm = self
            .realms
            .get(&tab_id)
            .expect("the immutable snapshot lookup validated this live realm");
        realm
            .vm
            .debugger_linked_value_preview(frame.invocation_serial, &current, entry)
            .map_err(BlueJsPageRuntimeError::Runtime)
    }

    /// Copies only the retained paused root, or the exact nested child and
    /// its waiting root, in child-first order. The selected program must
    /// still belong to this tab and match the VM's installed generation.
    pub fn debugger_stack_snapshot(
        &self,
        tab_id: u64,
        program: BlueJsProgramHandle,
        frame: Option<BlueJsPageDebuggerFrame>,
        max_frames: u32,
        max_scope_entries: u32,
    ) -> Result<VmDebuggerStackSnapshot, BlueJsPageRuntimeError> {
        let realm = self
            .realms
            .get(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?;
        if !realm.programs.contains(&program) {
            return Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
                tab_id,
                handle: program,
            });
        }
        if let Some(frame) = frame {
            if frame.tab_id != tab_id
                || frame.program != program
                || realm.debugger_nested_frame != Some(frame)
            {
                return Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable);
            }
        }
        let snapshot = realm
            .vm
            .debugger_stack_snapshot(
                frame.map(|frame| frame.invocation_serial),
                max_frames,
                max_scope_entries,
            )
            .map_err(BlueJsPageRuntimeError::Runtime)?;
        if snapshot.program_generation != program.generation().as_u64() {
            return Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
                tab_id,
                handle: program,
            });
        }
        Ok(snapshot)
    }

    /// Copies only one active binding of an exact retained continuation.
    /// The native VM applies the fixed plain-data budgets without execution.
    pub fn debugger_value_preview(
        &self,
        tab_id: u64,
        program: BlueJsProgramHandle,
        frame: Option<BlueJsPageDebuggerFrame>,
        target: BlueJsPageDebuggerValueTarget,
    ) -> Result<VmDebuggerValuePreview, BlueJsPageRuntimeError> {
        // Reuse the page ownership, generation and invocation checks before
        // the native VM checks the selected safe point and active slot.
        self.debugger_stack_snapshot(tab_id, program, frame, 2, 256)?;
        let realm = self
            .realms
            .get(&tab_id)
            .expect("the immutable snapshot lookup validated this live realm");
        realm
            .vm
            .debugger_value_preview(
                frame.map(|frame| frame.invocation_serial),
                target.frame_index,
                target.code_unit_ordinal,
                target.bytecode_offset,
                target.scope_entry,
            )
            .map_err(BlueJsPageRuntimeError::Runtime)
    }

    /// Returns the most recent uncaught language throw only when its native
    /// installed generation belongs to this exact tab-owned program. This is
    /// an ephemeral source-free observation: a later realm execution can
    /// replace it, so a debugger host must snapshot it at terminal execution.
    pub fn debugger_uncaught_throw_site(
        &self,
        tab_id: u64,
        program: BlueJsProgramHandle,
    ) -> Result<Option<VmDebuggerThrowSite>, BlueJsPageRuntimeError> {
        let realm = self
            .realms
            .get(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?;
        if !realm.programs.contains(&program) {
            return Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
                tab_id,
                handle: program,
            });
        }
        Ok(realm
            .vm
            .debugger_uncaught_throw_site()
            .filter(|site| site.program_generation == program.generation().as_u64()))
    }

    /// Resumes the single root-frame debugger continuation in a tab realm.
    /// The caller does not receive a result value, bytecode, source, or VM
    /// reference; terminal errors remain the page runtime's usual category.
    pub fn resume_debugger_execution(
        &mut self,
        tab_id: u64,
    ) -> Result<BlueJsPageDebuggerExecutionState, BlueJsPageRuntimeError> {
        let realm = self
            .realms
            .get_mut(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?;
        realm
            .vm
            .resume_debugger_execution()
            .map(page_debugger_execution_state)
            .map_err(BlueJsPageRuntimeError::Runtime)
    }

    /// Advances one instruction in the paused classic root frame, then
    /// returns its actual next verified bytecode boundary or terminal state.
    /// The VM keeps the same continuation; this operation cannot inspect a
    /// nested frame or expose its operands, source, or completion value.
    pub fn step_debugger_root_instruction(
        &mut self,
        tab_id: u64,
    ) -> Result<BlueJsPageDebuggerExecutionState, BlueJsPageRuntimeError> {
        let realm = self
            .realms
            .get_mut(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?;
        realm
            .vm
            .step_debugger_root_instruction()
            .map(page_debugger_execution_state)
            .map_err(BlueJsPageRuntimeError::Runtime)
    }

    /// Delivers a host-authorized click only to a wrapper minted in this
    /// exact live tab realm, then runs a bounded microtask checkpoint before
    /// the host can apply its default action. Only the cancellation bit leaves
    /// the realm; navigation remains the embedding host's decision.
    pub fn dispatch_host_click(
        &mut self,
        tab_id: u64,
        family: HostObjectFamily,
        key: HostObjectKey,
    ) -> Result<bool, BlueJsPageRuntimeError> {
        let vm = &mut self
            .realms
            .get_mut(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?
            .vm;
        let default_prevented = vm
            .dispatch_host_click(family, key)
            .map_err(BlueJsPageRuntimeError::Runtime)?;
        self.run_click_microtask_checkpoint(tab_id)?;
        Ok(default_prevented)
    }

    /// Completes the same bounded checkpoint for a click in a realm with no
    /// event bindings. Pending jobs from its preceding script turn cannot be
    /// deferred past a core default action merely because it has no listener.
    pub fn run_click_microtask_checkpoint(
        &mut self,
        tab_id: u64,
    ) -> Result<(), BlueJsPageRuntimeError> {
        const MAX_CLICK_MICROTASK_JOBS: usize = 256;
        self.realms
            .get_mut(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?
            .vm
            .run_promise_jobs_bounded(MAX_CLICK_MICROTASK_JOBS)
            .map_err(BlueJsPageRuntimeError::Runtime)?;
        Ok(())
    }

    fn module_graph_bytecode(
        &self,
        tab_id: u64,
        entry: BlueJsProgramHandle,
        modules: &mut dyn Iterator<Item = BlueJsProgramHandle>,
    ) -> Result<(String, HashMap<String, Bytecode>), BlueJsPageRuntimeError> {
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
                .expect("an owned handle is live in the registry");
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
        Ok((entry_module, module_bytecode))
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
        let (entry_module, module_bytecode) =
            self.module_graph_bytecode(tab_id, entry, &mut modules.into_iter())?;
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

    /// Starts one exact, already-admitted ESM graph and parks its entry module
    /// before the first verified evaluate-body instruction. All graph handles
    /// and the safe point must belong to this live realm generation.
    pub fn execute_module_graph_until_debugger_pause(
        &mut self,
        tab_id: u64,
        entry: BlueJsProgramHandle,
        modules: impl IntoIterator<Item = BlueJsProgramHandle>,
        safe_point: BlueJsSafePoint,
    ) -> Result<BlueJsPageDebuggerExecutionState, BlueJsPageRuntimeError> {
        self.validate_safe_point(tab_id, entry, safe_point)?;
        if self.module_evaluate_entry_safe_point(tab_id, entry)? != safe_point {
            return Err(BlueJsPageRuntimeError::DebuggerModuleEntrySafePointOnly);
        }
        let (entry_module, module_bytecode) =
            self.module_graph_bytecode(tab_id, entry, &mut modules.into_iter())?;
        let realm = self
            .realms
            .get_mut(&tab_id)
            .expect("the graph and its entry belong to this live realm");
        realm
            .linked_module_ids
            .extend(module_bytecode.keys().cloned());
        realm
            .vm
            .execute_module_graph_until_debugger_pause(
                &entry_module,
                &module_bytecode,
                safe_point.bytecode_offset,
            )
            .map(page_debugger_execution_state)
            .map_err(BlueJsPageRuntimeError::Runtime)
    }

    /// Evaluates an admitted ESM graph until its entry module calls the
    /// selected direct synchronous child. The graph remains VM-owned on pause.
    pub fn execute_module_graph_until_nested_debugger_pause(
        &mut self,
        tab_id: u64,
        entry: BlueJsProgramHandle,
        modules: impl IntoIterator<Item = BlueJsProgramHandle>,
        safe_point: BlueJsSafePoint,
    ) -> Result<BlueJsPageDebuggerNestedExecutionState, BlueJsPageRuntimeError> {
        self.validate_safe_point(tab_id, entry, safe_point)?;
        if safe_point.code_unit.ordinal() == 0 {
            return Err(BlueJsPageRuntimeError::DebuggerNestedCodeUnitOnly);
        }
        let (entry_module, module_bytecode) =
            self.module_graph_bytecode(tab_id, entry, &mut modules.into_iter())?;
        let realm = self.realms.get_mut(&tab_id).expect("validated live realm");
        realm
            .linked_module_ids
            .extend(module_bytecode.keys().cloned());
        let state = realm.vm.execute_module_graph_until_nested_debugger_pause(
            &entry_module,
            &module_bytecode,
            safe_point.code_unit.ordinal(),
            safe_point.bytecode_offset,
        );
        let state = state.map_err(BlueJsPageRuntimeError::Runtime)?;
        Ok(page_debugger_nested_execution_state(
            realm, tab_id, entry, state,
        ))
    }

    /// Pauses an exact dependency safe point only when the entry's live,
    /// authorized module graph calls that dependency child synchronously.
    pub fn execute_module_graph_until_linked_nested_debugger_pause(
        &mut self,
        tab_id: u64,
        entry: BlueJsProgramHandle,
        dependency: BlueJsProgramHandle,
        modules: impl IntoIterator<Item = BlueJsProgramHandle>,
        safe_point: BlueJsSafePoint,
    ) -> Result<BlueJsPageDebuggerLinkedExecutionState, BlueJsPageRuntimeError> {
        if entry == dependency || safe_point.code_unit.ordinal() == 0 {
            return Err(BlueJsPageRuntimeError::DebuggerNestedCodeUnitOnly);
        }
        self.validate_safe_point(tab_id, dependency, safe_point)?;
        let dependency_module = self
            .registry
            .get(dependency)
            .expect("safe-point validation retained the live installed dependency")
            .source()
            .canonical_module_id()
            .to_string();
        let (entry_module, module_bytecode) =
            self.module_graph_bytecode(tab_id, entry, &mut modules.into_iter())?;
        if !module_bytecode.contains_key(&dependency_module) {
            return Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable);
        }
        let realm = self.realms.get_mut(&tab_id).expect("validated live realm");
        realm
            .linked_module_ids
            .extend(module_bytecode.keys().cloned());
        let state = realm
            .vm
            .execute_module_graph_until_linked_nested_debugger_pause(
                &entry_module,
                &dependency_module,
                &module_bytecode,
                VmDebuggerLinkedPauseTarget {
                    entry_generation: entry.generation().as_u64(),
                    dependency_generation: dependency.generation().as_u64(),
                    code_unit_ordinal: safe_point.code_unit.ordinal(),
                    bytecode_offset: safe_point.bytecode_offset,
                },
            );
        let state = state.map_err(BlueJsPageRuntimeError::Runtime)?;
        Ok(page_debugger_linked_execution_state(
            realm, tab_id, entry, dependency, state,
        ))
    }

    /// Preflights one linked arm against the exact live page programs and
    /// closed reachable module graph, without reserving or executing it.
    pub fn validate_linked_nested_debugger_target(
        &self,
        tab_id: u64,
        entry: BlueJsProgramHandle,
        dependency: BlueJsProgramHandle,
        modules: impl IntoIterator<Item = BlueJsProgramHandle>,
        safe_point: BlueJsSafePoint,
    ) -> Result<(), BlueJsPageRuntimeError> {
        if entry == dependency || safe_point.code_unit.ordinal() == 0 {
            return Err(BlueJsPageRuntimeError::DebuggerNestedCodeUnitOnly);
        }
        self.validate_safe_point(tab_id, dependency, safe_point)?;
        let dependency_module = self
            .registry
            .get(dependency)
            .expect("safe-point validation retained the live installed dependency")
            .source()
            .canonical_module_id()
            .to_string();
        let (entry_module, module_bytecode) =
            self.module_graph_bytecode(tab_id, entry, &mut modules.into_iter())?;
        Vm::validate_linked_nested_debugger_target(
            &entry_module,
            &dependency_module,
            &module_bytecode,
            VmDebuggerLinkedPauseTarget {
                entry_generation: entry.generation().as_u64(),
                dependency_generation: dependency.generation().as_u64(),
                code_unit_ordinal: safe_point.code_unit.ordinal(),
                bytecode_offset: safe_point.bytecode_offset,
            },
        )
        .map_err(BlueJsPageRuntimeError::Runtime)
    }

    /// Resumes the retained ESM entry frame in one exact live page realm.
    pub fn resume_debugger_module_execution(
        &mut self,
        tab_id: u64,
    ) -> Result<BlueJsPageDebuggerExecutionState, BlueJsPageRuntimeError> {
        self.realms
            .get_mut(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?
            .vm
            .resume_debugger_module_execution()
            .map(page_debugger_execution_state)
            .map_err(BlueJsPageRuntimeError::Runtime)
    }

    /// Advances one instruction in the retained module-entry root frame and
    /// returns only the actual next bytecode offset or terminal state.
    pub fn step_debugger_module_root_instruction(
        &mut self,
        tab_id: u64,
    ) -> Result<BlueJsPageDebuggerExecutionState, BlueJsPageRuntimeError> {
        self.realms
            .get_mut(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?
            .vm
            .step_debugger_module_root_instruction()
            .map(page_debugger_execution_state)
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

    /// Returns the opaque program handles currently owned by one live page
    /// realm. The returned handles carry no source, bytecode, object, or VM
    /// state; a debugger host must still validate every requested location
    /// against the exact live handle.
    pub fn program_handles(
        &self,
        tab_id: u64,
    ) -> Result<Vec<BlueJsProgramHandle>, BlueJsPageRuntimeError> {
        let realm = self
            .realms
            .get(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?;
        Ok(realm.programs.iter().copied().collect())
    }

    /// Enumerates compiler-verified instruction boundaries for one exact
    /// program owned by a live page realm. This is a debugger-location
    /// inventory, not a VM pause hook or a bytecode/source extraction API.
    pub fn safe_points(
        &self,
        tab_id: u64,
        handle: BlueJsProgramHandle,
        max_safe_points: usize,
    ) -> Result<Vec<BlueJsSafePoint>, BlueJsPageRuntimeError> {
        let realm = self
            .realms
            .get(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?;
        if !realm.programs.contains(&handle) {
            return Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id, handle });
        }
        let program = self
            .registry
            .get(handle)
            .expect("an owned handle is live in the registry");
        let mut safe_points = Vec::new();
        for safe_point in program.safe_points() {
            if safe_points.len() == max_safe_points {
                return Err(BlueJsPageRuntimeError::SafePointLimit {
                    tab_id,
                    limit: max_safe_points,
                });
            }
            safe_points.push(safe_point);
        }
        Ok(safe_points)
    }

    /// Selects the first verified root instruction in a live module's
    /// evaluation body. Declaration-instantiation instructions before the
    /// compiler's `module_evaluate_entry` are not executable pause targets.
    /// The returned point is still bound to this exact program generation.
    pub fn module_evaluate_entry_safe_point(
        &self,
        tab_id: u64,
        handle: BlueJsProgramHandle,
    ) -> Result<BlueJsSafePoint, BlueJsPageRuntimeError> {
        let realm = self
            .realms
            .get(&tab_id)
            .ok_or(BlueJsPageRuntimeError::UnknownRealm(tab_id))?;
        if !realm.programs.contains(&handle) {
            return Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id, handle });
        }
        let compiled = self
            .registry
            .get(handle)
            .expect("the realm retains ownership of this live installed program");
        if !matches!(
            compiled.ast_nodes().first().map(|node| node.kind()),
            Some(BlueJsAstNodeKind::Module)
        ) {
            return Err(BlueJsPageRuntimeError::ProgramShape);
        }
        let entry = compiled
            .bytecode()
            .module_evaluate_entry
            .expect("every module installed from its AST has an evaluation entry");
        Ok(compiled
            .safe_points()
            .filter(|point| point.code_unit.ordinal() == 0 && point.bytecode_offset >= entry)
            .min_by_key(|point| point.bytecode_offset)
            .expect("compiled module evaluation includes its terminal instruction"))
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
    SafePointLimit {
        tab_id: u64,
        limit: usize,
    },
    ProgramNotOwnedByRealm {
        tab_id: u64,
        handle: BlueJsProgramHandle,
    },
    ProgramShape,
    /// The classic-script root-frame seam rejects module programs; ESM graphs
    /// use the separate generation-bound module-entry pause operation.
    DebuggerRootScriptOnly,
    /// Nested bytecode functions still execute on the Rust call stack, so a
    /// root-frame continuation must reject their safe points exactly.
    DebuggerRootCodeUnitOnly,
    DebuggerModuleEntrySafePointOnly,
    DebuggerNestedCodeUnitOnly,
    DebuggerNestedFrameUnavailable,
    DuplicateModuleIdentity(String),
    VmInitialization(HeapError),
    HostBinding(RuntimeError),
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
            Self::SafePointLimit { tab_id, limit } => {
                write!(formatter, "tab {tab_id} exceeds safe-point limit {limit}")
            }
            Self::ProgramNotOwnedByRealm { tab_id, .. } => {
                write!(formatter, "program is not owned by tab {tab_id}")
            }
            Self::ProgramShape => {
                formatter.write_str("compiled page program has no script or module root")
            }
            Self::DebuggerRootScriptOnly => {
                formatter.write_str("native debugger continuation supports classic scripts only")
            }
            Self::DebuggerRootCodeUnitOnly => formatter
                .write_str("native debugger continuation supports root code-unit safe points only"),
            Self::DebuggerModuleEntrySafePointOnly => formatter.write_str(
                "native debugger module pause requires its first evaluate-body safe point",
            ),
            Self::DebuggerNestedCodeUnitOnly => {
                formatter.write_str("native nested debugger pause requires a child code unit")
            }
            Self::DebuggerNestedFrameUnavailable => {
                formatter.write_str("nested debugger frame is not active in this page realm")
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
            Self::HostBinding(error) => {
                write!(formatter, "cannot install page realm host binding: {error}")
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
            Self::HostBinding(error) => Some(error),
            Self::ProgramRegistry(error) => Some(error),
            Self::Runtime(error) => Some(error),
            _ => None,
        }
    }
}

fn page_debugger_execution_state(
    state: VmDebuggerExecutionState,
) -> BlueJsPageDebuggerExecutionState {
    match state {
        VmDebuggerExecutionState::Paused { bytecode_offset } => {
            BlueJsPageDebuggerExecutionState::Paused { bytecode_offset }
        }
        VmDebuggerExecutionState::Completed => BlueJsPageDebuggerExecutionState::Completed,
    }
}

fn page_debugger_nested_execution_state(
    realm: &mut PageRealm,
    tab_id: u64,
    program: BlueJsProgramHandle,
    state: VmDebuggerNestedExecutionState,
) -> BlueJsPageDebuggerNestedExecutionState {
    match state {
        VmDebuggerNestedExecutionState::Paused {
            frame_serial,
            code_unit_ordinal,
            bytecode_offset,
        } => {
            let frame = BlueJsPageDebuggerFrame {
                tab_id,
                program,
                code_unit_ordinal,
                invocation_serial: frame_serial,
            };
            debug_assert_ne!(frame_serial, 0);
            realm.debugger_nested_frame = Some(frame);
            BlueJsPageDebuggerNestedExecutionState::Paused {
                frame,
                bytecode_offset,
            }
        }
        VmDebuggerNestedExecutionState::FrameReturned {
            root_bytecode_offset,
        } => {
            realm.debugger_nested_frame = None;
            BlueJsPageDebuggerNestedExecutionState::FrameReturned {
                root_bytecode_offset,
            }
        }
        VmDebuggerNestedExecutionState::Completed => {
            realm.debugger_nested_frame = None;
            BlueJsPageDebuggerNestedExecutionState::Completed
        }
    }
}

fn page_debugger_linked_execution_state(
    realm: &mut PageRealm,
    tab_id: u64,
    entry: BlueJsProgramHandle,
    dependency: BlueJsProgramHandle,
    state: VmDebuggerNestedExecutionState,
) -> BlueJsPageDebuggerLinkedExecutionState {
    match state {
        VmDebuggerNestedExecutionState::Paused {
            frame_serial,
            code_unit_ordinal,
            bytecode_offset,
        } => {
            let frame = BlueJsPageDebuggerLinkedFrame {
                tab_id,
                entry_program: entry,
                dependency_program: dependency,
                code_unit_ordinal,
                invocation_serial: frame_serial,
            };
            debug_assert_ne!(frame_serial, 0);
            realm.debugger_linked_frame = Some(frame);
            BlueJsPageDebuggerLinkedExecutionState::Paused {
                frame,
                bytecode_offset,
            }
        }
        VmDebuggerNestedExecutionState::FrameReturned {
            root_bytecode_offset,
        } => {
            realm.debugger_linked_frame = None;
            BlueJsPageDebuggerLinkedExecutionState::FrameReturned {
                root_bytecode_offset,
            }
        }
        VmDebuggerNestedExecutionState::Completed => {
            realm.debugger_linked_frame = None;
            BlueJsPageDebuggerLinkedExecutionState::Completed
        }
    }
}

#[cfg(any(test, coverage))]
#[path = "../tests/fixtures/page_runtime_internal.rs"]
mod tests;

#[cfg(any(test, coverage))]
#[path = "../tests/fixtures/page_runtime_coverage_internal.rs"]
mod coverage_tests;
