// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! VM construction and public realm-execution entry points.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use crate::{Bytecode, Heap, HeapError, ImportPhase, Value};

use super::{test262_agents, RuntimeError, Vm, VmConfig};

impl Default for Vm {
    fn default() -> Self {
        Self::new(VmConfig::default()).expect("default VM configuration is valid")
    }
}

impl Vm {
    /// Creates an isolated VM and its rooted object/array prototypes.
    /// The heap budget must accommodate both prototype records.
    pub fn new(config: VmConfig) -> Result<Self, HeapError> {
        let mut heap = Heap::new(config.heap)?;
        let object_prototype = heap.alloc_object(None)?;
        // Permanent root, released with the heap. Builtin properties and
        // callable methods are installed lazily by string_intrinsics.
        heap.root(object_prototype)?;
        let array_prototype = heap.alloc_array(0, Some(object_prototype))?;
        heap.root(array_prototype)?;
        Ok(Self {
            config,
            heap,
            object_prototype,
            array_prototype,
            has_own_property_installed: false,
            property_is_enumerable_installed: false,
            string_intrinsics: None,
            typed_array_intrinsics: None,
            regexp_legacy: crate::regexp::LegacyStatics::default(),
            result_root: None,
            debugger_continuation: None,
            debugger_module_pause_request: None,
            debugger_module_continuation: None,
            debugger_nested_pause_request: None,
            debugger_nested_continuation: None,
            debugger_nested_direct_call: false,
            next_debugger_frame_serial: 1,
            stack: Vec::new(),
            bindings: Vec::new(),
            binding_metadata: Vec::new(),
            completion: Value::Undefined,
            completion_empty: true,
            active_scopes: Vec::new(),
            active_scope_slots: Vec::new(),
            with_objects: Vec::new(),
            pending_completions: Vec::new(),
            pending_tail_call: None,
            parameter_eval_env: None,
            completion_saves: Vec::new(),
            remaining_instructions: 0,
            cells: HashMap::new(),
            module_registry: HashMap::new(),
            json_module_sources: HashMap::new(),
            text_module_sources: HashMap::new(),
            bytes_module_sources: HashMap::new(),
            dynamic_module_sources: HashMap::new(),
            module_source_registry: HashSet::new(),
            module_source_cache: HashMap::new(),
            module_source_roots: HashMap::new(),
            module_import_meta: HashMap::new(),
            module_import_meta_roots: HashMap::new(),
            abstract_module_source_prototype: None,
            host_module_source_prototype: None,
            last_module_namespace: None,
            last_module_namespace_root: None,
            module_namespace_cache: HashMap::new(),
            module_namespace_roots: HashMap::new(),
            deferred_namespaces: HashMap::new(),
            module_deferred_namespace_cache: HashMap::new(),
            module_deferred_namespace_roots: HashMap::new(),
            module_graph: None,
            evaluating_linked: None,
            nested_module_roots: Vec::new(),
            deferred_import_waiters: Vec::new(),
            last_deferred_dependencies: Vec::new(),
            module_continuations: HashMap::new(),
            next_module_continuation: 0,
            async_continuations: HashMap::new(),
            next_async_continuation: 0,
            module_pending_dependencies: HashMap::new(),
            module_async_parents: HashMap::new(),
            module_import_waiters: HashMap::new(),
            active_module_name: None,
            module_closure_referrers: HashMap::new(),
            dynamic_eval_bindings: HashMap::new(),
            eval_dynamic_slots: HashMap::new(),
            dynamic_eval_outer_bindings: Vec::new(),
            this: Value::Undefined,
            arguments: Vec::new(),
            callee: Value::Undefined,
            strict: false,
            call_depth: 0,
            top_level_module: false,
            globals: HashMap::new(),
            host_functions: Vec::new(),
            host_object_factories: Vec::new(),
            host_object_methods: Vec::new(),
            host_object_pair_methods: Vec::new(),
            host_object_families: Vec::new(),
            host_click_listeners: Vec::new(),
            active_host_click_event: None,
            global_bindings: HashMap::new(),
            symbol_registry: Rc::new(RefCell::new(HashMap::new())),
            intl_legacy_constructed_symbol: None,
            script_global_slots: HashMap::new(),
            variable_scope: 0,
            variable_scope_lexicals: Vec::new(),
            iterator_prototype: None,
            regexp_iterator_prototype: None,
            templates: HashMap::new(),
            new_target: Value::Undefined,
            new_target_allowed: false,
            home_object: None,
            class_field_initializer: false,
            iterator_base: None,
            iterator_helpers_installed: Vec::new(),
            iterator_wrapper_prototype: None,
            iterator_helper_prototype: None,
            array_iterator_prototype: None,
            map_iterator_prototype: None,
            set_iterator_prototype: None,
            generator_function_prototype: None,
            generator_prototype: None,
            async_iterator_base: None,
            async_generator_prototype: None,
            async_generator_function_prototype: None,
            async_function_prototype: None,
            promise_prototype: None,
            date_prototype: None,
            map_prototype: None,
            set_prototype: None,
            weak_map_prototype: None,
            weak_set_prototype: None,
            weak_ref_prototype: None,
            finalization_registry_prototype: None,
            disposable_stack_prototype: None,
            async_disposable_stack_prototype: None,
            async_dispose_helper: None,
            disposable_stacks: HashMap::new(),
            async_disposable_stacks: HashMap::new(),
            disposables: Vec::new(),
            dispose_marks: Vec::new(),
            kept_weak_objects: Vec::new(),
            promises: HashMap::new(),
            promise_jobs: VecDeque::new(),
            test262_done: None,
            test262_agent_host: None,
            test262_agent_control: None,
            test262_async_waits: std::sync::Arc::new(test262_agents::Test262AsyncWaits::new()),
            test262_realms: HashMap::new(),
            test262_foreign_values: HashMap::new(),
            test262_foreign_buffer_mirrors: HashMap::new(),
            construct_completion_check_failed: false,
            acting_realm: None,
            test262_reverse_values: HashMap::new(),
            shadow_realm_prototype: None,
            shadow_realms: HashMap::new(),
            shadow_realm_by_heap: HashMap::new(),
            shadow_wrapped_functions: HashMap::new(),
            throw_type_error: None,
            legacy_function_getters: None,
            call_stack: Vec::new(),
            inherited_with_depth: 0,
            joining: Vec::new(),
        })
    }

