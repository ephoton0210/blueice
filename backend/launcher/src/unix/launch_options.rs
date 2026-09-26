// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl CoreLaunchOptions {
    /// Forwards an owner-selected gatekeeper endpoint to the trusted core.
    /// This is intentionally unrelated to the page-host capability.
    pub fn with_gatekeeper_socket(mut self, path: PathBuf) -> Self {
        self.gatekeeper_socket = Some(path);
        self
    }

    /// Selects the launcher-owned, out-of-process BlueJS page-host route.
    ///
    /// This remains disabled by default. The launcher creates the private
    /// socket and fresh token itself; callers cannot configure either
    /// value through this API.
    pub fn supervise_out_of_process_bluejs(mut self) -> Self {
        self.supervise_out_of_process_bluejs = true;
        self
    }

    /// Starts an isolated page host with the embedding owner's fixed
    /// per-realm runtime limits. They bound realm/program/root-bytecode
    /// admission and VM managed heap, but are not a child-process RSS or
    /// aggregate memory limit. The ordinary `blueice-launcher` CLI has no
    /// equivalent flags, and neither core nor page traffic can widen them.
    pub fn supervise_out_of_process_bluejs_with_runtime_limits(
        mut self,
        limits: BlueJsHostRuntimeLimits,
    ) -> Self {
        self.supervise_out_of_process_bluejs = true;
        self.bluejs_host_runtime_limits = limits;
        self
    }

    /// Starts the launcher-supervised page host with core's sole fixed
    /// HTTP page-script integration fixture.
    ///
    /// The public `blueice-launcher` CLI deliberately has no equivalent
    /// switch. This trusted embedding API selects only a compiled profile
    /// identity; its resource path, integrity manifest, origin relation,
    /// resolver policy, byte limits, and fetch behavior remain inside the
    /// core binary and cannot be caller supplied.
    pub fn supervise_out_of_process_bluejs_with_core_http_fixture(mut self) -> Self {
        self.supervise_out_of_process_bluejs = true;
        self.core_http_page_script_fixture = true;
        self
    }

    /// Opts a supervised child into the narrow, untyped JavaScript DOM
    /// lookup proof callback. This is a trusted embedding test profile,
    /// unavailable to page markup, frontend IPC, and the launcher CLI.
    pub fn supervise_out_of_process_bluejs_with_dom_lookup_probe_fixture(mut self) -> Self {
        self.supervise_out_of_process_bluejs = true;
        self.core_dom_lookup_probe_fixture = true;
        self
    }

    /// Selects the bounded live-DOM text profile for this supervised
    /// child. Its exact BlueTS method/accessor typings and runtime
    /// inventory are owner-selected; pages cannot opt in themselves.
    pub fn supervise_out_of_process_bluejs_with_dom_text_fixture(mut self) -> Self {
        self.supervise_out_of_process_bluejs = true;
        self.core_dom_text_fixture = true;
        self
    }

    /// Selects the exact bounded DOM mutation profile for one supervised
    /// child. Page, frontend, and public launcher traffic cannot enable it.
    pub fn supervise_out_of_process_bluejs_with_dom_mutation_fixture(mut self) -> Self {
        self.supervise_out_of_process_bluejs = true;
        self.core_dom_mutation_fixture = true;
        self
    }

    pub fn supervise_out_of_process_bluejs_with_dom_event_fixture(mut self) -> Self {
        self.supervise_out_of_process_bluejs = true;
        self.core_dom_event_fixture = true;
        self
    }

    /// Opts this core generation into the fixed, closed compiler project
    /// profile and selects the Unix endpoint that its query-only compiler
    /// listener will own.
    ///
    /// The endpoint is the only caller-provided compiler value.  The
    /// launcher always passes the compiled-in
    /// `core-closed-fixture-v1` profile to `blueice-core`; this method
    /// cannot carry project paths, sources, resolver/compiler options,
    /// update/build/write authority, or a caller-selected profile.  The
    /// endpoint is validated before the launcher creates any child.
    ///
    /// The public endpoint is owned by the launcher, rather than a core
    /// generation.  It relays each accepted connection to exactly one
    /// generation-private compiler socket.  That lets a cutover stage a
    /// verified v2 listener before it changes the public route, without
    /// ever retargeting an existing compiler/MCP connection.
    pub fn with_core_closed_compiler_mcp_endpoint(mut self, path: PathBuf) -> Self {
        self.compiler_mcp_socket = Some(path);
        self.compiler_catalog = None;
        self
    }

    /// Supplies a complete, owner-authorized sealed catalog for startup.
    /// The compiler socket remains query-only; this never enables dynamic
    /// registration, filesystem lookup, build, or write operations.
    pub fn with_owner_compiler_catalog_mcp_endpoint(
        mut self,
        path: PathBuf,
        catalog: CompilerCatalogBootstrap,
    ) -> io::Result<Self> {
        catalog.validate()?;
        self.compiler_mcp_socket = Some(path);
        self.compiler_catalog = Some(catalog);
        Ok(self)
    }

    /// Installs a bounded owner-selected HTTP(S) resource policy for the
    /// supervised page host. Core applies its existing canonical URL,
    /// origin, integrity, MIME, graph, and fetch checks before the child
    /// receives any source. The policy cannot be changed by page traffic.
    pub fn supervise_out_of_process_bluejs_with_owner_http_policy(
        mut self,
        policy: OwnerHttpPolicyBootstrap,
    ) -> io::Result<Self> {
        policy.validate()?;
        self.supervise_out_of_process_bluejs = true;
        self.page_http_policy = Some(policy);
        Ok(self)
    }

    /// Selects the launcher-owned stable endpoint for the core debugger
    /// protocol.
    ///
    /// The launcher binds the supplied public path as owner-only (`0600`)
    /// and relays each accepted stream to exactly one core generation's
    /// private listener.  It does not add debugger operations, source,
    /// bytecode, runtime values, or arbitrary child-host authority.
    /// Replacement cores receive fresh private sockets; live streams are
    /// never retargeted across a cutover.
    pub fn with_debugger_endpoint(mut self, path: PathBuf) -> Self {
        self.debugger_socket = Some(path);
        self
    }

    /// Carries the default-denied bounded-value policy to this core
    /// generation. It does not itself grant a debugger client any read.
    pub fn with_debugger_bounded_values(mut self) -> Self {
        self.debugger_bounded_values = true;
        self
    }

    /// Enables the bounded opaque static-metadata inventory for this
    /// launch profile. A debugger endpoint must also be selected before
    /// this policy can take effect. It never enables source, type, symbol,
    /// span, contract, bytecode, or runtime-value access.
    pub fn with_debugger_static_metadata_inventory(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self
    }

    /// Enables bounded source-free summaries for handles from the
    /// explicitly selected static-metadata inventory. The method also
    /// enables that prerequisite inventory, but a client must still
    /// negotiate both distinct capabilities on its debugger stream.
    pub fn with_debugger_static_metadata_summary(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_summary = true;
        self
    }

    /// Enables only compiler-minted source-record IDs for an exact
    /// static-metadata handle. The parent inventory remains the required
    /// opaque authority; individual source/provenance detail is absent.
    pub fn with_debugger_static_metadata_source_inventory(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_source_inventory = true;
        self
    }

    /// Enables the distinct source-provenance disclosure policy together
    /// with its required opaque parent and source-ID inventory. A debugger
    /// peer must still negotiate all canonical capabilities on its stream.
    pub fn with_debugger_static_metadata_source_provenance(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_source_inventory = true;
        self.debugger_static_metadata_source_provenance = true;
        self
    }

    /// Enables only compiler-minted type-record IDs for one exact static
    /// metadata handle. The parent inventory remains the required opaque
    /// authority; type displays and records are not exposed.
    pub fn with_debugger_static_metadata_type_inventory(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_type_inventory = true;
        self
    }

    /// Enables one bounded compiler-produced type display for a type ID
    /// returned by the exact debugger stream's type inventory. This also
    /// selects the necessary opaque parent and type-ID inventory policy;
    /// a client must still negotiate all three capabilities.
    pub fn with_debugger_static_metadata_type_display(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_type_inventory = true;
        self.debugger_static_metadata_type_display = true;
        self
    }

    /// Enables compiler-minted symbol-record IDs for one exact metadata
    /// handle. This also selects the required opaque parent inventory,
    /// while symbol names, spans, types, and record reads remain denied.
    pub fn with_debugger_static_metadata_symbol_inventory(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_symbol_inventory = true;
        self
    }

    /// Enables compiler-minted contract IDs for one exact metadata handle.
    /// This also selects the required opaque parent inventory, while
    /// contract names, spans, plans, and validation remain denied.
    pub fn with_debugger_static_metadata_contract_inventory(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_contract_inventory = true;
        self
    }

    /// Enables a bounded compiler-produced display for an already
    /// inventoried contract ID. This also selects parent and contract
    /// inventories, while source spans, plans, validation, and records stay denied.
    pub fn with_debugger_static_metadata_contract_display(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_contract_inventory = true;
        self.debugger_static_metadata_contract_display = true;
        self
    }

    /// Enables a bounded data-only validation for an already inventoried
    /// contract ID. This also selects parent and contract inventories, but
    /// only exposes a boolean outcome—never the plan or failure detail.
    pub fn with_debugger_static_metadata_contract_validation(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_contract_inventory = true;
        self.debugger_static_metadata_contract_validation = true;
        self
    }

    /// Enables an aggregate verified direct-lowering-map summary for a
    /// prior opaque metadata handle. This selects only the parent
    /// inventory; source spans, map entries, and bytecode stay denied.
    pub fn with_debugger_static_metadata_lowering_summary(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_lowering_summary = true;
        self
    }

    /// Enables a bounded compiler-produced display for an already
    /// inventoried symbol ID. This also selects parent and symbol
    /// inventories, while source spans, types, contracts, and records stay denied.
    pub fn with_debugger_static_metadata_symbol_display(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_symbol_inventory = true;
        self.debugger_static_metadata_symbol_display = true;
        self
    }

    /// Enables one bounded half-open UTF-8 byte range for an exact
    /// separately inventoried symbol/source pair. This selects parent,
    /// source-ID, and symbol-ID inventories; it never enables source
    /// text, module identity, line/column mappings, metadata records,
    /// names, types, contracts, bytecode, or runtime values.
    pub fn with_debugger_static_metadata_symbol_location(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_source_inventory = true;
        self.debugger_static_metadata_symbol_inventory = true;
        self.debugger_static_metadata_symbol_location = true;
        self
    }

    /// Enables only exact original BlueTS safe-point spans and their
    /// required opaque parent/source inventories. The debugger stream
    /// must negotiate this distinct grant and receive the source ID.
    pub fn with_debugger_static_metadata_safe_point_span(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_source_inventory = true;
        self.debugger_static_metadata_safe_point_span = true;
        self
    }

    pub fn with_debugger_static_metadata_source_breakpoint(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_source_inventory = true;
        self.debugger_static_metadata_source_breakpoint = true;
        self
    }

    pub fn with_debugger_static_metadata_source_span_step(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_source_inventory = true;
        self.debugger_static_metadata_safe_point_span = true;
        self.debugger_static_metadata_source_span_step = true;
        self
    }

    /// Enables one bounded contract/source location under the existing
    /// opaque parent and separate ID inventories. The debugger peer must
    /// still negotiate each grant and receive both IDs on its stream.
    pub fn with_debugger_static_metadata_contract_location(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_source_inventory = true;
        self.debugger_static_metadata_contract_inventory = true;
        self.debugger_static_metadata_contract_location = true;
        self
    }

    /// Enables one verified symbol/type relation and its parent, symbol,
    /// and type inventory policies. A debugger peer still must negotiate
    /// each grant and obtain both exact ID receipts on its own stream.
    pub fn with_debugger_static_metadata_symbol_type(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_type_inventory = true;
        self.debugger_static_metadata_symbol_inventory = true;
        self.debugger_static_metadata_symbol_type = true;
        self
    }

    /// Enables one verified symbol/contract relation and its parent,
    /// symbol, and contract inventory policies. A debugger peer still
    /// must negotiate each grant and obtain both exact ID receipts.
    pub fn with_debugger_static_metadata_symbol_contract(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_symbol_inventory = true;
        self.debugger_static_metadata_contract_inventory = true;
        self.debugger_static_metadata_symbol_contract = true;
        self
    }

    /// Enables compiler-only paused-slot relations and their three opaque
    /// inventories. It does not enable runtime value previews.
    pub fn with_debugger_static_scope_relation(mut self) -> Self {
        self.debugger_static_metadata_inventory = true;
        self.debugger_static_metadata_type_inventory = true;
        self.debugger_static_metadata_symbol_inventory = true;
        self.debugger_static_scope_relation = true;
        self
    }
}
