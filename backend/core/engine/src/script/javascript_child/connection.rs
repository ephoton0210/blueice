// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// One connected, authenticated private page-host transport. It intentionally
/// owns no child process: `blueice-launcher` remains the supervisor and must
/// reap the child after core disconnects or exits.
pub struct PageHostConnection {
    pub(super) stream: UnixStream,
}

impl PageHostConnection {
    /// Connects to a launcher-created private child socket and completes the
    /// versioned capability handshake. The caller must obtain both values from its
    /// launcher owner; a page, frontend client, or script never receives this
    /// configuration.
    pub fn connect(socket_path: &Path, session_token: &str) -> io::Result<Self> {
        let mut stream = UnixStream::connect(socket_path)?;
        page_host::write_page_host_request(
            &mut stream,
            &PageHostRequest::Hello {
                protocol_version: page_host::PAGE_HOST_PROTOCOL_VERSION,
                session_token: session_token.to_string(),
            },
        )?;
        match page_host::read_page_host_reply(&mut stream)? {
            PageHostReply::HelloAck {
                protocol_version: page_host::PAGE_HOST_PROTOCOL_VERSION,
            } => Ok(Self { stream }),
            reply => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("BlueJS child host rejected core handshake: {reply:?}"),
            )),
        }
    }

    pub(super) fn request(&mut self, request: PageHostRequest) -> io::Result<PageHostReply> {
        page_host::write_page_host_request(&mut self.stream, &request)?;
        page_host::read_page_host_reply(&mut self.stream)
    }

    /// Reads the child result on a transport-only worker while the calling
    /// core session thread remains free to serve this document's DOM calls.
    /// No page, VM, or `TabManager` reference crosses to the reader worker.
    pub(super) fn request_while_pumping_script(
        &mut self,
        request: PageHostRequest,
        pump: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<PageHostReply> {
        self.request_while_pumping_script_with_timeout(request, pump, CHILD_DOCUMENT_REPLY_WAIT)
    }

    pub(super) fn request_while_pumping_script_with_timeout(
        &mut self,
        request: PageHostRequest,
        pump: &mut dyn FnMut() -> io::Result<()>,
        wait: Duration,
    ) -> io::Result<PageHostReply> {
        if wait.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "page-host wait must be nonzero",
            ));
        }
        let deadline = Instant::now() + wait;
        self.stream.set_write_timeout(Some(wait)).map_err(|error| {
            io::Error::new(error.kind(), format!("page-host write timeout: {error}"))
        })?;
        if let Err(error) = page_host::write_page_host_request(&mut self.stream, &request) {
            let _ = self.stream.shutdown(Shutdown::Both);
            return Err(error);
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let _ = self.stream.shutdown(Shutdown::Both);
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "page-host request write exceeded its fixed wait",
            ));
        }
        let mut reader = match self.stream.try_clone() {
            Ok(reader) => reader,
            Err(error) => {
                let _ = self.stream.shutdown(Shutdown::Both);
                return Err(error);
            }
        };
        if let Err(error) = reader.set_read_timeout(Some(remaining)) {
            let _ = self.stream.shutdown(Shutdown::Both);
            return Err(io::Error::new(
                error.kind(),
                format!("page-host read timeout: {error}"),
            ));
        }
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        let reader_task = thread::spawn(move || {
            let _ = reply_sender.send(page_host::read_page_host_reply(&mut reader));
        });
        let result = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "page-host reply exceeded its fixed wait",
                ));
            }
            match reply_receiver.recv_timeout(remaining.min(CHILD_DOCUMENT_POLL_INTERVAL)) {
                Ok(result) if Instant::now() <= deadline => break result,
                Ok(_) => {
                    break Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "page-host reply exceeded its fixed wait",
                    ));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "page-host reply reader disconnected",
                    ));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if let Err(error) = pump() {
                break Err(error);
            }
        };
        if result.is_err() {
            let _ = self.stream.shutdown(Shutdown::Both);
        }
        if reader_task.join().is_err() {
            let _ = self.stream.shutdown(Shutdown::Both);
            return Err(io::Error::other("page-host reply reader panicked"));
        }
        // The socket retains fixed read/write bounds for subsequent control
        // operations. Some Unix platforms reject clearing SO_RCVTIMEO after
        // the peer has closed immediately following a valid reply.
        result
    }
}

