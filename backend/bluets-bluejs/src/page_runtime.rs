// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct BlueTS classic-script admission into a BlueJS page realm.
//!
//! This is a host-neutral seam, not a page loader: callers still authorize the
//! tab, origin, source graph, host-typing profile, and script kind. It ensures
//! that a direct artifact uses the page runtime's ownership/accounting rather
//! than creating an unrelated program registry, then attaches only to the
//! exact just-installed generation.

use super::*;

impl DirectScript {
    /// Installs this checked classic TypeScript artifact in one already-open
    /// BlueJS page realm and attaches its verified lowering provenance to the
    /// resulting generation. An attachment failure discards the program before
    /// it can be executed, releasing the realm's program/bytecode charge.
    pub fn attach_in_page_realm(
        &self,
        runtime: &mut bluejs::BlueJsPageRuntime,
        tab_id: u64,
        origin: &bluejs::BlueJsPageOrigin,
    ) -> Result<DirectProgramAttachment, BridgeError> {
        let source = source_identity(&self.sources)?;
        let handle = runtime
            .install_program(tab_id, origin, source, &self.program)
            .map_err(BridgeError::PageRuntime)?;
        match self.attach_existing_in(runtime.program_registry(), handle) {
            Ok(attachment) => Ok(attachment),
            Err(error) => {
                runtime
                    .discard_program(tab_id, handle)
                    .map_err(BridgeError::PageRuntime)?;
                Err(error)
            }
        }
    }

    /// Like [`Self::attach_in_page_realm`], additionally retaining static
    /// BlueTS metadata for that exact live generation. The metadata registry
    /// stores no runtime values. Any failed retention discards the just-created
    /// page-realm program so it cannot run without the requested pairing.
    pub fn attach_debug_in_page_realm(
        &self,
        runtime: &mut bluejs::BlueJsPageRuntime,
        tab_id: u64,
        origin: &bluejs::BlueJsPageOrigin,
        debug_registry: &mut DirectDebugRegistry,
    ) -> Result<DirectProgramAttachment, BridgeError> {
        let attachment = self.attach_in_page_realm(runtime, tab_id, origin)?;
        if let Err(error) = debug_registry.retain(
            runtime.program_registry(),
            &attachment,
            &self.language_version,
            &self.compiler_options_fingerprint,
            &self.sources,
            &self.debug_info,
        ) {
            runtime
                .discard_program(tab_id, attachment.handle)
                .map_err(BridgeError::PageRuntime)?;
            return Err(BridgeError::DebugAttachment(error));
        }
        Ok(attachment)
    }
}
