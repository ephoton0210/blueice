// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-owned, deterministic `lib.blueice.d.ts` generation.
//!
//! This module is intentionally adjacent to the core script boundary rather
//! than the BlueTS compiler. A host declares only bindings it actually
//! installs, then derives both the runtime-binding inventory and TypeScript
//! declarations from that one schema. The current core implementation has an
//! explicit empty profile because IPC request handlers are not JavaScript
//! globals by themselves.

use blueice_bluets::ModuleSource;
use blueice_bluets_bluejs::page_host_typings::page_host_document_context_bindings_v1;
use std::collections::BTreeMap;
use std::fmt;

/// Wire identity for generated host typing manifests.
pub const BLUEICE_HOST_TYPINGS_ABI_V1: &str = "blueice-host-typings-v1";

/// The BlueTS language matrix targeted by the initial core host profile.
pub const BLUEICE_HOST_TYPINGS_LANGUAGE_VERSION_V1: &str = "blue-ts-0.1";

/// Core's initial profile has no JavaScript globals until the BlueJS page host
/// installs matching runtime bindings.
pub const CORE_SCRIPT_EMPTY_PROFILE_V1: &str = "core-script-empty-v1";

/// The first truthful core page profile: one read-only document-text global.
pub const CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1: &str = "core-script-document-text-v1";

/// A read-only page-context profile. It exposes only the canonical origin and
/// a copied document-text snapshot; it is not a general `document` object.
/// The profile identity and binding schema are shared with the isolated child
/// so BlueTS cannot be typed against a different callback inventory there.
pub use blueice_bluets_bluejs::page_host_typings::PAGE_HOST_DOCUMENT_CONTEXT_PROFILE_V1 as CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1;

/// Core host API version associated with [`CORE_SCRIPT_EMPTY_PROFILE_V1`].
pub const CORE_SCRIPT_HOST_API_VERSION_V1: &str = "blueice-core-script-v1";

const DECLARATION_HEADER: &str = "// Generated from the BlueIce host type surface. Do not edit.\n";

/// Whether a schema binding occupies a runtime value, type-only, or namespace
/// position. The role is part of the schema fingerprint so a type/value drift
/// cannot reuse an older manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HostBindingRoleV1 {
    Value,
    Type,
    Namespace,
}

impl HostBindingRoleV1 {
    fn as_str(self) -> &'static str {
        match self {
            Self::Value => "value",
            Self::Type => "type",
            Self::Namespace => "namespace",
        }
    }
}

/// One host API definition. Its declaration text must describe exactly the
/// runtime value/type/namespace registered under `runtime_binding_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTypeBindingV1 {
    stable_id: String,
    declaration: String,
    role: HostBindingRoleV1,
    runtime_binding_id: String,
    capability: String,
    feature_flag: String,
    first_host_api_version: String,
}

impl HostTypeBindingV1 {
    /// Creates one schema binding. Validation occurs when its containing
    /// surface is generated, allowing callers to construct a complete schema
    /// before deciding whether to surface an error.
    pub fn new(
        stable_id: impl Into<String>,
        declaration: impl Into<String>,
        role: HostBindingRoleV1,
        runtime_binding_id: impl Into<String>,
        capability: impl Into<String>,
        feature_flag: impl Into<String>,
        first_host_api_version: impl Into<String>,
    ) -> Self {
        Self {
            stable_id: stable_id.into(),
            declaration: declaration.into(),
            role,
            runtime_binding_id: runtime_binding_id.into(),
            capability: capability.into(),
            feature_flag: feature_flag.into(),
            first_host_api_version: first_host_api_version.into(),
        }
    }

    /// Stable documentation and diagnostic identity for this binding.
    pub fn stable_id(&self) -> &str {
        &self.stable_id
    }

    /// Declared TypeScript role.
    pub fn role(&self) -> HostBindingRoleV1 {
        self.role
    }
}

/// A profile's host-owned binding schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTypeSurfaceV1 {
    language_version: String,
    host_api_version: String,
    feature_profile: String,
    bindings: Vec<HostTypeBindingV1>,
}

impl HostTypeSurfaceV1 {
    /// Creates one named host feature profile.
    pub fn new(
        language_version: impl Into<String>,
        host_api_version: impl Into<String>,
        feature_profile: impl Into<String>,
        bindings: Vec<HostTypeBindingV1>,
    ) -> Self {
        Self {
            language_version: language_version.into(),
            host_api_version: host_api_version.into(),
            feature_profile: feature_profile.into(),
            bindings,
        }
    }

