// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Generation-bound retention of static BlueTS debugger metadata.
//!
//! The bridge never conflates these static types and symbols with a BlueJS
//! runtime value. It retains no TypeScript source text, and a caller must use
//! the native debugger work for frames, scopes, values, pause mechanics, and
//! breakpoint execution.

use super::*;

/// Per-live-program retention bounds for static direct-bridge metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectDebugRetentionLimits {
    pub max_programs: usize,
    pub max_sources_per_program: usize,
    pub max_symbols_per_program: usize,
    pub max_types_per_program: usize,
    pub max_lowering_spans_per_program: usize,
}

impl Default for DirectDebugRetentionLimits {
    fn default() -> Self {
        Self {
            max_programs: 64,
            max_sources_per_program: 4_096,
            max_symbols_per_program: 65_536,
            max_types_per_program: 4_096,
            max_lowering_spans_per_program: 65_536,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RetainedBreakpointSpan {
    source: SourceSpan,
    safe_point: DirectSafePointBinding,
}

/// Static data retained for one live direct-program generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedDirectDebugInfo {
    handle: bluejs::BlueJsProgramHandle,
    static_info: BlueTsDebugInfo,
    safe_point_map: BlueTsSafePointMapV1,
    breakpoint_spans: Vec<RetainedBreakpointSpan>,
}

impl RetainedDirectDebugInfo {
    /// The exact live generation that owns this static metadata.
    pub fn handle(&self) -> bluejs::BlueJsProgramHandle {
        self.handle
    }

    /// Compiler-produced TypeScript metadata. This is not a BlueJS runtime
    /// scope and must not be displayed as the runtime type of a value.
    pub fn static_info(&self) -> &BlueTsDebugInfo {
        &self.static_info
    }

    /// The exact safe-point map paired with this static compilation.
    pub fn safe_point_map(&self) -> &BlueTsSafePointMapV1 {
        &self.safe_point_map
    }

    /// Resolves the same compiler-lowered source span as the original live
    /// attachment, including an explicit unbound result. Retaining only spans
    /// and verified safe points avoids retaining AST nodes or source text.
    pub fn breakpoint_at_or_after(
        &self,
        source: &str,
        source_byte: usize,
    ) -> DirectSafePointBinding {
        resolve_breakpoint_at_or_after(
            self.breakpoint_spans
                .iter()
                .map(|span| (&span.source, span.safe_point)),
            source,
            source_byte,
        )
    }
}

/// Retains static metadata only while its matching BlueJS generation remains
/// live. Handles are opaque; a generation can never be reinterpreted for a
/// replacement program.
#[derive(Debug, Clone)]
pub struct DirectDebugRegistry {
    limits: DirectDebugRetentionLimits,
    programs: BTreeMap<bluejs::BlueJsProgramGeneration, RetainedDirectDebugInfo>,
}

impl Default for DirectDebugRegistry {
    fn default() -> Self {
        Self::new(DirectDebugRetentionLimits::default())
    }
}

impl DirectDebugRegistry {
    /// Creates a metadata registry with explicit retention limits.
    pub fn new(limits: DirectDebugRetentionLimits) -> Self {
        Self {
            limits,
            programs: BTreeMap::new(),
        }
    }

    /// Number of retained live-generation records before pruning.
    pub fn len(&self) -> usize {
        self.programs.len()
    }

    /// Whether no metadata is retained.
    pub fn is_empty(&self) -> bool {
        self.programs.is_empty()
    }

    /// Looks up static metadata for an exact live generation. The BlueJS
    /// registry check is deliberately first, so stale or invalid handles fail
    /// rather than discovering a coincidental retained map entry.
    pub fn get<'a>(
        &'a self,
        registry: &bluejs::BlueJsProgramRegistry,
        handle: bluejs::BlueJsProgramHandle,
    ) -> Result<&'a RetainedDirectDebugInfo, DirectDebugAttachmentError> {
        registry
            .get(handle)
            .map_err(DirectDebugAttachmentError::BlueJsProgram)?;
        self.programs
            .get(&handle.generation())
            .filter(|retained| retained.handle == handle)
            .ok_or(DirectDebugAttachmentError::MetadataUnavailable)
    }

    /// Removes the exact generation record. It is safe to call after BlueJS
    /// has already invalidated the program.
    pub fn forget(&mut self, handle: bluejs::BlueJsProgramHandle) -> bool {
        self.programs.remove(&handle.generation()).is_some()
    }

    /// Drops records whose BlueJS generations are no longer live. Hosts call
    /// this after navigation, reload, cache eviction, or hibernation.
    pub fn prune_invalid(&mut self, registry: &bluejs::BlueJsProgramRegistry) -> usize {
        let before = self.programs.len();
        self.programs
            .retain(|_, retained| registry.get(retained.handle).is_ok());
        before - self.programs.len()
    }

