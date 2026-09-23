// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Install-time extension manifest loading and identity derivation.
//!
//! A connected extension never gets to select a friendly, author-written
//! identity as its authority. Instead the host reads a strict package manifest
//! plus its declared WASM module, validates both, and derives a domain-separated
//! SHA-256 identity from their exact installed bytes. That exact identity is
//! what the registry grants capabilities to and what a later protocol handshake
//! must present. Binding that handshake to the host-spawned package is a later
//! process-authentication task; a hash-derived ID alone is not credentials.
//! There is no WASM runtime in this slice: validating the module's
//! header establishes that the package is shaped for the selected format without
//! pretending it has executed extension code.

use crate::{
    CAPABILITY_DOM_READ, CAPABILITY_DOM_WRITE, CAPABILITY_NETWORK_INTERCEPT, ExtensionRegistry,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// The only manifest API version this Phase 9 host currently accepts.
pub const MANIFEST_API_VERSION: u32 = 1;

const IDENTITY_DOMAIN: &[u8] = b"blueice-extension-identity-v1\0";
const WASM_MAGIC: &[u8; 4] = b"\0asm";
const WASM_VERSION_1: &[u8; 4] = &[1, 0, 0, 0];

/// The install-time capability classes described by Phase 9. Only `declared`
/// grants authority immediately; optional and runtime-ephemeral declarations
/// are retained for a later consent/gesture flow but cannot authorize requests
/// on their own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestCapabilities {
    declared: BTreeSet<String>,
    optional: BTreeSet<String>,
    runtime_ephemeral: BTreeSet<String>,
}

impl ManifestCapabilities {
    /// Capabilities persistently granted when the package is installed.
    pub fn declared(&self) -> &BTreeSet<String> {
        &self.declared
    }

    /// Capabilities declared as available but not install-time-granted.
    pub fn optional(&self) -> &BTreeSet<String> {
        &self.optional
    }

    /// Capabilities that require a future user-gesture flow.
    pub fn runtime_ephemeral(&self) -> &BTreeSet<String> {
        &self.runtime_ephemeral
    }
}

/// The validated, declarative contents of `extension.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionManifest {
    name: String,
    version: String,
    blueice_api_version: u32,
    entry_point: PathBuf,
    capabilities: ManifestCapabilities,
}

impl ExtensionManifest {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn blueice_api_version(&self) -> u32 {
        self.blueice_api_version
    }

    /// A package-root-relative path to the validated WASM module.
    pub fn entry_point(&self) -> &Path {
        &self.entry_point
    }

    pub fn capabilities(&self) -> &ManifestCapabilities {
        &self.capabilities
    }
}

/// A validated extension package, ready for installation into an
/// [`ExtensionRegistry`]. The identity is derived by the host, never accepted
/// from a manifest field or a protocol peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledExtension {
    manifest: ExtensionManifest,
    manifest_path: PathBuf,
    wasm_path: PathBuf,
    extension_id: String,
}

impl InstalledExtension {
    pub fn manifest(&self) -> &ExtensionManifest {
        &self.manifest
    }

    pub fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    pub fn wasm_path(&self) -> &Path {
        &self.wasm_path
    }

    /// The `sha256:<hex>` identity that protocol peers must use in `Hello`.
    pub fn extension_id(&self) -> &str {
        &self.extension_id
    }
}

/// Failure to parse or validate an installable extension package.
#[derive(Debug)]
pub enum ManifestError {
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse(serde_json::Error),
    Invalid(String),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => write!(f, "could not read {}: {source}", path.display()),
            Self::Parse(source) => write!(f, "invalid extension manifest JSON: {source}"),
            Self::Invalid(message) => f.write_str(message),
        }
    }
}

impl Error for ManifestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse(source) => Some(source),
            Self::Invalid(_) => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    name: String,
    version: String,
    blueice_api_version: u32,
    entry_point: String,
    #[serde(default)]
    capabilities: RawCapabilities,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCapabilities {
    #[serde(default)]
    declared: Vec<String>,
    #[serde(default)]
    optional: Vec<String>,
    #[serde(default, alias = "runtime-ephemeral")]
    runtime_ephemeral: Vec<String>,
}

