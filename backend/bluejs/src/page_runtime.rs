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
    VmDebuggerNestedExecutionState, VmDebuggerStackSnapshot,
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

struct PageRealm {
    origin: BlueJsPageOrigin,
    vm: Vm,
    programs: BTreeSet<BlueJsProgramHandle>,
    module_programs: BTreeMap<String, BlueJsProgramHandle>,
    linked_module_ids: BTreeSet<String>,
    bytecode_bytes: usize,
    debugger_nested_frame: Option<BlueJsPageDebuggerFrame>,
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
                    debugger_nested_frame: None,
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
                .map_err(BlueJsPageRuntimeError::ProgramRegistry)?;
            self.registry
                .validate_safe_point(handle, safe_point)
                .map_err(BlueJsPageRuntimeError::ProgramRegistry)?;
            let root = compiled
                .ast_nodes()
                .first()
                .map(|node| node.kind())
                .ok_or(BlueJsPageRuntimeError::ProgramShape)?;
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
            .map_err(BlueJsPageRuntimeError::ProgramRegistry)?
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
            .map_err(BlueJsPageRuntimeError::ProgramRegistry)?;
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
        modules: impl IntoIterator<Item = BlueJsProgramHandle>,
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
        let (entry_module, module_bytecode) = self.module_graph_bytecode(tab_id, entry, modules)?;
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
        let (entry_module, module_bytecode) = self.module_graph_bytecode(tab_id, entry, modules)?;
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
        let (entry_module, module_bytecode) = self.module_graph_bytecode(tab_id, entry, modules)?;
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
            .map_err(BlueJsPageRuntimeError::ProgramRegistry)?;
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
            .map_err(BlueJsPageRuntimeError::ProgramRegistry)?;
        if !matches!(
            compiled.ast_nodes().first().map(|node| node.kind()),
            Some(BlueJsAstNodeKind::Module)
        ) {
            return Err(BlueJsPageRuntimeError::ProgramShape);
        }
        let entry = compiled
            .bytecode()
            .module_evaluate_entry
            .ok_or(BlueJsPageRuntimeError::ProgramShape)?;
        compiled
            .safe_points()
            .filter(|point| point.code_unit.ordinal() == 0 && point.bytecode_offset >= entry)
            .min_by_key(|point| point.bytecode_offset)
            .ok_or(BlueJsPageRuntimeError::ProgramShape)
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

    fn first_nested_point(
        runtime: &BlueJsPageRuntime,
        tab_id: u64,
        handle: BlueJsProgramHandle,
    ) -> BlueJsSafePoint {
        runtime
            .safe_points(tab_id, handle, 1024)
            .unwrap()
            .into_iter()
            .find(|point| point.code_unit.ordinal() == 1 && point.bytecode_offset == 0)
            .expect("the direct child has a first verified instruction")
    }