    /// Targeted BlueTS language matrix identity.
    pub fn language_version(&self) -> &str {
        &self.language_version
    }

    /// Host API identity for this surface.
    pub fn host_api_version(&self) -> &str {
        &self.host_api_version
    }

    /// Explicit feature profile name.
    pub fn feature_profile(&self) -> &str {
        &self.feature_profile
    }

    /// Generates deterministic TypeScript and runtime-binding artifacts.
    pub fn generate(&self) -> Result<GeneratedHostTypingsV1, HostTypingsError> {
        validate_non_empty("language_version", &self.language_version)?;
        validate_non_empty("host_api_version", &self.host_api_version)?;
        validate_non_empty("feature_profile", &self.feature_profile)?;

        let mut bindings = self.bindings.iter().collect::<Vec<_>>();
        bindings.sort_unstable_by(|left, right| left.stable_id.cmp(&right.stable_id));
        for pair in bindings.windows(2) {
            if pair[0].stable_id == pair[1].stable_id {
                return Err(HostTypingsError::DuplicateBindingId(
                    pair[0].stable_id.clone(),
                ));
            }
        }

        let mut declaration_source = DECLARATION_HEADER.to_string();
        let mut schema_parts = vec![
            BLUEICE_HOST_TYPINGS_ABI_V1.to_string(),
            self.language_version.clone(),
            self.host_api_version.clone(),
            self.feature_profile.clone(),
        ];
        let mut runtime_bindings = Vec::with_capacity(bindings.len());
        let mut binding_ids = Vec::with_capacity(bindings.len());
        for binding in bindings {
            validate_binding(binding)?;
            let declaration = normalize_declaration(&binding.declaration)?;
            if !declaration_source.ends_with('\n') {
                declaration_source.push('\n');
            }
            declaration_source.push('\n');
            declaration_source.push_str(&declaration);
            schema_parts.extend([
                binding.stable_id.clone(),
                binding.role.as_str().to_string(),
                binding.runtime_binding_id.clone(),
                binding.capability.clone(),
                binding.feature_flag.clone(),
                binding.first_host_api_version.clone(),
                declaration,
            ]);
            binding_ids.push(binding.stable_id.clone());
            runtime_bindings.push(HostRuntimeBindingV1 {
                stable_id: binding.stable_id.clone(),
                role: binding.role,
                runtime_binding_id: binding.runtime_binding_id.clone(),
                capability: binding.capability.clone(),
                feature_flag: binding.feature_flag.clone(),
                first_host_api_version: binding.first_host_api_version.clone(),
            });
        }
        let schema_hash = stable_hash("blueice-host-schema", &schema_parts);
        let declaration_hash =
            stable_hash("blueice-host-declaration", &[declaration_source.clone()]);
        let manifest = HostTypingsManifestV1 {
            format: BLUEICE_HOST_TYPINGS_ABI_V1.to_string(),
            language_version: self.language_version.clone(),
            host_api_version: self.host_api_version.clone(),
            schema_hash,
            declaration_hash,
            enabled_feature_profile: self.feature_profile.clone(),
            binding_ids,
        };
        let manifest_json = manifest.to_json();
        Ok(GeneratedHostTypingsV1 {
            declaration_source,
            manifest,
            manifest_json,
            runtime_bindings,
        })
    }
}

/// A deterministic collection of profiles supplied by one host version.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HostTypeSurfaceCatalogV1 {
    surfaces: BTreeMap<String, HostTypeSurfaceV1>,
}

impl HostTypeSurfaceCatalogV1 {
    /// Builds a catalog. Profile names are unique and cannot be selected by an
    /// implicit default, so a direct-page request must name one explicitly.
    pub fn new(
        surfaces: impl IntoIterator<Item = HostTypeSurfaceV1>,
    ) -> Result<Self, HostTypingsError> {
        let mut by_profile = BTreeMap::new();
        for surface in surfaces {
            validate_non_empty("feature_profile", &surface.feature_profile)?;
            let profile = surface.feature_profile.clone();
            if by_profile.contains_key(&profile) {
                return Err(HostTypingsError::DuplicateFeatureProfile(profile));
            }
            by_profile.insert(profile, surface);
        }
        Ok(Self {
            surfaces: by_profile,
        })
    }

