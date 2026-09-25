// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded, owner-only bootstrap for a sealed compiler project catalog.
//!
//! This is deliberately separate from [`crate::compiler`]: source text may
//! cross only a launcher's inherited, one-shot core stdin before any listener
//! is bound. No compiler IPC or MCP request can carry this structure or ask a
//! running core to register another project. Canonical identities and exact
//! import edges are selected by the trusted owner, never resolved here.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use std::io::{self, Read, Write};

/// Version two adds explicit, default-denied per-project compiler visibility.
/// Version one is rejected rather than silently changing its public inventory.
pub const COMPILER_CATALOG_BOOTSTRAP_VERSION: u32 = 2;
pub const MAX_COMPILER_CATALOG_FRAME_BYTES: usize = 16 * 1_024 * 1_024;
pub const MAX_COMPILER_CATALOG_PROJECTS: usize = 128;
pub const MAX_COMPILER_CATALOG_MODULES_PER_PROJECT: usize = 256;
pub const MAX_COMPILER_CATALOG_RESOLUTIONS_PER_PROJECT: usize = 2_048;
pub const MAX_COMPILER_CATALOG_AMBIENT_MODULES_PER_PROJECT: usize = 32;
pub const MAX_COMPILER_CATALOG_IDENTITY_BYTES: usize = 4_096;
pub const MAX_COMPILER_CATALOG_SOURCE_BYTES: usize = 1_024 * 1_024;

/// One complete owner-selected startup catalog. `Debug` is intentionally
/// redacted because source bytes must not enter launcher/core diagnostics.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerCatalogBootstrap {
    pub version: u32,
    pub projects: Vec<CompilerCatalogProject>,
}