    pub fn heap(&self) -> &Heap {
        &self.heap
    }

    /// Executes only bytecode produced by [`crate::compile`] with fresh
    /// bindings. A returned object and its reachable graph stay alive until
    /// the next execute (including a failing execute), or until this VM is
    /// dropped. Both success and error paths release temporary runtime roots.
    pub fn execute(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        self.execute_with_global_bindings(code, false, false)
    }

    /// Executes a classic script in this realm and publishes successful
    /// top-level `var` and function declarations on `globalThis` for a later
    /// classic script. Lexical bindings retain their script-local boundary.
    pub fn execute_script(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        self.execute_with_global_bindings(code, true, false)
    }

    /// Evaluates one dependency-free module. Its declarations are scoped to
    /// this evaluation and are never exposed as classic global properties.
    /// Use [`Vm::execute_module_graph`] when the code has import/export
    /// entries that need static linking.
    pub fn execute_module(&mut self, code: &Bytecode) -> Result<Value, RuntimeError> {
        self.execute_with_global_bindings(code, false, true)
    }

    /// Installs the bounded module-loader context used by dynamic `import()`
    /// from classic script code. The host supplies precompiled Module-goal
    /// bytecode and an opaque referrer key; BlueJS never reads module files
    /// itself.
    pub fn set_module_loader_context(
        &mut self,
        referrer: impl Into<String>,
        modules: HashMap<String, Bytecode>,
    ) {
        self.module_registry = modules;
        self.active_module_name = Some(referrer.into());
    }

    /// Installs the host's canonical names for source-phase module records.
    /// These records produce Module Source Objects and must not be supplied as
    /// executable Source Text Module bytecode.
    pub fn set_module_source_loader_context(&mut self, sources: Vec<String>) {
        self.module_source_registry = sources.into_iter().collect();
    }

    /// Installs the host's raw JSON text for `type: "json"` module requests,
    /// keyed by resolved module name (the same resolution `set_module_loader_context`'s
    /// `modules` map keys use). `ensure_synthetic_module` (`vm/modules.rs`)
    /// parses and synthesizes a Synthetic Module Record from this text the
    /// first time each resolved path is actually requested.
    pub fn set_json_module_sources(&mut self, sources: HashMap<String, String>) {
        self.json_module_sources = sources;
    }

    /// Installs the host's text for `type: "text"` module requests, keyed by
    /// resolved module name. The host has already decoded the resource as
    /// UTF-8 (import-text's HostLoadImportedModule step); the module's
    /// `default` export is exactly this string.
    pub fn set_text_module_sources(&mut self, sources: HashMap<String, String>) {
        self.text_module_sources = sources;
    }

    /// Installs the host's raw bytes for `type: "bytes"` module requests,
    /// keyed by resolved module name. The module's `default` export is a
    /// `Uint8Array` over an immutable `ArrayBuffer` holding these bytes.
    pub fn set_bytes_module_sources(&mut self, sources: HashMap<String, Vec<u8>>) {
        self.bytes_module_sources = sources;
    }

    /// Installs the host's raw JavaScript text for modules it did not
    /// pre-compile into `set_module_loader_context`'s `modules` map, keyed
    /// by resolved module name. `ensure_dynamic_module_compiled`
    /// (`vm/modules.rs`) parses and compiles this text on demand, only when
    /// a dynamic import actually resolves to a path not already present in
    /// the registry -- so a module invalid *only as a module* (but valid,
    /// and reachable only, as something a dynamic import happens to target)
    /// fails lazily as that import's own promise rejection, rather than
    /// eagerly before any code has even run.
    pub fn set_dynamic_module_sources(&mut self, sources: HashMap<String, String>) {
        self.dynamic_module_sources = sources;
    }

    /// Links and synchronously evaluates one static module graph.
    ///
    /// Keys in `modules` are host-resolved module names. Relative requests
    /// are resolved against their referrer's slash-separated key, so callers
    /// normally use canonical paths such as `directory/entry.js`. This is a
    /// deliberately synchronous subset: dynamic import and top-level await
    /// remain outside this API, but normal static cycles and live bindings
    /// use the same instantiate-before-evaluate shape as Source Text Module
    /// Records.
    pub fn execute_module_graph(
        &mut self,
        entry: &str,
        modules: &HashMap<String, Bytecode>,
    ) -> Result<Value, RuntimeError> {
        self.ensure_no_debugger_continuation()?;
        self.execute_module_graph_inner(entry, modules, true, false, ImportPhase::Evaluation)
    }
}