    /// Resolves exactly one caller-selected profile.
    pub fn generate(
        &self,
        feature_profile: &str,
    ) -> Result<GeneratedHostTypingsV1, HostTypingsError> {
        self.surfaces
            .get(feature_profile)
            .ok_or_else(|| HostTypingsError::UnknownFeatureProfile(feature_profile.to_string()))?
            .generate()
    }

    /// Returns whether a profile is explicitly supplied by this host version.
    pub fn contains_profile(&self, feature_profile: &str) -> bool {
        self.surfaces.contains_key(feature_profile)
    }
}

/// Core's currently truthful host type catalog. The dispatcher accepts IPC
/// operations, but no BlueJS object exposes them yet, so emitting an empty
/// declaration root prevents TypeScript from claiming an unavailable DOM API.
pub fn core_script_host_type_catalog() -> HostTypeSurfaceCatalogV1 {
    HostTypeSurfaceCatalogV1::new([
        HostTypeSurfaceV1::new(
            BLUEICE_HOST_TYPINGS_LANGUAGE_VERSION_V1,
            CORE_SCRIPT_HOST_API_VERSION_V1,
            CORE_SCRIPT_EMPTY_PROFILE_V1,
            Vec::new(),
        ),
        HostTypeSurfaceV1::new(
            BLUEICE_HOST_TYPINGS_LANGUAGE_VERSION_V1,
            CORE_SCRIPT_HOST_API_VERSION_V1,
            CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1,
            vec![HostTypeBindingV1::new(
                "dom.document-text",
                "declare function blueiceDocumentText(): string;",
                HostBindingRoleV1::Value,
                "global.blueiceDocumentText",
                "dom-read",
                "document-text",
                CORE_SCRIPT_HOST_API_VERSION_V1,
            )],
        ),
        HostTypeSurfaceV1::new(
            BLUEICE_HOST_TYPINGS_LANGUAGE_VERSION_V1,
            CORE_SCRIPT_HOST_API_VERSION_V1,
            CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1,
            page_host_document_context_bindings_v1()
                .iter()
                .map(|binding| {
                    HostTypeBindingV1::new(
                        binding.stable_id,
                        binding.declaration,
                        HostBindingRoleV1::Value,
                        binding.runtime_binding_id,
                        binding.capability,
                        binding.feature_flag,
                        binding.first_host_api_version,
                    )
                })
                .collect(),
        ),
    ])
    .expect("the built-in core script host profile is valid")
}

/// Runtime registration records derived from the same schema as a declaration
/// artifact. A BlueJS host must install every record before it advertises the
/// corresponding profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeBindingV1 {
    pub stable_id: String,
    pub role: HostBindingRoleV1,
    pub runtime_binding_id: String,
    pub capability: String,
    pub feature_flag: String,
    pub first_host_api_version: String,
}

/// The serialized identity distributed beside `lib.blueice.d.ts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTypingsManifestV1 {
    pub format: String,
    pub language_version: String,
    pub host_api_version: String,
    pub schema_hash: String,
    /// Exact normalized declaration-source identity.
    pub declaration_hash: String,
    pub enabled_feature_profile: String,
    pub binding_ids: Vec<String>,
}

impl HostTypingsManifestV1 {
    /// Emits deterministic, LF-normalized JSON with a fixed field order.
    pub fn to_json(&self) -> String {
        let binding_ids = self
            .binding_ids
            .iter()
            .map(|id| json_string(id))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            concat!(
                "{{\n",
                "  \"format\": {},\n",
                "  \"language_version\": {},\n",
                "  \"host_api_version\": {},\n",
                "  \"schema_hash\": {},\n",
                "  \"declaration_hash\": {},\n",
                "  \"enabled_feature_profile\": {},\n",
                "  \"binding_ids\": [{}]\n",
                "}}\n"
            ),
            json_string(&self.format),
            json_string(&self.language_version),
            json_string(&self.host_api_version),
            json_string(&self.schema_hash),
            json_string(&self.declaration_hash),
            json_string(&self.enabled_feature_profile),
            binding_ids,
        )
    }
}