/// Loads, parses, and validates an `extension.json` package manifest and its
/// relative WASM entry point. The entry point cannot escape the manifest's
/// directory through `..`, an absolute path, or a symlink.
pub fn load_installed_extension(
    manifest_path: impl AsRef<Path>,
) -> Result<InstalledExtension, ManifestError> {
    let manifest_path = manifest_path.as_ref();
    let manifest_bytes = read_file(manifest_path)?;
    let raw: RawManifest = serde_json::from_slice(&manifest_bytes).map_err(ManifestError::Parse)?;
    let manifest = validate_manifest(raw)?;

    let package_root = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let canonical_root = fs::canonicalize(package_root).map_err(|source| ManifestError::Read {
        path: package_root.to_path_buf(),
        source,
    })?;
    let candidate_wasm_path = canonical_root.join(manifest.entry_point());
    let wasm_path =
        fs::canonicalize(&candidate_wasm_path).map_err(|source| ManifestError::Read {
            path: candidate_wasm_path,
            source,
        })?;
    if !wasm_path.starts_with(&canonical_root) {
        return Err(ManifestError::Invalid(
            "extension entry_point resolves outside its package directory".to_string(),
        ));
    }
    let wasm_bytes = read_file(&wasm_path)?;
    validate_wasm_header(&wasm_bytes)?;

    let extension_id = derive_extension_id(&manifest_bytes, &wasm_bytes);
    Ok(InstalledExtension {
        manifest,
        manifest_path: manifest_path.to_path_buf(),
        wasm_path,
        extension_id,
    })
}

/// Registers all host-supported Phase 9 capability windows and grants just an
/// installed manifest's persistent `declared` capabilities to its derived
/// identity. Optional/runtime declarations intentionally add no authority.
pub fn registry_for_installed_extension(extension: &InstalledExtension) -> ExtensionRegistry {
    let mut registry = ExtensionRegistry::with_supported_capabilities();
    for capability in extension.manifest().capabilities().declared() {
        registry.grant(extension.extension_id(), capability);
    }
    registry
}

fn read_file(path: &Path) -> Result<Vec<u8>, ManifestError> {
    fs::read(path).map_err(|source| ManifestError::Read {
        path: path.to_path_buf(),
        source,
    })
}

fn validate_manifest(raw: RawManifest) -> Result<ExtensionManifest, ManifestError> {
    validate_text_field("name", &raw.name, 128)?;
    validate_text_field("version", &raw.version, 64)?;
    if raw.blueice_api_version != MANIFEST_API_VERSION {
        return Err(ManifestError::Invalid(format!(
            "unsupported blueice_api_version {}; this host supports {MANIFEST_API_VERSION}",
            raw.blueice_api_version
        )));
    }
    let entry_point = validate_entry_point(&raw.entry_point)?;
    let capabilities = validate_capabilities(raw.capabilities)?;
    Ok(ExtensionManifest {
        name: raw.name,
        version: raw.version,
        blueice_api_version: raw.blueice_api_version,
        entry_point,
        capabilities,
    })
}

fn validate_text_field(field: &str, value: &str, max_len: usize) -> Result<(), ManifestError> {
    if value.is_empty() || value.len() > max_len || value.chars().any(char::is_control) {
        return Err(ManifestError::Invalid(format!(
            "manifest {field} must be non-empty printable text no longer than {max_len} bytes"
        )));
    }
    Ok(())
}

fn validate_entry_point(value: &str) -> Result<PathBuf, ManifestError> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || path.extension().is_none_or(|extension| extension != "wasm")
    {
        return Err(ManifestError::Invalid(
            "manifest entry_point must be a package-relative .wasm file without traversal"
                .to_string(),
        ));
    }
    Ok(path.to_path_buf())
}

fn validate_capabilities(raw: RawCapabilities) -> Result<ManifestCapabilities, ManifestError> {
    let mut all = BTreeSet::new();
    let declared = validate_capability_tier("declared", raw.declared, &mut all)?;
    let optional = validate_capability_tier("optional", raw.optional, &mut all)?;
    let runtime_ephemeral =
        validate_capability_tier("runtime_ephemeral", raw.runtime_ephemeral, &mut all)?;
    Ok(ManifestCapabilities {
        declared,
        optional,
        runtime_ephemeral,
    })
}

fn validate_capability_tier(
    tier: &str,
    values: Vec<String>,
    all: &mut BTreeSet<String>,
) -> Result<BTreeSet<String>, ManifestError> {
    let mut tier_values = BTreeSet::new();
    for capability in values {
        if !is_supported_capability(&capability) {
            return Err(ManifestError::Invalid(format!(
                "manifest {tier} capability {capability:?} is not supported by this host"
            )));
        }
        if !all.insert(capability.clone()) || !tier_values.insert(capability.clone()) {
            return Err(ManifestError::Invalid(format!(
                "manifest capability {capability:?} appears more than once"
            )));
        }
    }
    Ok(tier_values)
}

fn is_supported_capability(capability: &str) -> bool {
    matches!(
        capability,
        CAPABILITY_DOM_READ | CAPABILITY_DOM_WRITE | CAPABILITY_NETWORK_INTERCEPT
    )
}

fn validate_wasm_header(bytes: &[u8]) -> Result<(), ManifestError> {
    if bytes.len() < 8 || &bytes[..4] != WASM_MAGIC || &bytes[4..8] != WASM_VERSION_1 {
        return Err(ManifestError::Invalid(
            "extension entry_point is not a WebAssembly version-1 module".to_string(),
        ));
    }
    Ok(())
}