impl fmt::Debug for CompilerCatalogBootstrap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompilerCatalogBootstrap")
            .field("version", &self.version)
            .field("project_count", &self.projects.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerCatalogProject {
    pub canonical_project_root: String,
    pub canonical_config_root: String,
    pub canonical_output_root: String,
    pub entry_module: String,
    pub modules: Vec<CompilerCatalogModule>,
    /// Public compiler/MCP visibility is owner-selected and default-denied.
    /// Registration and visibility are separate: an unexposed project still
    /// belongs to the sealed core catalog, but no compiler stream can receive
    /// its opaque ID or use a guessed ID to query it.
    #[serde(default)]
    pub expose_to_compiler_ipc: bool,
    #[serde(default)]
    pub resolutions: Vec<CompilerCatalogResolution>,
    #[serde(default)]
    pub options: CompilerCatalogOptions,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerCatalogModule {
    pub canonical_id: String,
    pub text: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerCatalogResolution {
    pub from_module: String,
    pub specifier: String,
    pub target_module: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CompilerCatalogTarget {
    Es2020,
    #[default]
    Es2022,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CompilerCatalogRuntimePolicy {
    TranspileOnly,
    #[default]
    Checked,
    StrictRuntime,
}

/// Only compiler semantics are selectable here. Resource ceilings remain
/// core-selected defaults; no manifest field grants filesystem or write I/O.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CompilerCatalogOptions {
    pub target: CompilerCatalogTarget,
    pub runtime_policy: CompilerCatalogRuntimePolicy,
    pub source_map: bool,
    pub declaration: bool,
    pub resolver_fingerprint: String,
    pub ambient_declaration_modules: Vec<CompilerCatalogModule>,
    pub require_declared_global_calls: bool,
}

impl Default for CompilerCatalogOptions {
    fn default() -> Self {
        Self {
            target: CompilerCatalogTarget::Es2022,
            runtime_policy: CompilerCatalogRuntimePolicy::Checked,
            source_map: false,
            declaration: false,
            resolver_fingerprint: "sealed-catalog-v1".to_string(),
            ambient_declaration_modules: Vec::new(),
            require_declared_global_calls: false,
        }
    }
}

impl CompilerCatalogBootstrap {
    pub fn validate(&self) -> io::Result<()> {
        if self.version != COMPILER_CATALOG_BOOTSTRAP_VERSION {
            return Err(invalid("unsupported compiler catalog bootstrap version"));
        }
        if self.projects.is_empty() || self.projects.len() > MAX_COMPILER_CATALOG_PROJECTS {
            return Err(invalid(
                "compiler catalog project count is outside its fixed bound",
            ));
        }
        let mut decoded_bytes = 0usize;
        let mut project_roots = BTreeSet::new();
        let mut input_identities = Vec::new();
        let mut output_roots = Vec::new();
        for project in &self.projects {
            if !project_roots.insert(project.canonical_project_root.as_str()) {
                return Err(invalid("duplicate compiler catalog project root"));
            }
            for identity in [
                &project.canonical_project_root,
                &project.canonical_config_root,
                &project.canonical_output_root,
                &project.entry_module,
                &project.options.resolver_fingerprint,
            ] {
                validate_identity(identity, &mut decoded_bytes)?;
            }
            for identity in [
                &project.canonical_project_root,
                &project.canonical_config_root,
                &project.canonical_output_root,
                &project.entry_module,
            ] {
                validate_path_identity(identity)?;
            }
            if !is_descendant(
                &project.canonical_config_root,
                &project.canonical_project_root,
            ) || !is_descendant(&project.entry_module, &project.canonical_project_root)
            {
                return Err(invalid(
                    "compiler catalog config or entry escapes its project root",
                ));
            }
            input_identities.push(project.canonical_config_root.as_str());
            output_roots.push(project.canonical_output_root.as_str());
            if project.modules.is_empty()
                || project.modules.len() > MAX_COMPILER_CATALOG_MODULES_PER_PROJECT
                || project.resolutions.len() > MAX_COMPILER_CATALOG_RESOLUTIONS_PER_PROJECT
                || project.options.ambient_declaration_modules.len()
                    > MAX_COMPILER_CATALOG_AMBIENT_MODULES_PER_PROJECT
            {
                return Err(invalid(
                    "compiler catalog graph exceeds its fixed count bounds",
                ));
            }
            let mut module_ids = BTreeSet::new();
            for module in &project.modules {
                validate_identity(&module.canonical_id, &mut decoded_bytes)?;
                validate_path_identity(&module.canonical_id)?;
                if !is_descendant(&module.canonical_id, &project.canonical_project_root) {
                    return Err(invalid("compiler catalog module escapes its project root"));
                }
                if !module_ids.insert(module.canonical_id.as_str()) {
                    return Err(invalid("duplicate compiler catalog module"));
                }
                if module.text.len() > MAX_COMPILER_CATALOG_SOURCE_BYTES {
                    return Err(invalid(
                        "compiler catalog source exceeds its fixed byte bound",
                    ));
                }
                add_bytes(&mut decoded_bytes, module.text.len())?;
                input_identities.push(module.canonical_id.as_str());
            }
            if !module_ids.contains(project.entry_module.as_str()) {
                return Err(invalid("compiler catalog entry module is missing"));
            }
            let mut ambient_ids = BTreeSet::new();
            for module in &project.options.ambient_declaration_modules {
                validate_identity(&module.canonical_id, &mut decoded_bytes)?;
                validate_path_identity(&module.canonical_id)?;
                if module_ids.contains(module.canonical_id.as_str())
                    || !ambient_ids.insert(module.canonical_id.as_str())
                {
                    return Err(invalid("duplicate compiler catalog ambient module"));
                }
                if module.text.len() > MAX_COMPILER_CATALOG_SOURCE_BYTES {
                    return Err(invalid(
                        "compiler catalog source exceeds its fixed byte bound",
                    ));
                }
                add_bytes(&mut decoded_bytes, module.text.len())?;
                input_identities.push(module.canonical_id.as_str());
            }
            let mut resolution_keys = BTreeSet::new();
            for resolution in &project.resolutions {
                validate_identity(&resolution.from_module, &mut decoded_bytes)?;
                validate_identity(&resolution.specifier, &mut decoded_bytes)?;
                validate_identity(&resolution.target_module, &mut decoded_bytes)?;
                if !module_ids.contains(resolution.from_module.as_str())
                    || !module_ids.contains(resolution.target_module.as_str())
                {
                    return Err(invalid(
                        "compiler catalog resolution refers to a missing module",
                    ));
                }
                if !resolution_keys.insert((
                    resolution.from_module.as_str(),
                    resolution.specifier.as_str(),
                )) {
                    return Err(invalid("duplicate compiler catalog resolution"));
                }
            }
        }
        if output_roots.iter().any(|output| {
            input_identities
                .iter()
                .any(|input| paths_overlap(output, input))
        }) || output_roots.iter().enumerate().any(|(index, output)| {
            output_roots[index + 1..]
                .iter()
                .any(|other| paths_overlap(output, other))
        }) {
            return Err(invalid(
                "compiler catalog output overlaps an input or another output",
            ));
        }
        Ok(())
    }

    /// Parses a launcher-owner file before spawning a child. Unknown fields,
    /// excess bytes, malformed graphs, and unsupported versions fail closed.
    pub fn from_json_slice(bytes: &[u8]) -> io::Result<Self> {
        if bytes.is_empty() || bytes.len() > MAX_COMPILER_CATALOG_FRAME_BYTES {
            return Err(invalid(
                "compiler catalog frame exceeds its fixed byte bound",
            ));
        }
        let catalog: Self = serde_json::from_slice(bytes)
            .map_err(|_| invalid("invalid compiler catalog bootstrap JSON"))?;
        catalog.validate()?;
        Ok(catalog)
    }
}

/// Writes exactly one capped, versioned catalog to the inherited bootstrap
/// pipe. The public compiler socket never uses this framing.
pub fn write_compiler_catalog<W: Write>(
    writer: &mut W,
    catalog: &CompilerCatalogBootstrap,
) -> io::Result<()> {
    catalog.validate()?;
    let mut payload = CappedPayload::default();
    serde_json::to_writer(&mut payload, catalog)
        .map_err(|_| invalid("compiler catalog JSON exceeds its fixed byte bound"))?;
    let length = u32::try_from(payload.0.len())
        .map_err(|_| invalid("compiler catalog frame exceeds its fixed byte bound"))?;
    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(&payload.0)
}

/// Reads one catalog before any core listener is bound. A bad length is
/// rejected before allocation; the sender must close the pipe after writing
/// so trailing bytes cannot be silently treated as another registration.
pub fn read_compiler_catalog<R: Read>(reader: &mut R) -> io::Result<CompilerCatalogBootstrap> {
    let mut length = [0u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_COMPILER_CATALOG_FRAME_BYTES {
        return Err(invalid(
            "compiler catalog frame exceeds its fixed byte bound",
        ));
    }
    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload)?;
    let mut trailing = [0u8; 1];
    if reader.read(&mut trailing)? != 0 {
        return Err(invalid("compiler catalog bootstrap contains trailing data"));
    }
    CompilerCatalogBootstrap::from_json_slice(&payload)
}

#[derive(Default)]
struct CappedPayload(Vec<u8>);

impl Write for CappedPayload {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_COMPILER_CATALOG_FRAME_BYTES {
            return Err(invalid(
                "compiler catalog frame exceeds its fixed byte bound",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn validate_identity(value: &str, decoded_bytes: &mut usize) -> io::Result<()> {
    if value.is_empty() || value.contains('\0') || value.len() > MAX_COMPILER_CATALOG_IDENTITY_BYTES
    {
        return Err(invalid(
            "compiler catalog identity is malformed or over budget",
        ));
    }
    add_bytes(decoded_bytes, value.len())
}

/// Treat owner-supplied path identities as opaque names, but reject lexical
/// aliases before their equality or containment can authorize future output.
/// This does not resolve a host filesystem path or grant write access.
fn validate_path_identity(value: &str) -> io::Result<()> {
    let path = value.split_once("://").map_or(value, |(_, path)| path);
    if path.is_empty()
        || path.ends_with('/')
        || path.contains("//")
        || value.contains(['\\', '?', '#', '%'])
        || value.chars().any(char::is_control)
        || path
            .split('/')
            .any(|component| matches!(component, "." | ".."))
    {
        return Err(invalid("compiler catalog path identity is not canonical"));
    }
    Ok(())
}

fn is_descendant(path: &str, root: &str) -> bool {
    path.strip_prefix(root)
        .is_some_and(|remainder| remainder.starts_with('/') && remainder.len() > 1)
}

fn paths_overlap(left: &str, right: &str) -> bool {
    left == right || is_descendant(left, right) || is_descendant(right, left)
}

fn add_bytes(total: &mut usize, bytes: usize) -> io::Result<()> {
    *total = total
        .checked_add(bytes)
        .filter(|sum| *sum <= MAX_COMPILER_CATALOG_FRAME_BYTES)
        .ok_or_else(|| invalid("compiler catalog decoded data exceeds its fixed byte bound"))?;
    Ok(())
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_catalog() -> CompilerCatalogBootstrap {
        CompilerCatalogBootstrap {
            version: COMPILER_CATALOG_BOOTSTRAP_VERSION,
            projects: vec![CompilerCatalogProject {
                canonical_project_root: "project:///app".to_string(),
                canonical_config_root: "project:///app/blue-ts.json".to_string(),
                canonical_output_root: "project:///dist".to_string(),
                entry_module: "project:///app/main.ts".to_string(),
                modules: vec![CompilerCatalogModule {
                    canonical_id: "project:///app/main.ts".to_string(),
                    text: "export const value: number = 42;".to_string(),
                }],
                expose_to_compiler_ipc: false,
                resolutions: Vec::new(),
                options: CompilerCatalogOptions::default(),
            }],
        }
    }

    #[test]
    fn owner_catalog_round_trips_without_debug_source_disclosure() {
        let catalog = fixture_catalog();
        let mut bytes = Vec::new();
        write_compiler_catalog(&mut bytes, &catalog).unwrap();
        assert_eq!(
            read_compiler_catalog(&mut bytes.as_slice()).unwrap(),
            catalog
        );
        assert!(!format!("{catalog:?}").contains("export const"));
    }

    #[test]
    fn omitted_project_exposure_defaults_to_deny() {
        let mut value = serde_json::to_value(fixture_catalog()).unwrap();
        value["projects"][0]
            .as_object_mut()
            .unwrap()
            .remove("expose_to_compiler_ipc");
        let decoded =
            CompilerCatalogBootstrap::from_json_slice(&serde_json::to_vec(&value).unwrap())
                .unwrap();
        assert!(!decoded.projects[0].expose_to_compiler_ipc);
    }

    #[test]
    fn malformed_catalogs_fail_before_registration_or_allocation() {
        let mut catalog = fixture_catalog();
        catalog.version += 1;
        assert!(catalog.validate().is_err());
        catalog.version = 1;
        assert!(catalog.validate().is_err());
        catalog = fixture_catalog();
        catalog.projects[0].modules[0].text = "x".repeat(MAX_COMPILER_CATALOG_SOURCE_BYTES + 1);
        assert!(catalog.validate().is_err());
        catalog = fixture_catalog();
        catalog.projects[0].canonical_project_root.clear();
        assert!(catalog.validate().is_err());
        catalog = fixture_catalog();
        catalog.projects[0].entry_module = "project:///missing.ts".to_string();
        assert!(catalog.validate().is_err());
        catalog = fixture_catalog();
        let duplicate_module = catalog.projects[0].modules[0].clone();
        catalog.projects[0].modules.push(duplicate_module);
        assert!(catalog.validate().is_err());
        catalog = fixture_catalog();
        catalog.projects.push(catalog.projects[0].clone());
        assert!(catalog.validate().is_err());
        catalog = fixture_catalog();
        catalog.projects[0]
            .resolutions
            .push(CompilerCatalogResolution {
                from_module: "project:///app/main.ts".to_string(),
                specifier: "./missing".to_string(),
                target_module: "project:///app/missing.ts".to_string(),
            });
        assert!(catalog.validate().is_err());
        assert!(CompilerCatalogBootstrap::from_json_slice(
            br#"{"version":1,"projects":[],"unknown":0}"#
        )
        .is_err());
        let oversized = u32::try_from(MAX_COMPILER_CATALOG_FRAME_BYTES + 1).unwrap();
        assert!(read_compiler_catalog(&mut oversized.to_le_bytes().as_slice()).is_err());
        let mut encoded = Vec::new();
        write_compiler_catalog(&mut encoded, &fixture_catalog()).unwrap();
        encoded.push(1);
        assert!(read_compiler_catalog(&mut encoded.as_slice()).is_err());
    }

    #[test]
    fn owner_catalog_rejects_aliased_inputs_and_output_collisions() {
        for aliased_root in [
            "project:///app/../app",
            "project:///app/./nested",
            "project:////app",
            "project:///app/",
            "project:///app%2fhidden",
            "project:///app\\hidden",
            "project:///app?version=1",
        ] {
            let mut catalog = fixture_catalog();
            catalog.projects[0].canonical_project_root = aliased_root.into();
            assert!(catalog.validate().is_err(), "accepted {aliased_root}");
        }

        let mut catalog = fixture_catalog();
        catalog.projects[0].canonical_config_root = "project:///outside/blue-ts.json".into();
        assert!(catalog.validate().is_err());

        let mut catalog = fixture_catalog();
        catalog.projects[0].modules.push(CompilerCatalogModule {
            canonical_id: "project:///outside/extra.ts".into(),
            text: "export const extra = 1;".into(),
        });
        assert!(catalog.validate().is_err());

        let mut catalog = fixture_catalog();
        catalog.projects[0].canonical_output_root = "project:///app".into();
        assert!(catalog.validate().is_err());

        let mut catalog = fixture_catalog();
        catalog.projects[0].canonical_output_root = "project:///app/main.ts".into();
        assert!(catalog.validate().is_err());

        let mut catalog = fixture_catalog();
        catalog.projects[0].canonical_output_root = "project:///app/dist".into();
        assert!(catalog.validate().is_ok());

        let mut catalog = fixture_catalog();
        catalog.projects[0].canonical_output_root = "project:///app2".into();
        assert!(
            catalog.validate().is_ok(),
            "root containment must be segment-aware"
        );

        let mut catalog = fixture_catalog();
        let mut second = catalog.projects[0].clone();
        second.canonical_project_root = "project:///second".into();
        second.canonical_config_root = "project:///second/blue-ts.json".into();
        second.entry_module = "project:///second/main.ts".into();
        second.modules[0].canonical_id = second.entry_module.clone();
        second.canonical_output_root = "project:///app".into();
        catalog.projects.push(second);
        assert!(catalog.validate().is_err());

        catalog.projects[1].canonical_output_root = "project:///dist/second".into();
        assert!(catalog.validate().is_err());

        catalog.projects[1].canonical_output_root = "project:///second-dist".into();
        assert!(catalog.validate().is_ok());
    }
}