/// The generated `lib.blueice.d.ts`, manifest, and runtime inventory for one
/// selected host profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedHostTypingsV1 {
    pub declaration_source: String,
    pub manifest: HostTypingsManifestV1,
    pub manifest_json: String,
    pub runtime_bindings: Vec<HostRuntimeBindingV1>,
}

impl GeneratedHostTypingsV1 {
    /// Validates a host-supplied manifest and declaration source before a
    /// direct-page compiler or executor accepts the profile. It compares all
    /// identities and exact normalized declaration bytes; there is no profile
    /// or schema fallback.
    pub fn validate_supplied(
        &self,
        supplied_manifest: &HostTypingsManifestV1,
        supplied_declaration_source: &str,
    ) -> Result<(), HostTypingsError> {
        if supplied_manifest.format != BLUEICE_HOST_TYPINGS_ABI_V1 {
            return Err(HostTypingsError::ManifestFormatMismatch);
        }
        if supplied_manifest.language_version != self.manifest.language_version
            || supplied_manifest.host_api_version != self.manifest.host_api_version
            || supplied_manifest.enabled_feature_profile != self.manifest.enabled_feature_profile
        {
            return Err(HostTypingsError::ManifestIdentityMismatch);
        }
        if supplied_manifest.schema_hash != self.manifest.schema_hash {
            return Err(HostTypingsError::SchemaHashMismatch);
        }
        if supplied_manifest.binding_ids != self.manifest.binding_ids {
            return Err(HostTypingsError::BindingInventoryMismatch);
        }
        let supplied_hash = stable_hash(
            "blueice-host-declaration",
            &[supplied_declaration_source.to_string()],
        );
        if supplied_manifest.declaration_hash != supplied_hash
            || supplied_manifest.declaration_hash != self.manifest.declaration_hash
            || supplied_declaration_source != self.declaration_source
        {
            return Err(HostTypingsError::DeclarationBytesMismatch);
        }
        Ok(())
    }

    /// Verifies that a host installed exactly the bindings from this generated
    /// profile. Registration order is irrelevant, but missing, duplicate, or
    /// extra bindings and any identity/capability drift fail closed before the
    /// profile is advertised to a direct-page compiler or executor.
    pub fn validate_runtime_bindings(
        &self,
        supplied_bindings: &[HostRuntimeBindingV1],
    ) -> Result<(), HostTypingsError> {
        let mut supplied = supplied_bindings.to_vec();
        supplied.sort_unstable_by(|left, right| left.stable_id.cmp(&right.stable_id));
        if supplied
            .windows(2)
            .any(|pair| pair[0].stable_id == pair[1].stable_id)
        {
            return Err(HostTypingsError::RuntimeBindingInventoryMismatch);
        }
        let mut expected = self.runtime_bindings.clone();
        expected.sort_unstable_by(|left, right| left.stable_id.cmp(&right.stable_id));
        if supplied != expected {
            return Err(HostTypingsError::RuntimeBindingInventoryMismatch);
        }
        Ok(())
    }

    /// Verifies all host-side typing identities and returns the exact
    /// declaration module a direct-page compiler may expose as an ambient
    /// type surface. The caller must place the returned source in
    /// [`blueice_bluets::CompilerOptions::ambient_declaration_modules`]; this
    /// method never chooses a profile or lets a stale declaration fall back to
    /// another host schema.
    pub fn verify_for_direct_compiler(
        &self,
        canonical_module_id: impl Into<String>,
        supplied_manifest: &HostTypingsManifestV1,
        supplied_declaration_source: &str,
        supplied_runtime_bindings: &[HostRuntimeBindingV1],
    ) -> Result<ModuleSource, HostTypingsError> {
        let canonical_module_id = canonical_module_id.into();
        if canonical_module_id.is_empty()
            || canonical_module_id.contains('\0')
            || !canonical_module_id.ends_with(".d.ts")
        {
            return Err(HostTypingsError::InvalidCompilerModuleId(
                canonical_module_id,
            ));
        }
        self.validate_supplied(supplied_manifest, supplied_declaration_source)?;
        self.validate_runtime_bindings(supplied_runtime_bindings)?;
        Ok(ModuleSource::new(
            canonical_module_id,
            self.declaration_source.clone(),
        ))
    }
}