    pub(crate) fn retain(
        &mut self,
        registry: &bluejs::BlueJsProgramRegistry,
        attachment: &DirectProgramAttachment,
        language_version: &str,
        compiler_options_fingerprint: &str,
        sources: &[BridgeSource],
        static_info: &BlueTsDebugInfo,
    ) -> Result<(), DirectDebugAttachmentError> {
        registry
            .get(attachment.handle)
            .map_err(DirectDebugAttachmentError::BlueJsProgram)?;
        attachment
            .safe_point_map
            .validate_against(registry, attachment.handle)
            .map_err(|error| DirectDebugAttachmentError::SafePointMap(error.to_string()))?;
        if static_info.language_version != language_version {
            return Err(DirectDebugAttachmentError::LanguageVersionMismatch);
        }
        if static_info.compiler_options_hash != compiler_options_fingerprint {
            return Err(DirectDebugAttachmentError::CompilerOptionsMismatch);
        }
        if !source_sets_match(&static_info.sources, sources) {
            return Err(DirectDebugAttachmentError::SourceSetMismatch);
        }
        enforce_limit(
            "sources",
            static_info.sources.len(),
            self.limits.max_sources_per_program,
        )?;
        enforce_limit(
            "symbols",
            static_info.symbols.len(),
            self.limits.max_symbols_per_program,
        )?;
        enforce_limit(
            "types",
            static_info.types.len(),
            self.limits.max_types_per_program,
        )?;
        enforce_limit(
            "lowering spans",
            attachment.provenance.len(),
            self.limits.max_lowering_spans_per_program,
        )?;
        let rebuilt_map = build_safe_point_map(
            attachment.handle,
            compiler_options_fingerprint,
            sources,
            &attachment.provenance,
        )
        .map_err(|error| DirectDebugAttachmentError::SafePointMap(error.to_string()))?;
        if rebuilt_map != attachment.safe_point_map {
            return Err(DirectDebugAttachmentError::SafePointMap(
                "retained lowering spans do not match the verified safe-point map".to_string(),
            ));
        }

        let breakpoint_spans = attachment
            .provenance
            .iter()
            .map(|provenance| RetainedBreakpointSpan {
                source: provenance.source.clone(),
                safe_point: provenance.safe_point,
            })
            .collect::<Vec<_>>();

        let generation = attachment.handle.generation();
        if let Some(existing) = self.programs.get(&generation) {
            let replacement = RetainedDirectDebugInfo {
                handle: attachment.handle,
                static_info: static_info.clone(),
                safe_point_map: attachment.safe_point_map.clone(),
                breakpoint_spans: breakpoint_spans.clone(),
            };
            if existing == &replacement {
                return Ok(());
            }
            return Err(DirectDebugAttachmentError::GenerationAlreadyAttached);
        }
        enforce_limit(
            "programs",
            self.programs.len() + 1,
            self.limits.max_programs,
        )?;
        self.programs.insert(
            generation,
            RetainedDirectDebugInfo {
                handle: attachment.handle,
                static_info: static_info.clone(),
                safe_point_map: attachment.safe_point_map.clone(),
                breakpoint_spans,
            },
        );
        Ok(())
    }
}

/// A rejected attempt to associate static TypeScript metadata with a live
/// BlueJS generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectDebugAttachmentError {
    BlueJsProgram(bluejs::BlueJsProgramDebugError),
    SafePointMap(String),
    LanguageVersionMismatch,
    CompilerOptionsMismatch,
    SourceSetMismatch,
    RetentionLimit {
        resource: &'static str,
        limit: usize,
    },
    GenerationAlreadyAttached,
    MetadataUnavailable,
}

impl fmt::Display for DirectDebugAttachmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlueJsProgram(error) => {
                write!(formatter, "BlueJS program is unavailable: {error}")
            }
            Self::SafePointMap(error) => write!(formatter, "safe-point map is invalid: {error}"),
            Self::LanguageVersionMismatch => {
                formatter.write_str("BlueTS language version does not match direct program")
            }
            Self::CompilerOptionsMismatch => formatter
                .write_str("BlueTS compiler-options fingerprint does not match direct program"),
            Self::SourceSetMismatch => {
                formatter.write_str("BlueTS source identity set does not match direct program")
            }
            Self::RetentionLimit { resource, limit } => {
                write!(
                    formatter,
                    "direct debug {resource} exceeds retention limit {limit}"
                )
            }
            Self::GenerationAlreadyAttached => {
                formatter.write_str("BlueJS generation already has different static debug metadata")
            }
            Self::MetadataUnavailable => {
                formatter.write_str("no static TypeScript metadata is retained for this generation")
            }
        }
    }
}

impl std::error::Error for DirectDebugAttachmentError {}

fn enforce_limit(
    resource: &'static str,
    observed: usize,
    limit: usize,
) -> Result<(), DirectDebugAttachmentError> {
    if observed > limit {
        return Err(DirectDebugAttachmentError::RetentionLimit { resource, limit });
    }
    Ok(())
}

fn source_sets_match(
    debug_sources: &[blueice_bluets::DebugSource],
    bridge_sources: &[BridgeSource],
) -> bool {
    let debug = debug_sources
        .iter()
        .map(|source| (source.module.as_str(), source.content_hash.as_str()))
        .collect::<BTreeMap<_, _>>();
    let bridge = bridge_sources
        .iter()
        .map(|source| (source.module.as_str(), source.content_hash.as_str()))
        .collect::<BTreeMap<_, _>>();
    debug.len() == debug_sources.len() && bridge.len() == bridge_sources.len() && debug == bridge
}