fn derive_extension_id(manifest_bytes: &[u8], wasm_bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(IDENTITY_DOMAIN);
    for bytes in [manifest_bytes, wasm_bytes] {
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    const WASM_V1: &[u8] = b"\0asm\x01\0\0\0";

    fn temporary_package(label: &str, manifest: &str, wasm: &[u8]) -> (PathBuf, PathBuf) {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "blueice-extension-manifest-{label}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let manifest_path = root.join("extension.json");
        let wasm_path = root.join("extension.wasm");
        std::fs::write(&manifest_path, manifest).unwrap();
        std::fs::write(&wasm_path, wasm).unwrap();
        (root, manifest_path)
    }

    fn manifest(capabilities: &str) -> String {
        format!(
            r#"{{"name":"Demo","version":"1.0.0","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{capabilities}}}"#
        )
    }

    #[test]
    fn a_valid_package_derives_a_stable_sha256_identity_and_only_grants_declared_capabilities() {
        let (root, path) = temporary_package(
            "valid",
            &manifest(
                r#"{"declared":["dom:read"],"optional":["dom:write"],"runtime_ephemeral":["network:intercept"]}"#,
            ),
            WASM_V1,
        );
        let extension = load_installed_extension(&path).unwrap();
        let again = load_installed_extension(&path).unwrap();
        assert_eq!(extension.extension_id(), again.extension_id());
        assert!(extension.extension_id().starts_with("sha256:"));
        assert_eq!(extension.extension_id().len(), "sha256:".len() + 64);

        let registry = registry_for_installed_extension(&extension);
        assert!(registry.has_capability(extension.extension_id(), CAPABILITY_DOM_READ));
        assert!(!registry.has_capability(extension.extension_id(), CAPABILITY_DOM_WRITE));
        assert!(!registry.has_capability(extension.extension_id(), CAPABILITY_NETWORK_INTERCEPT));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn identity_changes_when_either_installed_artifact_changes() {
        let source = manifest(r#"{"declared":["dom:read"]}"#);
        let (root, path) = temporary_package("identity", &source, WASM_V1);
        let first = load_installed_extension(&path).unwrap();

        std::fs::write(root.join("extension.wasm"), b"\0asm\x01\0\0\0changed").unwrap();
        let module_changed = load_installed_extension(&path).unwrap();
        assert_ne!(first.extension_id(), module_changed.extension_id());

        std::fs::write(
            &path,
            manifest(r#"{"declared":["dom:read"],"optional":["dom:write"]}"#),
        )
        .unwrap();
        let manifest_changed = load_installed_extension(&path).unwrap();
        assert_ne!(
            module_changed.extension_id(),
            manifest_changed.extension_id()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_manifests_cannot_widen_authority_or_escape_the_package() {
        for (label, source, expected) in [
            (
                "unknown-capability",
                manifest(r#"{"declared":["network:exfiltrate"]}"#),
                "not supported",
            ),
            (
                "duplicate-capability",
                manifest(r#"{"declared":["dom:read"],"optional":["dom:read"]}"#),
                "appears more than once",
            ),
            (
                "traversal",
                r#"{"name":"Demo","version":"1","blueice_api_version":1,"entry_point":"../outside.wasm"}"#.to_string(),
                "package-relative",
            ),
            (
                "spoof-id-field",
                r#"{"name":"Demo","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","extension_id":"friendly-name"}"#.to_string(),
                "invalid extension manifest JSON",
            ),
        ] {
            let (root, path) = temporary_package(label, &source, WASM_V1);
            let error = load_installed_extension(&path).unwrap_err().to_string();
            assert!(error.contains(expected), "{label}: {error}");
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn invalid_wasm_header_is_rejected_before_an_identity_is_registered() {
        let (root, path) = temporary_package(
            "bad-wasm",
            &manifest(r#"{"declared":["dom:read"]}"#),
            b"not a wasm module",
        );
        assert!(
            load_installed_extension(&path)
                .unwrap_err()
                .to_string()
                .contains("not a WebAssembly")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn capability_windows_remain_separate_from_manifest_grants() {
        let (root, path) =
            temporary_package("window", &manifest(r#"{"declared":["dom:read"]}"#), WASM_V1);
        let extension = load_installed_extension(&path).unwrap();
        let registry = registry_for_installed_extension(&extension);
        assert_eq!(
            registry.unsupported_capability_version(CAPABILITY_DOM_READ, 3),
            Some(
                blueice_ipc::extension::UnsupportedCapabilityVersion::OutsideSupportedRange {
                    min_inclusive: 1,
                    max_inclusive: 2,
                }
            )
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