/// A rejected host typing profile or artifact identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostTypingsError {
    EmptyField(&'static str),
    DuplicateFeatureProfile(String),
    DuplicateBindingId(String),
    InvalidDeclaration(String),
    UnknownFeatureProfile(String),
    ManifestFormatMismatch,
    ManifestIdentityMismatch,
    SchemaHashMismatch,
    BindingInventoryMismatch,
    RuntimeBindingInventoryMismatch,
    DeclarationBytesMismatch,
    InvalidCompilerModuleId(String),
}

impl fmt::Display for HostTypingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField(field) => write!(f, "host typing field `{field}` is empty"),
            Self::DuplicateFeatureProfile(profile) => {
                write!(f, "host typing feature profile `{profile}` is duplicated")
            }
            Self::DuplicateBindingId(id) => {
                write!(f, "host typing binding ID `{id}` is duplicated")
            }
            Self::InvalidDeclaration(message) => write!(f, "invalid host declaration: {message}"),
            Self::UnknownFeatureProfile(profile) => {
                write!(f, "host typing feature profile `{profile}` is unavailable")
            }
            Self::ManifestFormatMismatch => {
                f.write_str("host typing manifest format does not match")
            }
            Self::ManifestIdentityMismatch => {
                f.write_str("host typing manifest language, API, or profile does not match")
            }
            Self::SchemaHashMismatch => {
                f.write_str("host typing manifest schema hash does not match")
            }
            Self::BindingInventoryMismatch => {
                f.write_str("host typing manifest binding inventory does not match")
            }
            Self::RuntimeBindingInventoryMismatch => {
                f.write_str("host runtime binding inventory does not match the typing profile")
            }
            Self::DeclarationBytesMismatch => {
                f.write_str("host typing declaration source bytes do not match")
            }
            Self::InvalidCompilerModuleId(module_id) => write!(
                f,
                "host typing compiler module ID `{module_id}` must be a non-empty `.d.ts` identity"
            ),
        }
    }
}

impl std::error::Error for HostTypingsError {}

fn validate_binding(binding: &HostTypeBindingV1) -> Result<(), HostTypingsError> {
    for (field, value) in [
        ("binding.stable_id", &binding.stable_id),
        ("binding.runtime_binding_id", &binding.runtime_binding_id),
        ("binding.capability", &binding.capability),
        ("binding.feature_flag", &binding.feature_flag),
        (
            "binding.first_host_api_version",
            &binding.first_host_api_version,
        ),
    ] {
        validate_non_empty(field, value)?;
    }
    Ok(())
}

fn validate_non_empty(field: &'static str, value: &str) -> Result<(), HostTypingsError> {
    if value.is_empty() {
        Err(HostTypingsError::EmptyField(field))
    } else {
        Ok(())
    }
}

fn normalize_declaration(source: &str) -> Result<String, HostTypingsError> {
    if source.contains('\0') {
        return Err(HostTypingsError::InvalidDeclaration(
            "NUL bytes are not permitted".to_string(),
        ));
    }
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized
        .split('\n')
        .map(|line| line.trim_end_matches([' ', '\t']))
        .collect::<Vec<_>>();
    while lines.first().is_some_and(|line| line.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return Err(HostTypingsError::InvalidDeclaration(
            "declaration text is empty".to_string(),
        ));
    }
    Ok(format!("{}\n", lines.join("\n")))
}

