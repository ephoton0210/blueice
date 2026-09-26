// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl CompilerServiceIpcAdapter {
    /// Wraps a service selected by the core owner. The adapter never exposes
    /// the service mutably, preventing an IPC caller from changing its
    /// registration inputs after construction.
    pub fn new(
        service: RegisteredProjectCompilerService,
        limits: CompilerServiceIpcLimits,
    ) -> Result<Self, CompilerServiceIpcConfigurationError> {
        validate_limits(limits)?;
        if service.registered_project_ids().count() > COMPILER_MAX_PROJECT_INVENTORY {
            return Err(CompilerServiceIpcConfigurationError::TooManyRegisteredProjects);
        }
        Ok(Self {
            service,
            limits,
            // A pre-populated service is not evidence that its projects were
            // selected for public compiler IPC. Exposure is explicit only.
            registered_projects: BTreeSet::new(),
            project_inventory_streams: BTreeMap::new(),
            session_cursors: BTreeMap::new(),
        })
    }

    /// Creates an empty core-owned service with the supplied service and IPC
    /// retention limits. Projects still have to be registered by a core owner
    /// using [`Self::register_core_project`] before the protocol can inspect
    /// them.
    pub fn with_limits(
        service_limits: CompilerServiceLimits,
        ipc_limits: CompilerServiceIpcLimits,
    ) -> Result<Self, CompilerServiceIpcConfigurationError> {
        Self::new(
            RegisteredProjectCompilerService::new(service_limits),
            ipc_limits,
        )
    }

    /// Core-only registration seam. It is intentionally not represented in
    /// [`CompilerRequest`]: remote callers have no path, source graph,
    /// resolver, plugin, compiler-option, or output-root field to influence.
    pub fn register_core_project(
        &mut self,
        registration: RegisteredProjectRegistration,
    ) -> Result<CompilerProject, CompilerServiceError> {
        self.register_core_project_with_visibility(registration, true)
    }

    /// Keeps a core-owned registration out of every public stream inventory.
    /// The service still owns it for later privileged core-only work; no
    /// compiler IPC or MCP request can promote its visibility after sealing.
    pub(super) fn register_core_project_private(
        &mut self,
        registration: RegisteredProjectRegistration,
    ) -> Result<CompilerProject, CompilerServiceError> {
        self.register_core_project_with_visibility(registration, false)
    }

    fn register_core_project_with_visibility(
        &mut self,
        registration: RegisteredProjectRegistration,
        exposed: bool,
    ) -> Result<CompilerProject, CompilerServiceError> {
        if self.service.registered_project_ids().count() >= COMPILER_MAX_PROJECT_INVENTORY {
            return Err(CompilerServiceError::ProjectLimit {
                limit: COMPILER_MAX_PROJECT_INVENTORY,
            });
        }
        let project = project_to_wire(self.service.register(registration)?);
        if exposed {
            self.registered_projects.insert(project.id);
        }
        Ok(project)
    }

    /// Handles one request after the transport has successfully negotiated
    /// `Hello`. A transport owner is responsible for first-message handling;
    /// an in-band `Hello` is rejected rather than renegotiating state.
    pub fn handle(&mut self, request: CompilerRequest) -> CompilerReply {
        if let Some(project_id) = compiler_request_project_id(&request) {
            // Direct core-side callers must not bypass owner visibility. Keep
            // malformed and unknown handle categories unchanged by denying
            // only identities the underlying service actually owns.
            if !self.registered_projects.contains(&project_id)
                && self
                    .service
                    .registered_project_ids()
                    .any(|project| project.as_u64() == project_id)
            {
                return CompilerReply::Error {
                    code: CompilerErrorCode::UnobservedProject,
                    message: "compiler project was not exposed by the core owner".to_string(),
                };
            }
        }
        match request {
            CompilerRequest::ListProjects => self.project_inventory(),
            CompilerRequest::DescribeProject { project } => self.describe_project(project),
            CompilerRequest::Check { project } => self.check(project),
            CompilerRequest::ListDiagnostics {
                generation,
                cursor,
                limit,
            } => self.diagnostic_page(generation, cursor, limit),
            CompilerRequest::ListWorkSet {
                generation,
                kind,
                cursor,
                limit,
            } => self.work_set_page(generation, kind, cursor, limit),
            CompilerRequest::GetStaticType {
                generation,
                type_id,
            } => self.static_type(generation, type_id),
            CompilerRequest::GetStaticSymbol {
                generation,
                symbol_id,
            } => self.static_symbol(generation, symbol_id),
            CompilerRequest::GetStaticSymbolLocation {
                generation,
                symbol_id,
                source_id,
            } => self.static_symbol_location(generation, symbol_id, source_id),
            CompilerRequest::ListStaticMetadata {
                generation,
                kind,
                cursor,
                limit,
            } => self.static_metadata_page(generation, kind, cursor, limit),
            CompilerRequest::GetStaticProvenance {
                generation,
                source_id,
            } => self.static_provenance(generation, source_id),
            CompilerRequest::GetStaticContract {
                generation,
                contract_id,
            } => self.static_contract(generation, contract_id),
            CompilerRequest::GetStaticContractLocation {
                generation,
                contract_id,
                source_id,
            } => self.static_contract_location(generation, contract_id, source_id),
            CompilerRequest::ValidateStaticContract {
                generation,
                contract_id,
                value,
            } => self.validate_static_contract(generation, contract_id, value),
            CompilerRequest::Hello { .. } => CompilerReply::Error {
                code: CompilerErrorCode::ProtocolVersion,
                message: "compiler Hello is valid only as the first request".to_string(),
            },
            CompilerRequest::Unknown => CompilerReply::Unsupported {
                operation: "unknown compiler request".to_string(),
                reason: "this core build does not recognize the requested compiler operation"
                    .to_string(),
            },
        }
    }

    /// Applies a decoded request under the exact accepted compiler stream's
    /// core-minted attestation. A cursor is usable only if this stream
    /// previously received it as `next_cursor` for the same generation and
    /// collection. The attestation is internal hand-off data, not an IPC
    /// request field that a remote client can choose.
    pub(super) fn handle_session_request(
        &mut self,
        session_id: &str,
        request: CompilerRequest,
    ) -> CompilerReply {
        if matches!(request, CompilerRequest::ListProjects) {
            if !self.project_inventory_streams.contains_key(session_id)
                && self.project_inventory_streams.len() >= MAX_PROJECT_INVENTORY_STREAMS
            {
                return CompilerReply::Error {
                    code: CompilerErrorCode::ResourceLimit,
                    message: "compiler project inventory stream limit exceeded".to_string(),
                };
            }
            let reply = self.project_inventory();
            if let CompilerReply::Projects(inventory) = &reply {
                self.project_inventory_streams.insert(
                    session_id.to_string(),
                    inventory
                        .projects
                        .iter()
                        .map(|project| project.id)
                        .collect(),
                );
            }
            return reply;
        }
        if let Some(project_id) = compiler_request_project_id(&request) {
            if !self
                .project_inventory_streams
                .get(session_id)
                .is_some_and(|projects| projects.contains(&project_id))
            {
                return CompilerReply::Error {
                    code: CompilerErrorCode::UnobservedProject,
                    message: "compiler project was not inventoried on this stream".to_string(),
                };
            }
        }
        let pagination = match &request {
            CompilerRequest::ListDiagnostics {
                generation, cursor, ..
            } => Some((
                *generation,
                CompilerSessionCursorKind::Diagnostics,
                cursor.map(|cursor| cursor.id),
            )),
            CompilerRequest::ListWorkSet {
                generation,
                kind,
                cursor,
                ..
            } => Some((
                *generation,
                CompilerSessionCursorKind::WorkSet(*kind),
                cursor.map(|cursor| cursor.id),
            )),
            CompilerRequest::ListStaticMetadata {
                generation,
                kind,
                cursor,
                ..
            } => Some((
                *generation,
                CompilerSessionCursorKind::from_static_kind(*kind),
                cursor.map(|cursor| cursor.id),
            )),
            _ => None,
        };
        let checked_project = match &request {
            CompilerRequest::Check { project } => Some(project.id),
            _ => None,
        };
        let presented_cursor = pagination.and_then(|(generation, kind, id)| {
            id.map(|id| CompilerSessionCursorReceipt::new(generation, kind, id))
        });
        if let Some(cursor) = presented_cursor {
            if !self
                .session_cursors
                .get(session_id)
                .is_some_and(|receipts| receipts.contains(&cursor))
            {
                let (code, message) = match cursor.kind {
                    CompilerSessionCursorKind::Diagnostics => (
                        CompilerErrorCode::InvalidDiagnosticCursor,
                        "compiler diagnostic cursor was not returned on this stream",
                    ),
                    CompilerSessionCursorKind::WorkSet(_) => (
                        CompilerErrorCode::InvalidWorkSetCursor,
                        "compiler work-set cursor was not returned on this stream",
                    ),
                    _ => (
                        CompilerErrorCode::InvalidMetadataCursor,
                        "static metadata cursor was not returned on this stream",
                    ),
                };
                return CompilerReply::Error {
                    code,
                    message: message.to_string(),
                };
            }
        }
        let reply = self.handle(request);
        if let Some(project_id) = checked_project {
            // A check may advance the generation before a response-budget
            // error is reported. Conservatively revoke every old cursor for
            // that project, including cursors held by another stream.
            self.revoke_project_session_cursors(project_id);
        }
        let next_cursor = match (pagination, &reply) {
            (
                Some((generation, CompilerSessionCursorKind::Diagnostics, _)),
                CompilerReply::DiagnosticPage(page),
            ) if page.generation == generation => page.next_cursor.map(|cursor| {
                CompilerSessionCursorReceipt::new(
                    generation,
                    CompilerSessionCursorKind::Diagnostics,
                    cursor.id,
                )
            }),
            (
                Some((generation, CompilerSessionCursorKind::WorkSet(kind), _)),
                CompilerReply::WorkSetPage(page),
            ) if page.generation == generation && page.kind == kind => {
                page.next_cursor.map(|cursor| {
                    CompilerSessionCursorReceipt::new(
                        generation,
                        CompilerSessionCursorKind::WorkSet(kind),
                        cursor.id,
                    )
                })
            }
            (Some((generation, kind, _)), CompilerReply::StaticMetadataPage(page))
                if page.generation == generation
                    && CompilerSessionCursorKind::from_static_kind(page.kind) == kind =>
            {
                page.next_cursor
                    .map(|cursor| CompilerSessionCursorReceipt::new(generation, kind, cursor.id))
            }
            _ => None,
        };
        if matches!(
            &reply,
            CompilerReply::DiagnosticPage(_)
                | CompilerReply::WorkSetPage(_)
                | CompilerReply::StaticMetadataPage(_)
        ) {
            let receipts = self
                .session_cursors
                .entry(session_id.to_string())
                .or_default();
            if let Some(cursor) = presented_cursor {
                receipts.remove(&cursor);
            }
            if let Some(cursor) = next_cursor {
                receipts.insert(cursor);
            }
            if receipts.is_empty() {
                self.session_cursors.remove(session_id);
            }
            if let Some(next_cursor) = next_cursor {
                let total = self
                    .session_cursors
                    .values()
                    .map(BTreeSet::len)
                    .sum::<usize>();
                if total > self.limits.max_stream_cursor_receipts {
                    let receipts = self
                        .session_cursors
                        .get_mut(session_id)
                        .expect("the newly minted cursor was just recorded on this stream");
                    receipts.remove(&next_cursor);
                    if receipts.is_empty() {
                        self.session_cursors.remove(session_id);
                    }
                    self.release_session_cursors(BTreeSet::from([next_cursor]));
                    return CompilerReply::Error {
                        code: CompilerErrorCode::ResourceLimit,
                        message: "compiler stream cursor receipt limit exceeded".to_string(),
                    };
                }
            }
        }
        reply
    }

    /// Releases unconsumed cursor slots when the accepted stream ends. This
    /// prevents a client from exhausting the core's fixed cursor budget by
    /// repeatedly abandoning first pages and reconnecting.
    pub(super) fn end_session(&mut self, session_id: &str) {
        self.project_inventory_streams.remove(session_id);
        let Some(receipts) = self.session_cursors.remove(session_id) else {
            return;
        };
        self.release_session_cursors(receipts);
    }

    fn project_inventory(&self) -> CompilerReply {
        let projects = self
            .registered_projects
            .iter()
            .map(|id| CompilerProject { id: *id })
            .collect::<Vec<_>>();
        let inventory = CompilerProjectInventory { projects };
        if !inventory.is_well_formed() {
            return response_limit_reply();
        }
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(128 + 32 * inventory.projects.len()) {
            return response_limit_reply();
        }
        CompilerReply::Projects(inventory)
    }

    fn revoke_project_session_cursors(&mut self, project_id: u64) {
        let mut revoked = BTreeSet::new();
        self.session_cursors.retain(|_, receipts| {
            receipts.retain(|receipt| {
                if receipt.project_id == project_id {
                    revoked.insert(*receipt);
                    false
                } else {
                    true
                }
            });
            !receipts.is_empty()
        });
        self.release_session_cursors(revoked);
    }

    fn release_session_cursors(&mut self, receipts: BTreeSet<CompilerSessionCursorReceipt>) {
        let mut metadata = Vec::new();
        let mut diagnostics = Vec::new();
        let mut work_sets = Vec::new();
        for receipt in receipts {
            match receipt.kind {
                CompilerSessionCursorKind::Diagnostics => diagnostics.push(receipt.id),
                CompilerSessionCursorKind::WorkSet(_) => work_sets.push(receipt.id),
                _ => metadata.push(receipt.id),
            }
        }
        self.service
            .revoke_inventory_cursors(&metadata, &diagnostics, &work_sets);
    }

    fn describe_project(&self, project: CompilerProject) -> CompilerReply {
        let project_id = match project_from_wire(project) {
            Ok(project_id) => project_id,
            Err(error) => return handle_error_reply(error),
        };
        let identity = match self.service.identity(project_id) {
            Ok(identity) => identity,
            Err(error) => return service_error_reply(&error),
        };
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(256)
            || !budget.reserve_required_string(&identity.entry_module, self.limits.max_field_bytes)
        {
            return response_limit_reply();
        }
        CompilerReply::Project(CompilerProjectIdentity {
            project,
            entry_module: identity.entry_module,
        })
    }

    fn check(&mut self, project: CompilerProject) -> CompilerReply {
        let project_id = match project_from_wire(project) {
            Ok(project_id) => project_id,
            Err(error) => return handle_error_reply(error),
        };
        let check = match self.service.check(project_id) {
            Ok(check) => check,
            Err(error) => return service_error_reply(&error),
        };
        // A successful check is the only public way an MCP receipt learns a
        // generation. Validate every retained diagnostic field now, before
        // any one-shot diagnostic cursor exists, so a later page cannot lose
        // a cursor merely because an unseen later entry violates the fixed
        // public field policy.
        if !retained_diagnostics_fit_wire_policy(
            &check.retained_diagnostics,
            &check.retained_diagnostic_locations,
            self.limits,
        ) {
            return response_limit_reply();
        }
        if [
            &check.parsed_modules,
            &check.reused_parsed_modules,
            &check.rechecked_modules,
            &check.reused_checked_modules,
        ]
        .into_iter()
        .flatten()
        .any(|module| module.len() > self.limits.max_field_bytes)
        {
            return response_limit_reply();
        }
        match self.check_to_wire(check) {
            Ok(check) => CompilerReply::Check(check),
            Err(()) => response_limit_reply(),
        }
    }

    /// Returns one source-free diagnostic page for an exact retained
    /// generation. The adapter bounds a page using the same pessimistic JSON
    /// accounting as ordinary check replies before it asks the service to
    /// consume a one-shot cursor.
    fn diagnostic_page(
        &mut self,
        generation: CompilerGeneration,
        cursor: Option<CompilerDiagnosticCursor>,
        requested_limit: Option<u32>,
    ) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        if cursor.is_some_and(|cursor| !cursor.is_well_formed()) {
            return CompilerReply::Error {
                code: CompilerErrorCode::InvalidDiagnosticCursor,
                message: "invalid compiler diagnostic cursor".to_string(),
            };
        }
        let requested_limit = match requested_limit {
            Some(0) => {
                return CompilerReply::Error {
                    code: CompilerErrorCode::InvalidDiagnosticPage,
                    message: "compiler diagnostic page limit must be positive".to_string(),
                };
            }
            Some(limit) => usize::try_from(limit).unwrap_or(usize::MAX),
            None => self.limits.max_diagnostic_page_entries,
        };
        // A diagnostic always carries two project-controlled strings (module
        // identity and prose). Reserve their worst-case JSON expansion before
        // the service accepts a cursor, so an accepted page cannot overflow
        // this adapter's response envelope merely because a field is dense in
        // escapable bytes.
        const PAGE_FIXED_BYTES: usize = 256;
        const PAGE_ENTRY_FIXED_BYTES: usize = 352;
        let max_entry_bytes = self
            .limits
            .max_field_bytes
            .checked_mul(12)
            .and_then(|bytes| {
                COMPILER_DIAGNOSTIC_MAX_CODE_BYTES
                    .checked_mul(6)
                    .and_then(|code_bytes| bytes.checked_add(code_bytes))
            })
            .and_then(|bytes| bytes.checked_add(PAGE_ENTRY_FIXED_BYTES));
        let Some(max_entry_bytes) = max_entry_bytes else {
            return response_limit_reply();
        };
        let response_cap = self
            .limits
            .max_response_bytes
            .saturating_sub(PAGE_FIXED_BYTES)
            / max_entry_bytes;
        let limit = requested_limit
            .min(self.limits.max_diagnostic_page_entries)
            .min(response_cap);
        if limit == 0 {
            return response_limit_reply();
        }
        let page = match self.service.diagnostic_inventory(
            generation,
            cursor.map(|cursor| cursor.id),
            limit,
        ) {
            Ok(page) => page,
            Err(error) => return service_error_reply(&error),
        };
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(PAGE_FIXED_BYTES) {
            if let Some(id) = page.next_cursor {
                self.service.revoke_inventory_cursors(&[], &[id], &[]);
            }
            return response_limit_reply();
        }
        let entries = match diagnostic_page_entries_to_wire(
            &page.entries,
            &page.locations,
            self.limits,
            &mut budget,
        ) {
            Ok(entries) => entries,
            Err(()) => {
                if let Some(id) = page.next_cursor {
                    self.service.revoke_inventory_cursors(&[], &[id], &[]);
                }
                return response_limit_reply();
            }
        };
        CompilerReply::DiagnosticPage(CompilerDiagnosticPage {
            generation: generation_to_wire(generation),
            entries,
            next_cursor: page.next_cursor.map(|id| CompilerDiagnosticCursor { id }),
            truncated: page.truncated,
        })
    }

    fn work_set_page(
        &mut self,
        generation: CompilerGeneration,
        kind: CompilerWorkSetKind,
        cursor: Option<CompilerWorkSetCursor>,
        requested_limit: Option<u32>,
    ) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        if cursor.is_some_and(|cursor| !cursor.is_well_formed()) {
            return CompilerReply::Error {
                code: CompilerErrorCode::InvalidWorkSetCursor,
                message: "invalid compiler work-set cursor".to_string(),
            };
        }
        let requested_limit = match requested_limit {
            Some(0) => {
                return CompilerReply::Error {
                    code: CompilerErrorCode::InvalidWorkSetPage,
                    message: "compiler work-set page limit must be positive".to_string(),
                };
            }
            Some(limit) => usize::try_from(limit).unwrap_or(usize::MAX),
            None => self.limits.max_work_set_page_entries,
        };
        const PAGE_FIXED_BYTES: usize = 256;
        const ENTRY_FIXED_BYTES: usize = 64;
        let Some(max_entry_bytes) = self
            .limits
            .max_field_bytes
            .checked_mul(6)
            .and_then(|bytes| bytes.checked_add(ENTRY_FIXED_BYTES + 2))
        else {
            return response_limit_reply();
        };
        let response_cap = self
            .limits
            .max_response_bytes
            .saturating_sub(PAGE_FIXED_BYTES)
            / max_entry_bytes;
        let limit = requested_limit
            .min(self.limits.max_work_set_page_entries)
            .min(response_cap);
        if limit == 0 {
            return response_limit_reply();
        }
        let page = match self.service.work_set_inventory(
            generation,
            work_set_kind_from_wire(kind),
            cursor.map(|cursor| cursor.id),
            limit,
        ) {
            Ok(page) => page,
            Err(error) => return service_error_reply(&error),
        };
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        let valid = budget.reserve_fixed(PAGE_FIXED_BYTES)
            && page.entries.iter().all(|entry| {
                budget.reserve_required_string(entry, self.limits.max_field_bytes)
                    && budget.reserve_fixed(ENTRY_FIXED_BYTES)
            });
        if !valid {
            if let Some(id) = page.next_cursor {
                self.service.revoke_inventory_cursors(&[], &[], &[id]);
            }
            return response_limit_reply();
        }
        CompilerReply::WorkSetPage(CompilerWorkSetPage {
            generation: generation_to_wire(generation),
            kind,
            entries: page.entries,
            next_cursor: page.next_cursor.map(|id| CompilerWorkSetCursor { id }),
            truncated: page.truncated,
        })
    }

    fn static_type(&self, generation: CompilerGeneration, type_id: u32) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        let static_type = match self
            .service
            .static_type(generation, blueice_bluets::TypeId(type_id))
        {
            Ok(static_type) => static_type,
            Err(error) => return service_error_reply(&error),
        };
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(256)
            || !budget.reserve_required_string(&static_type.display, self.limits.max_field_bytes)
        {
            return response_limit_reply();
        }
        CompilerReply::StaticType(CompilerStaticType {
            generation: generation_to_wire(generation),
            id: static_type.id.0,
            display: static_type.display,
        })
    }

    fn static_symbol(&self, generation: CompilerGeneration, symbol_id: u32) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        let symbol = match self
            .service
            .static_symbol(generation, blueice_bluets::SymbolId(symbol_id))
        {
            Ok(symbol) => symbol,
            Err(error) => return service_error_reply(&error),
        };
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(384)
            || !budget.reserve_required_string(&symbol.name, self.limits.max_field_bytes)
            || !budget.reserve_required_string(&symbol.span.module, self.limits.max_field_bytes)
        {
            return response_limit_reply();
        }
        let (start, end) = match (
            u64::try_from(symbol.span.start),
            u64::try_from(symbol.span.end),
        ) {
            (Ok(start), Ok(end)) => (start, end),
            _ => return response_limit_reply(),
        };
        CompilerReply::StaticSymbol(CompilerStaticSymbol {
            generation: generation_to_wire(generation),
            id: symbol.id.0,
            name: symbol.name,
            kind: symbol_kind_to_wire(symbol.kind),
            exported: symbol.exported,
            module: symbol.span.module,
            start,
            end,
            static_type_id: symbol.static_type.map(|id| id.0),
            source_id: symbol.source.0,
            contract_id: symbol.contract.map(|id| id.0),
        })
    }

    fn static_symbol_location(
        &self,
        generation: CompilerGeneration,
        symbol_id: u32,
        source_id: u32,
    ) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        let symbol = match self
            .service
            .static_symbol(generation, blueice_bluets::SymbolId(symbol_id))
        {
            Ok(symbol) => symbol,
            Err(error) => return service_error_reply(&error),
        };
        if symbol.source.0 != source_id {
            return invalid_location_target_reply();
        }
        let Some((start_byte, end_byte, coordinates)) =
            compiler_declaration_location(&symbol.span, symbol.location)
        else {
            return response_limit_reply();
        };
        let reply = CompilerStaticSymbolLocation {
            generation: generation_to_wire(generation),
            symbol_id,
            source_id,
            start_byte,
            end_byte,
            coordinates,
        };
        if !reply.is_well_formed()
            || !ResponseBudget::new(self.limits.max_response_bytes).reserve_fixed(256)
        {
            return response_limit_reply();
        }
        CompilerReply::StaticSymbolLocation(reply)
    }

    /// Returns one source-free page of opaque static IDs. The cursor is
    /// validated by both this adapter and the core service, which binds it to
    /// the exact retained generation and consumes it after one use. The page
    /// cap has both a configured policy limit and a response-budget limit.
    fn static_metadata_page(
        &mut self,
        generation: CompilerGeneration,
        kind: CompilerStaticMetadataKind,
        cursor: Option<CompilerStaticMetadataCursor>,
        requested_limit: Option<u32>,
    ) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        if cursor.is_some_and(|cursor| !cursor.is_well_formed()) {
            return CompilerReply::Error {
                code: CompilerErrorCode::InvalidMetadataCursor,
                message: "invalid static metadata cursor".to_string(),
            };
        }
        let requested_limit = match requested_limit {
            Some(0) => {
                return CompilerReply::Error {
                    code: CompilerErrorCode::InvalidMetadataPage,
                    message: "static metadata page limit must be positive".to_string(),
                };
            }
            Some(limit) => usize::try_from(limit).unwrap_or(usize::MAX),
            None => self.limits.max_static_metadata_page_entries,
        };
        // Each ID, list delimiter and conservative JSON framing are charged
        // before the service consumes a one-shot cursor. This means a tiny
        // adapter response policy returns a limit failure without losing the
        // cursor or accidentally creating a partial page.
        const PAGE_FIXED_BYTES: usize = 256;
        const PAGE_ID_BYTES: usize = 32;
        let response_cap = self
            .limits
            .max_response_bytes
            .saturating_sub(PAGE_FIXED_BYTES)
            / PAGE_ID_BYTES;
        let limit = requested_limit
            .min(self.limits.max_static_metadata_page_entries)
            .min(response_cap);
        if limit == 0 {
            return response_limit_reply();
        }
        let page = match self.service.static_metadata_inventory(
            generation,
            static_metadata_kind_from_wire(kind),
            cursor.map(|cursor| cursor.id),
            limit,
        ) {
            Ok(page) => page,
            Err(error) => return service_error_reply(&error),
        };
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(PAGE_FIXED_BYTES)
            || page
                .ids
                .iter()
                .any(|_| !budget.reserve_optional_fixed(PAGE_ID_BYTES))
        {
            if let Some(id) = page.next_cursor {
                self.service.revoke_inventory_cursors(&[id], &[], &[]);
            }
            return response_limit_reply();
        }
        CompilerReply::StaticMetadataPage(CompilerStaticMetadataPage {
            generation: generation_to_wire(generation),
            kind,
            ids: page.ids,
            next_cursor: page
                .next_cursor
                .map(|id| CompilerStaticMetadataCursor { id }),
        })
    }

    fn static_provenance(&self, generation: CompilerGeneration, source_id: u32) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        let provenance = match self
            .service
            .static_provenance(generation, SourceId(source_id))
        {
            Ok(provenance) => provenance,
            Err(error) => return service_error_reply(&error),
        };
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(384)
            || !budget.reserve_required_string(&provenance.module, self.limits.max_field_bytes)
            || !budget
                .reserve_required_string(&provenance.content_hash, self.limits.max_field_bytes)
        {
            return response_limit_reply();
        }
        CompilerReply::StaticProvenance(CompilerStaticProvenance {
            generation: generation_to_wire(generation),
            source_id: provenance.id.0,
            module: provenance.module,
            content_hash: provenance.content_hash,
        })
    }

    fn static_contract(&self, generation: CompilerGeneration, contract_id: u32) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        let contract = match self
            .service
            .static_contract(generation, ContractId(contract_id))
        {
            Ok(contract) => contract,
            Err(error) => return service_error_reply(&error),
        };
        let root = format!("{:?}", contract.plan.root);
        let definitions = format!("{:?}", contract.plan.definitions);
        let definition_count = match u32::try_from(contract.plan.definitions.len()) {
            Ok(count) => count,
            Err(_) => return response_limit_reply(),
        };
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(512)
            || !budget.reserve_required_string(&contract.name, self.limits.max_field_bytes)
            || !budget
                .reserve_required_string(&contract.plan.fingerprint, self.limits.max_field_bytes)
            || !budget.reserve_required_string(&root, self.limits.max_field_bytes)
            || !budget.reserve_required_string(&definitions, self.limits.max_field_bytes)
        {
            return response_limit_reply();
        }
        CompilerReply::StaticContract(CompilerStaticContract {
            generation: generation_to_wire(generation),
            contract_id: contract.id.0,
            source_id: contract.source.0,
            name: contract.name,
            fingerprint: contract.plan.fingerprint,
            root,
            definitions,
            definition_count,
        })
    }

    fn static_contract_location(
        &self,
        generation: CompilerGeneration,
        contract_id: u32,
        source_id: u32,
    ) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        let contract = match self
            .service
            .static_contract(generation, ContractId(contract_id))
        {
            Ok(contract) => contract,
            Err(error) => return service_error_reply(&error),
        };
        if contract.source.0 != source_id {
            return invalid_location_target_reply();
        }
        let Some((start_byte, end_byte, coordinates)) =
            compiler_declaration_location(&contract.span, contract.location)
        else {
            return response_limit_reply();
        };
        let reply = CompilerStaticContractLocation {
            generation: generation_to_wire(generation),
            contract_id,
            source_id,
            start_byte,
            end_byte,
            coordinates,
        };
        if !reply.is_well_formed()
            || !ResponseBudget::new(self.limits.max_response_bytes).reserve_fixed(256)
        {
            return response_limit_reply();
        }
        CompilerReply::StaticContractLocation(reply)
    }

    fn validate_static_contract(
        &self,
        generation: CompilerGeneration,
        contract_id: u32,
        value: CompilerContractValue,
    ) -> CompilerReply {
        let generation = match generation_from_wire(generation) {
            Ok(generation) => generation,
            Err(error) => return handle_error_reply(error),
        };
        let value = match wire_contract_value(value, self.service.contract_validation_limits()) {
            Ok(value) => value,
            Err(()) => {
                return CompilerReply::Error {
                    code: CompilerErrorCode::InvalidContractValue,
                    message: "invalid or over-budget data-only contract value".to_string(),
                };
            }
        };
        let outcome =
            match self
                .service
                .validate_static_contract(generation, ContractId(contract_id), &value)
            {
                Ok(outcome) => outcome,
                Err(error) => return service_error_reply(&error),
            };
        let failure = outcome.err().map(validation_failure_to_wire);
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(512)
            || failure.as_ref().is_some_and(|failure| {
                !budget.reserve_required_string(&failure.path, self.limits.max_field_bytes)
                    || !budget
                        .reserve_required_string(&failure.expected, self.limits.max_field_bytes)
                    || !budget
                        .reserve_required_string(&failure.observed, self.limits.max_field_bytes)
            })
        {
            return response_limit_reply();
        }
        CompilerReply::ContractValidation(CompilerContractValidation {
            generation: generation_to_wire(generation),
            contract_id,
            valid: failure.is_none(),
            failure,
        })
    }

    fn check_to_wire(&self, check: CompilerServiceCheck) -> Result<CompilerCheck, ()> {
        let mut budget = ResponseBudget::new(self.limits.max_response_bytes);
        if !budget.reserve_fixed(2_048) {
            return Err(());
        }
        let parsed_modules = module_list(&check.parsed_modules, self.limits, &mut budget)?;
        let reused_parsed_modules =
            module_list(&check.reused_parsed_modules, self.limits, &mut budget)?;
        let rechecked_modules = module_list(&check.rechecked_modules, self.limits, &mut budget)?;
        let reused_checked_modules =
            module_list(&check.reused_checked_modules, self.limits, &mut budget)?;
        let diagnostics = diagnostics_to_wire(
            &check.diagnostics.entries,
            check
                .retained_diagnostic_locations
                .get(..check.diagnostics.entries.len())
                .ok_or(())?,
            check.diagnostics.truncated,
            self.limits,
            &mut budget,
        )?;
        let artifact_fingerprint = match check.artifact_fingerprint {
            Some(fingerprint) => {
                if !budget.reserve_required_string(&fingerprint, self.limits.max_field_bytes) {
                    return Err(());
                }
                Some(fingerprint)
            }
            None => None,
        };
        let static_metadata = match check.static_debug_info {
            Some(info) => {
                if !budget.reserve_fixed(256)
                    || !budget.reserve_required_string(
                        &info.language_version,
                        self.limits.max_field_bytes,
                    )
                    || !budget.reserve_required_string(
                        &info.compiler_options_hash,
                        self.limits.max_field_bytes,
                    )
                {
                    return Err(());
                }
                Some(CompilerStaticMetadataSummary {
                    language_version: info.language_version,
                    compiler_options_hash: info.compiler_options_hash,
                    source_count: u32::try_from(info.sources.len()).map_err(|_| ())?,
                    type_count: u32::try_from(info.types.len()).map_err(|_| ())?,
                    symbol_count: u32::try_from(info.symbols.len()).map_err(|_| ())?,
                    contract_count: u32::try_from(info.contracts.len()).map_err(|_| ())?,
                })
            }
            None => None,
        };
        Ok(CompilerCheck {
            generation: generation_to_wire(check.generation),
            cache_hit: check.cache_hit,
            parsed_modules,
            reused_parsed_modules,
            rechecked_modules,
            reused_checked_modules,
            diagnostics,
            has_errors: check.has_errors,
            artifact_fingerprint,
            static_metadata,
        })
    }
}