impl PageHostClient for PageHostConnection {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::SynchronizeDocument { document })
    }

    fn synchronize_document_with_script_pump(
        &mut self,
        document: PageHostDocument,
        pump: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<PageHostReply> {
        self.request_while_pumping_script(PageHostRequest::SynchronizeDocument { document }, pump)
    }

    fn dispatch_click_with_script_pump(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        node_id: u64,
        pump: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<PageHostReply> {
        self.request_while_pumping_script(
            PageHostRequest::DispatchClick {
                tab_id,
                document_generation,
                node_id,
            },
            pump,
        )
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::CloseRealm {
            tab_id,
            document_generation,
        })
    }

    fn debugger_breakpoint_configuration_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_summary_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_lowering_summary_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_sources_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_source_provenance_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_types_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_type_display_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_symbols_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_contracts_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_contract_display_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_contract_validation_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_symbol_display_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_symbol_location_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_contract_location_available(&self) -> bool {
        true
    }

    fn debugger_bluets_safe_point_span_available(&self) -> bool {
        true
    }

    fn debugger_bluets_exception_location_available(&self) -> bool {
        true
    }

    fn debugger_bluets_source_breakpoint_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_symbol_type_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_symbol_contract_available(&self) -> bool {
        true
    }

    fn debugger_realm_stats(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::GetRealmStats {
            tab_id,
            document_generation,
        })
    }

    fn child_stats(&mut self) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::GetChildStats)
    }

    fn debugger_programs(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ListDebuggerPrograms {
            tab_id,
            document_generation,
        })
    }

    fn debugger_bluets_metadata(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id,
            document_generation,
            program,
        })
    }

    fn debugger_bluets_metadata_summary(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::DescribeDebuggerBlueTsMetadata {
            tab_id,
            document_generation,
            program,
            metadata,
        })
    }

    fn debugger_bluets_metadata_lowering_summary(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        self.request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataLoweringSummary {
                tab_id,
                document_generation,
                program,
                metadata,
            },
        )
    }

    fn debugger_bluets_metadata_sources(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ListDebuggerBlueTsMetadataSources {
            tab_id,
            document_generation,
            program,
            metadata,
        })
    }

    fn debugger_bluets_metadata_source_provenance(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::DescribeDebuggerBlueTsMetadataSource {
            tab_id,
            document_generation,
            program,
            metadata,
            source_id,
        })
    }

    fn debugger_bluets_metadata_types(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ListDebuggerBlueTsMetadataTypes {
            tab_id,
            document_generation,
            program,
            metadata,
        })
    }

    fn debugger_bluets_metadata_type_display(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        type_id: u32,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::DescribeDebuggerBlueTsMetadataType {
            tab_id,
            document_generation,
            program,
            metadata,
            type_id,
        })
    }

    fn debugger_bluets_metadata_symbols(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ListDebuggerBlueTsMetadataSymbols {
            tab_id,
            document_generation,
            program,
            metadata,
        })
    }

    fn debugger_bluets_metadata_contracts(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ListDebuggerBlueTsMetadataContracts {
            tab_id,
            document_generation,
            program,
            metadata,
        })
    }

    fn debugger_bluets_metadata_contract_display(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::DescribeDebuggerBlueTsMetadataContract {
            tab_id,
            document_generation,
            program,
            metadata,
            contract_id,
        })
    }

    fn debugger_bluets_metadata_contract_validation(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
        value: CompilerContractValue,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ValidateDebuggerBlueTsMetadataContract {
            tab_id,
            document_generation,
            program,
            metadata,
            contract_id,
            value,
        })
    }

    fn debugger_bluets_metadata_symbol_display(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbol {
            tab_id,
            document_generation,
            program,
            metadata,
            symbol_id,
        })
    }

    fn debugger_bluets_metadata_symbol_location(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
    ) -> io::Result<PageHostReply> {
        self.request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolLocation {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
            },
        )
    }

    fn debugger_bluets_metadata_contract_location(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        contract_id: u32,
    ) -> io::Result<PageHostReply> {
        self.request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
                tab_id,
                document_generation,
                program,
                metadata,
                contract_id,
            },
        )
    }

    fn debugger_bluets_safe_point_span(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::DescribeDebuggerBlueTsSafePointSpan {
            tab_id,
            document_generation,
            metadata,
            safe_point,
        })
    }

    fn debugger_bluets_exception_location(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::DescribeDebuggerBlueTsExceptionLocation {
            tab_id,
            document_generation,
            program,
            metadata,
        })
    }

    fn debugger_bluets_source_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
        source_byte: u32,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
            tab_id,
            document_generation,
            program,
            metadata,
            source_id,
            source_byte,
        })
    }

    fn debugger_bluets_metadata_symbol_type(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
        type_id: u32,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolType {
            tab_id,
            document_generation,
            program,
            metadata,
            symbol_id,
            type_id,
        })
    }

    fn debugger_bluets_metadata_symbol_contract(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        metadata: PageHostDebuggerMetadataHandle,
        symbol_id: u32,
        contract_id: u32,
    ) -> io::Result<PageHostReply> {
        self.request(
            PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolContract {
                tab_id,
                document_generation,
                program,
                metadata,
                symbol_id,
                contract_id,
            },
        )
    }

    fn debugger_safe_points(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ListDebuggerSafePoints {
            tab_id,
            document_generation,
            program,
        })
    }

    fn validate_debugger_safe_point(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ValidateDebuggerSafePoint {
            tab_id,
            document_generation,
            safe_point,
        })
    }

    fn set_debugger_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::SetDebuggerBreakpoint {
            tab_id,
            document_generation,
            safe_point,
        })
    }

    fn debugger_breakpoints(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ListDebuggerBreakpoints {
            tab_id,
            document_generation,
        })
    }

    fn clear_debugger_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ClearDebuggerBreakpoint {
            tab_id,
            document_generation,
            safe_point,
        })
    }

    fn debugger_execution_control_available(&self) -> bool {
        true
    }

    fn debugger_nested_frames_available(&self) -> bool {
        true
    }

    fn debugger_linked_frames_available(&self) -> bool {
        true
    }

    fn debugger_stack_snapshot_available(&self) -> bool {
        true
    }

    fn debugger_stack_snapshot(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        frame: Option<PageHostDebuggerFrame>,
        max_frames: u32,
        max_scope_entries: u32,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::GetDebuggerStackSnapshot {
            tab_id,
            document_generation,
            program,
            frame,
            max_frames,
            max_scope_entries,
        })
    }

    fn debugger_value_snapshot_available(&self) -> bool {
        true
    }

    fn debugger_value_snapshot(
        &mut self,
        target: PageHostDebuggerValueTarget,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::GetDebuggerValueSnapshot { target })
    }

    fn debugger_static_scope_relation(
        &mut self,
        target: PageHostDebuggerStaticScopeTarget,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::DescribeDebuggerStaticScopeRelation {
            target: Box::new(target),
        })
    }

    fn arm_debugger_nested_safe_point_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ArmDebuggerNestedSafePointBreakpoint {
            tab_id,
            document_generation,
            safe_point,
        })
    }

    fn step_debugger_nested_instruction(
        &mut self,
        frame: PageHostDebuggerFrame,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::StepDebuggerNestedInstruction { frame })
    }

    fn resume_debugger_nested_execution(
        &mut self,
        frame: PageHostDebuggerFrame,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ResumeDebuggerNestedExecution { frame })
    }

    fn arm_debugger_linked_nested_safe_point_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        entry_program: PageHostDebuggerProgram,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        self.request(
            PageHostRequest::ArmDebuggerLinkedNestedSafePointBreakpoint {
                tab_id,
                document_generation,
                entry_program,
                safe_point,
            },
        )
    }

    fn debugger_linked_stack_snapshot(
        &mut self,
        frame: PageHostDebuggerLinkedFrame,
        max_scope_entries: u32,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::GetDebuggerLinkedStackSnapshot {
            frame,
            max_scope_entries,
        })
    }

    fn debugger_linked_stack_spans(
        &mut self,
        frame: PageHostDebuggerLinkedFrame,
        expected_stack: PageHostDebuggerLinkedStackSnapshot,
        sources: [PageHostDebuggerLinkedSource; 2],
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::DescribeDebuggerLinkedStackSpans {
            frame,
            expected_stack,
            sources,
        })
    }

    fn resume_debugger_linked_nested_execution(
        &mut self,
        frame: PageHostDebuggerLinkedFrame,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ResumeDebuggerLinkedNestedExecution { frame })
    }

    fn arm_debugger_root_safe_point_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id,
            document_generation,
            safe_point,
        })
    }

    fn debugger_execution_state(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::GetDebuggerExecutionState {
            tab_id,
            document_generation,
            program,
        })
    }

    fn resume_debugger_execution(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::ResumeDebuggerExecution {
            tab_id,
            document_generation,
            program,
        })
    }

    fn debugger_stepping_available(&self) -> bool {
        true
    }

    fn debugger_bluets_source_span_step_available(&self) -> bool {
        true
    }

    fn step_debugger_root_instruction(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::StepDebuggerRootInstruction {
            tab_id,
            document_generation,
            program,
        })
    }

    fn step_debugger_bluets_source_span(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        metadata: PageHostDebuggerMetadataHandle,
        source_id: u32,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::StepDebuggerBlueTsSourceSpan {
            tab_id,
            document_generation,
            metadata,
            source_id,
            safe_point,
        })
    }

    fn advance_debugger_execution(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        self.request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id,
            document_generation,
        })
    }

    fn advance_debugger_execution_with_script_pump(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        pump: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<PageHostReply> {
        self.request_while_pumping_script(
            PageHostRequest::AdvanceDebuggerExecution {
                tab_id,
                document_generation,
            },
            pump,
        )
    }
}