fn stable_hash(prefix: &str, parts: &[String]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for part in std::iter::once(prefix).chain(parts.iter().map(String::as_str)) {
        let length = u64::try_from(part.len()).expect("string length fits in u64");
        for byte in length.to_le_bytes().into_iter().chain(part.bytes()) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("{prefix}-{hash:016x}")
}

fn json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\u{08}' => escaped.push_str("\\b"),
            '\u{0c}' => escaped.push_str("\\f"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                use std::fmt::Write;
                write!(escaped, "\\u{:04x}", u32::from(character)).expect("writing to String")
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(id: &str, declaration: &str) -> HostTypeBindingV1 {
        HostTypeBindingV1::new(
            id,
            declaration,
            HostBindingRoleV1::Value,
            format!("runtime.{id}"),
            "dom-read",
            "dom",
            "blueice-page-v1",
        )
    }

    #[test]
    fn generation_sorts_normalizes_and_derives_runtime_inventory() {
        let surface = HostTypeSurfaceV1::new(
            "blue-ts-0.1",
            "blueice-page-v1",
            "test-dom",
            vec![
                binding(
                    "dom.document",
                    "\r\n  declare const document: Document;  \r\n",
                ),
                binding("dom.console", "declare const console: Console;\n"),
            ],
        );
        let artifact = surface.generate().unwrap();
        assert_eq!(
            artifact.declaration_source,
            concat!(
                "// Generated from the BlueIce host type surface. Do not edit.\n\n",
                "declare const console: Console;\n\n",
                "  declare const document: Document;\n"
            )
        );
        assert_eq!(
            artifact.manifest.binding_ids,
            ["dom.console", "dom.document"]
        );
        assert_eq!(
            artifact
                .runtime_bindings
                .iter()
                .map(|binding| binding.stable_id.as_str())
                .collect::<Vec<_>>(),
            ["dom.console", "dom.document"]
        );
        assert!(artifact.manifest_json.ends_with("\n"));
        assert!(!artifact.manifest_json.contains('\r'));
        let reverse_order = artifact
            .runtime_bindings
            .iter()
            .cloned()
            .rev()
            .collect::<Vec<_>>();
        assert_eq!(artifact.validate_runtime_bindings(&reverse_order), Ok(()));

        let missing_binding = vec![reverse_order[0].clone()];
        assert_eq!(
            artifact.validate_runtime_bindings(&missing_binding),
            Err(HostTypingsError::RuntimeBindingInventoryMismatch)
        );
        assert_eq!(
            artifact
                .validate_runtime_bindings(&[reverse_order[0].clone(), reverse_order[0].clone()]),
            Err(HostTypingsError::RuntimeBindingInventoryMismatch)
        );
    }

    #[test]
    fn current_empty_profile_matches_the_checked_in_artifacts() {
        let artifact = core_script_host_type_catalog()
            .generate(CORE_SCRIPT_EMPTY_PROFILE_V1)
            .unwrap();
        assert_eq!(
            artifact.declaration_source,
            include_str!("../../tests/fixtures/host_typings/core-script-empty/lib.blueice.d.ts")
        );
        assert_eq!(
            artifact.manifest_json,
            include_str!(
                "../../tests/fixtures/host_typings/core-script-empty/lib.blueice.manifest.json"
            )
        );
        assert!(artifact.runtime_bindings.is_empty());
        assert_eq!(artifact.validate_runtime_bindings(&[]), Ok(()));
        assert!(!artifact.declaration_source.contains("document"));
    }

    #[test]
    fn document_text_profile_declares_exactly_its_installed_global() {
        let artifact = core_script_host_type_catalog()
            .generate(CORE_SCRIPT_DOCUMENT_TEXT_PROFILE_V1)
            .unwrap();
        assert_eq!(
            artifact.declaration_source,
            include_str!(
                "../../tests/fixtures/host_typings/core-script-document-text/lib.blueice.d.ts"
            )
        );
        assert_eq!(
            artifact.manifest_json,
            include_str!(
                "../../tests/fixtures/host_typings/core-script-document-text/lib.blueice.manifest.json"
            )
        );
        assert_eq!(artifact.manifest.binding_ids, ["dom.document-text"]);
        assert_eq!(artifact.runtime_bindings.len(), 1);
        assert_eq!(
            artifact.runtime_bindings[0].runtime_binding_id,
            "global.blueiceDocumentText"
        );
    }

    #[test]
    fn document_context_profile_declares_only_copied_page_context() {
        let artifact = core_script_host_type_catalog()
            .generate(CORE_SCRIPT_DOCUMENT_CONTEXT_PROFILE_V1)
            .unwrap();
        let child_artifact =
            blueice_bluets_bluejs::page_host_typings::PageHostDocumentTypingsV1::generate();
        assert_eq!(
            artifact.declaration_source,
            include_str!(
                "../../tests/fixtures/host_typings/core-script-document-context/lib.blueice.d.ts"
            )
        );
        assert_eq!(
            artifact.manifest_json,
            include_str!(
                "../../tests/fixtures/host_typings/core-script-document-context/lib.blueice.manifest.json"
            )
        );
        assert_eq!(
            artifact.manifest.binding_ids,
            ["dom.document-origin", "dom.document-text"]
        );
        assert_eq!(
            artifact
                .runtime_bindings
                .iter()
                .map(|binding| binding.runtime_binding_id.as_str())
                .collect::<Vec<_>>(),
            ["global.blueiceDocumentOrigin", "global.blueiceDocumentText"]
        );
        assert_eq!(
            artifact.declaration_source,
            child_artifact.declaration_source
        );
        assert_eq!(
            artifact
                .runtime_bindings
                .iter()
                .map(|binding| binding.runtime_binding_id.as_str())
                .collect::<Vec<_>>(),
            child_artifact
                .runtime_bindings
                .iter()
                .map(|binding| binding.runtime_binding_id)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn unavailable_profile_and_mismatched_artifacts_fail_closed() {
        let catalog = core_script_host_type_catalog();
        assert_eq!(
            catalog.generate("imaginary-dom-v1"),
            Err(HostTypingsError::UnknownFeatureProfile(
                "imaginary-dom-v1".to_string()
            ))
        );
        let artifact = catalog.generate(CORE_SCRIPT_EMPTY_PROFILE_V1).unwrap();
        let mut manifest = artifact.manifest.clone();
        manifest.schema_hash.push_str("-wrong");
        assert_eq!(
            artifact.validate_supplied(&manifest, &artifact.declaration_source),
            Err(HostTypingsError::SchemaHashMismatch)
        );
        assert_eq!(
            artifact.validate_supplied(&artifact.manifest, "declare const document: unknown;\n"),
            Err(HostTypingsError::DeclarationBytesMismatch)
        );
    }

    #[test]
    fn verified_artifact_becomes_an_exact_ambient_compiler_module() {
        let artifact = HostTypeSurfaceV1::new(
            "blue-ts-0.1",
            "blueice-page-v1",
            "test-host-v1",
            vec![binding("host.answer", "declare const hostAnswer: number;")],
        )
        .generate()
        .unwrap();
        let declaration = artifact
            .verify_for_direct_compiler(
                "blueice:///profiles/test-host-v1/lib.blueice.d.ts",
                &artifact.manifest,
                &artifact.declaration_source,
                &artifact.runtime_bindings,
            )
            .unwrap();
        let compilation = blueice_bluets::compile(
            "page:///app/main.ts",
            &blueice_bluets::MapLoader::from([blueice_bluets::ModuleSource::new(
                "page:///app/main.ts",
                "const answer: number = hostAnswer;",
            )]),
            blueice_bluets::CompilerOptions {
                ambient_declaration_modules: vec![declaration],
                ..blueice_bluets::CompilerOptions::default()
            },
        );
        assert!(!compilation.has_errors(), "{:?}", compilation.diagnostics);

        assert_eq!(
            artifact.verify_for_direct_compiler(
                "blueice:///profiles/test-host-v1/lib.blueice.ts",
                &artifact.manifest,
                &artifact.declaration_source,
                &artifact.runtime_bindings,
            ),
            Err(HostTypingsError::InvalidCompilerModuleId(
                "blueice:///profiles/test-host-v1/lib.blueice.ts".to_string()
            ))
        );
        assert_eq!(
            artifact.verify_for_direct_compiler(
                "blueice:///profiles/test-host-v1/lib.blueice.d.ts",
                &artifact.manifest,
                &artifact.declaration_source,
                &[],
            ),
            Err(HostTypingsError::RuntimeBindingInventoryMismatch)
        );
    }

    #[test]
    fn duplicate_binding_ids_are_rejected_before_artifacts_are_emitted() {
        let surface = HostTypeSurfaceV1::new(
            "blue-ts-0.1",
            "blueice-page-v1",
            "test-dom",
            vec![
                binding("dom.document", "declare const document: Document;"),
                binding("dom.document", "declare const document: Document;"),
            ],
        );
        assert_eq!(
            surface.generate(),
            Err(HostTypingsError::DuplicateBindingId(
                "dom.document".to_string()
            ))
        );
    }

    #[test]
    fn duplicate_profile_names_are_rejected_before_profile_selection() {
        let first = HostTypeSurfaceV1::new("blue-ts-0.1", "blueice-page-v1", "test-dom", vec![]);
        let second = HostTypeSurfaceV1::new("blue-ts-0.1", "blueice-page-v2", "test-dom", vec![]);
        assert_eq!(
            HostTypeSurfaceCatalogV1::new([first, second]),
            Err(HostTypingsError::DuplicateFeatureProfile(
                "test-dom".to_string()
            ))
        );
    }
}