    #[test]
    fn page_stack_snapshot_requires_the_paused_program_and_exact_nested_frame() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        runtime.open_realm(8, origin()).unwrap();
        let program = runtime
            .install_program(
                7,
                &origin(),
                source("page:///stack.js"),
                &BlueJsProgramV1::Script(parse("function inner() { return 1; } inner();").unwrap()),
            )
            .unwrap();
        let other = runtime
            .install_program(
                7,
                &origin(),
                source("page:///other-stack.js"),
                &BlueJsProgramV1::Script(parse("1;").unwrap()),
            )
            .unwrap();
        let point = first_nested_point(&runtime, 7, program);
        let BlueJsPageDebuggerNestedExecutionState::Paused { frame, .. } = runtime
            .execute_program_until_nested_debugger_pause(7, program, point)
            .unwrap()
        else {
            panic!("child must pause");
        };
        let snapshot = runtime
            .debugger_stack_snapshot(7, program, Some(frame), 2, 256)
            .unwrap();
        assert_eq!(snapshot.program_generation, program.generation().as_u64());
        assert_eq!(snapshot.frames.len(), 2);
        assert_eq!(snapshot.frames[0].code_unit_ordinal, 1);
        assert_eq!(snapshot.frames[1].code_unit_ordinal, 0);
        assert!(runtime
            .debugger_stack_snapshot(7, program, None, 2, 256)
            .is_err());
        let mut stale = frame;
        stale.invocation_serial += 1;
        assert_eq!(
            runtime.debugger_stack_snapshot(7, program, Some(stale), 2, 256),
            Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
        );
        assert!(matches!(
            runtime.debugger_stack_snapshot(8, program, Some(frame), 2, 256),
            Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { .. })
        ));
        runtime.resume_debugger_nested_execution(frame).unwrap();
        assert!(runtime
            .debugger_stack_snapshot(7, program, Some(frame), 2, 256)
            .is_err());
        assert_eq!(
            runtime
                .debugger_stack_snapshot(7, program, None, 2, 256)
                .unwrap()
                .frames
                .len(),
            1
        );
        assert_eq!(
            runtime.debugger_stack_snapshot(7, other, None, 2, 256),
            Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
                tab_id: 7,
                handle: other,
            })
        );
        runtime.resume_debugger_execution(7).unwrap();
        assert!(runtime
            .debugger_stack_snapshot(7, program, None, 2, 256)
            .is_err());
    }

    #[test]
    fn nested_page_frame_is_exact_and_revoked_after_return_or_navigation() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        runtime.open_realm(8, origin()).unwrap();
        let program = runtime
            .install_program(
                7,
                &origin(),
                source("page:///nested.js"),
                &BlueJsProgramV1::Script(
                    parse("var calls = 0; function inner() { calls++; return 4; } inner();")
                        .unwrap(),
                ),
            )
            .unwrap();
        let point = first_nested_point(&runtime, 7, program);
        let other_program = runtime
            .install_program(
                7,
                &origin(),
                source("page:///other.js"),
                &BlueJsProgramV1::Script(parse("42").unwrap()),
            )
            .unwrap();
        let root_point = runtime
            .safe_points(7, program, 1024)
            .unwrap()
            .into_iter()
            .find(|point| point.code_unit.ordinal() == 0)
            .unwrap();
        assert_eq!(
            runtime.execute_program_until_nested_debugger_pause(7, program, root_point),
            Err(BlueJsPageRuntimeError::DebuggerNestedCodeUnitOnly)
        );
        assert!(matches!(
            runtime.execute_program_until_nested_debugger_pause(8, program, point),
            Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 8, .. })
        ));
        let BlueJsPageDebuggerNestedExecutionState::Paused { frame, .. } = runtime
            .execute_program_until_nested_debugger_pause(7, program, point)
            .unwrap()
        else {
            panic!("the child must pause");
        };
        assert_eq!(frame.tab_id(), 7);
        assert_eq!(frame.program(), program);
        assert_eq!(frame.code_unit_ordinal(), 1);
        assert_ne!(frame.invocation_serial(), 0);
        let mut other_tab = frame;
        other_tab.tab_id = 8;
        assert_eq!(
            runtime.step_debugger_nested_instruction(other_tab),
            Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
        );
        let mut other_serial = frame;
        other_serial.invocation_serial += 1;
        assert_eq!(
            runtime.step_debugger_nested_instruction(other_serial),
            Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
        );
        let mut other_generation = frame;
        other_generation.program = other_program;
        assert_eq!(
            runtime.step_debugger_nested_instruction(other_generation),
            Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
        );
        let mut returned = false;
        for _ in 0..64 {
            match runtime.step_debugger_nested_instruction(frame).unwrap() {
                BlueJsPageDebuggerNestedExecutionState::Paused {
                    frame: same_frame,
                    bytecode_offset,
                } => {
                    assert_eq!(same_frame, frame);
                    assert!(runtime
                        .safe_points(7, program, 1024)
                        .unwrap()
                        .iter()
                        .any(|candidate| candidate.code_unit.ordinal() == 1
                            && candidate.bytecode_offset == bytecode_offset));
                }
                BlueJsPageDebuggerNestedExecutionState::FrameReturned { .. } => {
                    returned = true;
                    break;
                }
                state => panic!("unexpected nested state: {state:?}"),
            }
        }
        assert!(returned);
        assert_eq!(
            runtime.step_debugger_nested_instruction(frame),
            Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
        );
        assert_eq!(
            runtime.resume_debugger_execution(7),
            Ok(BlueJsPageDebuggerExecutionState::Completed)
        );
        let BlueJsPageDebuggerNestedExecutionState::Paused {
            frame: next_frame, ..
        } = runtime
            .execute_program_until_nested_debugger_pause(7, program, point)
            .unwrap()
        else {
            panic!("the second invocation must pause");
        };
        assert_ne!(next_frame, frame);
        assert_eq!(
            runtime.step_debugger_nested_instruction(frame),
            Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
        );
        runtime.navigate(7, origin()).unwrap();
        assert_eq!(
            runtime.step_debugger_nested_instruction(next_frame),
            Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
        );
    }

    #[test]
    fn nested_page_module_frame_preserves_graph_and_rejoins_entry() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        let dependency = runtime
            .install_program(
                7,
                &origin(),
                source("page:///dep.mjs"),
                &BlueJsProgramV1::Module(
                    parse_module("globalThis.depRuns = (globalThis.depRuns || 0) + 1;").unwrap(),
                ),
            )
            .unwrap();
        let entry = runtime
            .install_program(
                7,
                &origin(),
                source("page:///entry.mjs"),
                &BlueJsProgramV1::Module(
                    parse_module(
                        "import './dep.mjs'; function inner() { return 4; } globalThis.answer = inner() + 1;",
                    )
                    .unwrap(),
                ),
            )
            .unwrap();
        let point = first_nested_point(&runtime, 7, entry);
        let BlueJsPageDebuggerNestedExecutionState::Paused { frame, .. } = runtime
            .execute_module_graph_until_nested_debugger_pause(7, entry, [dependency, entry], point)
            .unwrap()
        else {
            panic!("the module child must pause");
        };
        let snapshot = runtime
            .debugger_stack_snapshot(7, entry, Some(frame), 2, 256)
            .unwrap();
        assert_eq!(snapshot.frames.len(), 2);
        assert_eq!(snapshot.frames[0].code_unit_ordinal, 1);
        assert_eq!(snapshot.frames[1].code_unit_ordinal, 0);
        assert_eq!(snapshot.program_generation, entry.generation().as_u64());
        let mut returned = false;
        for _ in 0..64 {
            if matches!(
                runtime.step_debugger_nested_instruction(frame).unwrap(),
                BlueJsPageDebuggerNestedExecutionState::FrameReturned { .. }
            ) {
                returned = true;
                break;
            }
        }
        assert!(returned);
        let root_snapshot = runtime
            .debugger_stack_snapshot(7, entry, None, 2, 256)
            .unwrap();
        assert_eq!(root_snapshot.frames.len(), 1);
        assert_eq!(root_snapshot.frames[0].code_unit_ordinal, 0);
        assert!(!root_snapshot.stack_truncated);
        assert_eq!(
            runtime.resume_debugger_module_execution(7),
            Ok(BlueJsPageDebuggerExecutionState::Completed)
        );
        let dep_reader = runtime
            .install_program(
                7,
                &origin(),
                source("page:///dep-reader.js"),
                &BlueJsProgramV1::Script(parse("globalThis.depRuns").unwrap()),
            )
            .unwrap();
        assert_eq!(
            runtime.execute_program(7, dep_reader),
            Ok(Value::Number(1.0))
        );
        let answer_reader = runtime
            .install_program(
                7,
                &origin(),
                source("page:///answer-reader.js"),
                &BlueJsProgramV1::Script(parse("globalThis.answer").unwrap()),
            )
            .unwrap();
        assert_eq!(
            runtime.execute_program(7, answer_reader),
            Ok(Value::Number(5.0))
        );
    }

    #[test]
    fn nested_page_resume_rejoins_one_module_graph_and_revokes_exact_frame() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        runtime.open_realm(8, origin()).unwrap();
        let dependency = runtime
            .install_program(
                7,
                &origin(),
                source("page:///resume-dep.mjs"),
                &BlueJsProgramV1::Module(
                    parse_module("globalThis.depRuns = (globalThis.depRuns || 0) + 1;").unwrap(),
                ),
            )
            .unwrap();
        let entry = runtime
            .install_program(
                7,
                &origin(),
                source("page:///resume-entry.mjs"),
                &BlueJsProgramV1::Module(
                    parse_module(
                        "import './resume-dep.mjs'; function inner() { globalThis.childRuns = (globalThis.childRuns || 0) + 1; return 4; } globalThis.answer = inner() + 1;",
                    )
                    .unwrap(),
                ),
            )
            .unwrap();
        let point = first_nested_point(&runtime, 7, entry);
        let BlueJsPageDebuggerNestedExecutionState::Paused { frame, .. } = runtime
            .execute_module_graph_until_nested_debugger_pause(7, entry, [dependency, entry], point)
            .unwrap()
        else {
            panic!("module child must pause before its first instruction");
        };
        let mut wrong = frame;
        wrong.tab_id = 8;
        assert_eq!(
            runtime.resume_debugger_nested_execution(wrong),
            Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
        );
        assert!(matches!(
            runtime.resume_debugger_nested_execution(frame),
            Ok(BlueJsPageDebuggerNestedExecutionState::FrameReturned { .. })
        ));
        assert_eq!(
            runtime.resume_debugger_nested_execution(frame),
            Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
        );
        assert_eq!(
            runtime.resume_debugger_module_execution(7),
            Ok(BlueJsPageDebuggerExecutionState::Completed)
        );
        let probe = runtime
            .install_program(
                7,
                &origin(),
                source("page:///resume-probe.js"),
                &BlueJsProgramV1::Script(
                    parse("globalThis.depRuns + globalThis.childRuns + globalThis.answer").unwrap(),
                ),
            )
            .unwrap();
        assert_eq!(runtime.execute_program(7, probe), Ok(Value::Number(7.0)));
    }

    #[test]
    fn failed_nested_page_step_revokes_the_frame_without_claiming_completion() {
        let mut config = BlueJsPageRuntimeConfig::default();
        config.vm.instruction_budget = 256;
        let mut runtime = BlueJsPageRuntime::new(config).unwrap();
        runtime.open_realm(7, origin()).unwrap();
        let program = runtime
            .install_program(
                7,
                &origin(),
                source("page:///loop.js"),
                &BlueJsProgramV1::Script(
                    parse("function inner() { while (true) {} } inner();").unwrap(),
                ),
            )
            .unwrap();
        let point = first_nested_point(&runtime, 7, program);
        let BlueJsPageDebuggerNestedExecutionState::Paused { frame, .. } = runtime
            .execute_program_until_nested_debugger_pause(7, program, point)
            .unwrap()
        else {
            panic!("the looping child must pause");
        };
        let mut exhausted = false;
        for _ in 0..300 {
            match runtime.step_debugger_nested_instruction(frame) {
                Ok(BlueJsPageDebuggerNestedExecutionState::Paused { .. }) => {}
                Err(BlueJsPageRuntimeError::Runtime(RuntimeError::InstructionLimit)) => {
                    exhausted = true;
                    break;
                }
                state => panic!("unexpected nested loop state: {state:?}"),
            }
        }
        assert!(exhausted);
        assert_eq!(
            runtime.step_debugger_nested_instruction(frame),
            Err(BlueJsPageRuntimeError::DebuggerNestedFrameUnavailable)
        );
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
    fn resumes_a_non_entry_root_safe_point_without_exposing_vm_state() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        let paused_program = runtime
            .install_program(
                7,
                &origin(),
                source("page:///paused.js"),
                &BlueJsProgramV1::Script(
                    parse("globalThis.before = 1; globalThis.after = 2;").unwrap(),
                ),
            )
            .unwrap();
        let probe = runtime
            .install_program(
                7,
                &origin(),
                source("page:///probe.js"),
                &BlueJsProgramV1::Script(parse("globalThis.before + globalThis.after").unwrap()),
            )
            .unwrap();
        let safe_point = runtime
            .safe_points(7, paused_program, 128)
            .unwrap()
            .into_iter()
            .find(|safe_point| {
                safe_point.code_unit.ordinal() == 0 && safe_point.bytecode_offset != 0
            })
            .expect("fixture has a non-entry root safe point");

        assert_eq!(
            runtime
                .execute_program_until_debugger_pause(7, paused_program, safe_point)
                .unwrap(),
            BlueJsPageDebuggerExecutionState::Paused {
                bytecode_offset: safe_point.bytecode_offset
            }
        );
        assert!(matches!(
            runtime.execute_program(7, probe),
            Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Unsupported(
                "a debugger-paused root script must resume before another execution starts"
            )))
        ));
        assert_eq!(
            runtime.resume_debugger_execution(7).unwrap(),
            BlueJsPageDebuggerExecutionState::Completed
        );
        assert_eq!(
            runtime.execute_program(7, probe).unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn selects_only_a_live_module_evaluate_body_root_safe_point() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        let module = runtime
            .install_program(
                7,
                &origin(),
                source("page:///entry.js"),
                &BlueJsProgramV1::Module(parse_module("export const answer = 6 * 7;").unwrap()),
            )
            .unwrap();
        let classic = runtime
            .install_program(
                7,
                &origin(),
                source("page:///classic.js"),
                &BlueJsProgramV1::Script(parse("globalThis.answer = 42;").unwrap()),
            )
            .unwrap();
        let point = runtime.module_evaluate_entry_safe_point(7, module).unwrap();
        let entry = runtime
            .program_registry()
            .get(module)
            .unwrap()
            .bytecode()
            .module_evaluate_entry
            .unwrap();
        assert_eq!(point.code_unit.ordinal(), 0);
        assert!(point.bytecode_offset >= entry);
        assert_eq!(
            point.bytecode_offset,
            runtime
                .safe_points(7, module, 128)
                .unwrap()
                .into_iter()
                .filter(|candidate| {
                    candidate.code_unit.ordinal() == 0 && candidate.bytecode_offset >= entry
                })
                .map(|candidate| candidate.bytecode_offset)
                .min()
                .unwrap()
        );
        assert_eq!(
            runtime.module_evaluate_entry_safe_point(7, classic),
            Err(BlueJsPageRuntimeError::ProgramShape)
        );
        runtime.navigate(7, origin()).unwrap();
        assert_eq!(
            runtime.module_evaluate_entry_safe_point(7, module),
            Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
                tab_id: 7,
                handle: module,
            })
        );
    }

    #[test]
    fn steps_only_the_paused_tab_root_and_remains_generation_bound() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        runtime.open_realm(8, origin()).unwrap();
        let program = runtime
            .install_program(
                7,
                &origin(),
                source("page:///stepped.js"),
                &BlueJsProgramV1::Script(
                    parse("globalThis.stepped = (globalThis.stepped || 0) + 1;").unwrap(),
                ),
            )
            .unwrap();
        let root_offsets: Vec<_> = runtime
            .safe_points(7, program, 128)
            .unwrap()
            .into_iter()
            .filter(|point| point.code_unit.ordinal() == 0)
            .map(|point| point.bytecode_offset)
            .collect();
        assert!(root_offsets.len() > 2);
        assert_eq!(
            runtime
                .execute_program_until_debugger_pause_at_root_offset(7, program, root_offsets[0])
                .unwrap(),
            BlueJsPageDebuggerExecutionState::Paused {
                bytecode_offset: root_offsets[0]
            }
        );
        assert!(matches!(
            runtime.step_debugger_root_instruction(8),
            Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Unsupported(
                "no debugger-paused root script is available"
            )))
        ));
        assert_eq!(
            runtime.step_debugger_root_instruction(7).unwrap(),
            BlueJsPageDebuggerExecutionState::Paused {
                bytecode_offset: root_offsets[1]
            }
        );
        assert!(runtime.close_realm(7));
        runtime.open_realm(7, origin()).unwrap();
        assert!(matches!(
            runtime.step_debugger_root_instruction(7),
            Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Unsupported(
                "no debugger-paused root script is available"
            )))
        ));
    }

    #[test]
    fn close_and_reopen_discard_a_paused_root_continuation_with_its_generation() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        let paused_program = runtime
            .install_program(
                7,
                &origin(),
                source("page:///discarded.js"),
                &BlueJsProgramV1::Script(parse("globalThis.discarded = 1;").unwrap()),
            )
            .unwrap();
        let safe_point = runtime
            .safe_points(7, paused_program, 128)
            .unwrap()
            .into_iter()
            .find(|safe_point| safe_point.code_unit.ordinal() == 0)
            .unwrap();
        assert!(matches!(
            runtime.execute_program_until_debugger_pause(7, paused_program, safe_point),
            Ok(BlueJsPageDebuggerExecutionState::Paused { .. })
        ));

        assert!(runtime.close_realm(7));
        runtime.open_realm(7, origin()).unwrap();
        assert_eq!(
            runtime.resume_debugger_execution(7),
            Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Unsupported(
                "no debugger-paused root script is available"
            )))
        );
        assert_eq!(
            runtime.execute_program(7, paused_program),
            Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm {
                tab_id: 7,
                handle: paused_program
            })
        );
    }

    #[test]
    fn debugger_continuation_rejects_child_function_code_units_exactly() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        let program = runtime
            .install_program(
                7,
                &origin(),
                source("page:///nested.js"),
                &BlueJsProgramV1::Script(parse("function f() { return 1; } f();").unwrap()),
            )
            .unwrap();
        let child_safe_point = runtime
            .safe_points(7, program, 128)
            .unwrap()
            .into_iter()
            .find(|safe_point| safe_point.code_unit.ordinal() != 0)
            .expect("fixture emits a child function code unit");
        assert_eq!(
            runtime.execute_program_until_debugger_pause(7, program, child_safe_point),
            Err(BlueJsPageRuntimeError::DebuggerRootCodeUnitOnly)
        );
        assert_eq!(
            runtime.execute_program(7, program).unwrap(),
            Value::Number(1.0)
        );
    }

    #[test]
    fn host_bindings_are_realm_local_and_do_not_expose_vm_execution() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        runtime
            .configure_realm_bindings(7, |bindings| {
                let host = bindings.install_global_object("pageHost")?;
                bindings.install_method(host, "answer", 0, |_args: &[crate::HostValue]| {
                    Ok(crate::HostValue::Number(42.0))
                })
            })
            .unwrap();
        let first = runtime
            .install_program(
                7,
                &origin(),
                source("page:///first.js"),
                &BlueJsProgramV1::Script(parse("pageHost.answer();").unwrap()),
            )
            .unwrap();
        assert_eq!(
            runtime.execute_program(7, first).unwrap(),
            Value::Number(42.0)
        );

        runtime.navigate(7, origin()).unwrap();
        let replacement = runtime
            .install_program(
                7,
                &origin(),
                source("page:///replacement.js"),
                &BlueJsProgramV1::Script(parse("pageHost.answer();").unwrap()),
            )
            .unwrap();
        assert!(matches!(
            runtime.execute_program(7, replacement),
            Err(BlueJsPageRuntimeError::Runtime(RuntimeError::ReferenceError(name)))
                if name == "pageHost"
        ));
        assert_eq!(
            runtime.configure_realm_bindings(8, |_| Ok(())),
            Err(BlueJsPageRuntimeError::UnknownRealm(8))
        );
    }

    #[test]
    fn host_click_dispatch_is_realm_bound_and_old_document_listeners_expire() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        runtime.open_realm(8, origin()).unwrap();
        let mut family = None;
        runtime
            .configure_realm_bindings(7, |bindings| {
                let document = bindings.install_global_object("document")?;
                let node_family = bindings.create_host_object_family()?;
                bindings.install_host_click_event_methods(node_family)?;
                bindings.install_host_object_factory_method(
                    document,
                    "getElementById",
                    1,
                    node_family,
                    |_args: &[crate::HostValue]| Ok(Some(HostObjectKey::new(7, 1, 42))),
                )?;
                family = Some(node_family);
                Ok(())
            })
            .unwrap();
        let family = family.unwrap();
        let program = runtime
            .install_program(
                7,
                &origin(),
                source("page:///click.js"),
                &BlueJsProgramV1::Script(
                    parse("globalThis.clicks = 0; document.getElementById('x').addEventListener('click', function(event) { globalThis.clicks += 1; event.preventDefault(); });").unwrap(),
                ),
            )
            .unwrap();
        runtime.execute_program(7, program).unwrap();
        let key = HostObjectKey::new(7, 1, 42);
        assert!(runtime.dispatch_host_click(7, family, key).unwrap());
        assert!(matches!(
            runtime.dispatch_host_click(8, family, key),
            Err(BlueJsPageRuntimeError::Runtime(RuntimeError::TypeError(_)))
        ));
        runtime.navigate(7, origin()).unwrap();
        assert!(matches!(
            runtime.dispatch_host_click(7, family, key),
            Err(BlueJsPageRuntimeError::Runtime(RuntimeError::TypeError(_)))
        ));
        runtime.close_realm(7);
        assert!(matches!(
            runtime.dispatch_host_click(7, family, key),
            Err(BlueJsPageRuntimeError::UnknownRealm(7))
        ));
    }

    #[test]
    fn unconfigured_document_context_globals_are_rejected_at_runtime() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        for global in ["blueiceDocumentText", "blueiceDocumentOrigin"] {
            let program = runtime
                .install_program(
                    7,
                    &origin(),
                    source(&format!("page:///unconfigured-{global}.js")),
                    &BlueJsProgramV1::Script(parse(&format!("{global}();")).unwrap()),
                )
                .unwrap();

            assert!(matches!(
                runtime.execute_program(7, program),
                Err(BlueJsPageRuntimeError::Runtime(RuntimeError::ReferenceError(name)))
                    if name == global
            ));
        }
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
    fn module_entry_pause_retains_exact_page_generation_until_resume() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        runtime.open_realm(8, origin()).unwrap();
        let module_id = "page:///debug-entry.mjs";
        let entry = runtime
            .install_program(
                7,
                &origin(),
                source(module_id),
                &BlueJsProgramV1::Module(
                    parse_module("globalThis.moduleRuns = (globalThis.moduleRuns || 0) + 1; export const answer = 42;").unwrap(),
                ),
            )
            .unwrap();
        let point = runtime.module_evaluate_entry_safe_point(7, entry).unwrap();
        let later = runtime
            .safe_points(7, entry, 1024)
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.code_unit.ordinal() == 0 && *candidate != point)
            .unwrap();
        assert_eq!(
            runtime.execute_module_graph_until_debugger_pause(7, entry, [entry], later),
            Err(BlueJsPageRuntimeError::DebuggerModuleEntrySafePointOnly)
        );
        assert!(matches!(
            runtime.execute_module_graph_until_debugger_pause(8, entry, [entry], point),
            Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 8, .. })
        ));
        assert_eq!(
            runtime.execute_module_graph_until_debugger_pause(7, entry, [entry], point),
            Ok(BlueJsPageDebuggerExecutionState::Paused {
                bytecode_offset: point.bytecode_offset
            })
        );
        assert!(matches!(
            runtime.execute_module_graph(7, entry, [entry]),
            Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Unsupported(
                _
            )))
        ));
        assert!(matches!(
            runtime.step_debugger_module_root_instruction(8),
            Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Unsupported(
                "no debugger-paused root module is available"
            )))
        ));
        let successor = runtime.step_debugger_module_root_instruction(7).unwrap();
        let BlueJsPageDebuggerExecutionState::Paused { bytecode_offset } = successor else {
            panic!("one module-root instruction must have a verified successor");
        };
        assert!(runtime
            .safe_points(7, entry, 1024)
            .unwrap()
            .iter()
            .any(|candidate| candidate.code_unit.ordinal() == 0
                && candidate.bytecode_offset == bytecode_offset));
        assert_eq!(
            runtime.resume_debugger_module_execution(7),
            Ok(BlueJsPageDebuggerExecutionState::Completed)
        );
        runtime.execute_module_graph(7, entry, [entry]).unwrap();
        let reader = runtime
            .install_program(
                7,
                &origin(),
                source("page:///debug-reader.js"),
                &BlueJsProgramV1::Script(parse("globalThis.moduleRuns").unwrap()),
            )
            .unwrap();
        assert_eq!(runtime.execute_program(7, reader), Ok(Value::Number(1.0)));
        runtime.navigate(7, origin()).unwrap();
        assert!(matches!(
            runtime.execute_module_graph_until_debugger_pause(7, entry, [entry], point),
            Err(BlueJsPageRuntimeError::ProgramNotOwnedByRealm { tab_id: 7, .. })
        ));
        assert!(matches!(
            runtime.step_debugger_module_root_instruction(7),
            Err(BlueJsPageRuntimeError::Runtime(RuntimeError::Unsupported(
                "no debugger-paused root module is available"
            )))
        ));
    }

    #[test]
    fn a_failed_module_evaluation_still_reserves_its_identity_until_navigation() {
        let mut runtime = BlueJsPageRuntime::default();
        runtime.open_realm(7, origin()).unwrap();
        let module_id = "page:///failed-module.js";
        let failed = runtime
            .install_program(
                7,
                &origin(),
                source(module_id),
                &BlueJsProgramV1::Module(parse_module("throw 1;").unwrap()),
            )
            .unwrap();
        assert!(matches!(
            runtime.execute_module_graph(7, failed, [failed]),
            Err(BlueJsPageRuntimeError::Runtime(_))
        ));
        runtime.discard_program(7, failed).unwrap();

        let replacement =
            BlueJsProgramV1::Module(parse_module("export const answer = 42;").unwrap());
        assert_eq!(
            runtime.install_program(7, &origin(), source(module_id), &replacement),
            Err(BlueJsPageRuntimeError::DuplicateModuleIdentity(
                module_id.to_string()
            ))
        );
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
        assert_eq!(runtime.program_handles(1).unwrap(), vec![handle]);
        assert_eq!(runtime.safe_points(1, handle, 32).unwrap()[0], safe_point);
        assert_eq!(
            runtime.safe_points(1, handle, 0),
            Err(BlueJsPageRuntimeError::SafePointLimit {
                tab_id: 1,
                limit: 0,
            })
        );
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
