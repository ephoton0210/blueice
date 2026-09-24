// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed, host-owned BlueTS declarations for the isolated page host.
//!
//! This belongs at the BlueTS-to-BlueJS boundary rather than in either the
//! core or launcher crate: both need the same generated declaration bytes and
//! runtime inventory, while neither may depend on the other. The default
//! profile contains only copied snapshots; a distinct owner-selected profile
//! can describe the bounded live DOM text route. Neither is `lib.dom.d.ts`.

use blueice_bluets::ModuleSource;
use std::fmt;

/// The profile identity shared by the core binding catalog and the isolated
/// page-host child.  A page request contains no profile selector.
pub const PAGE_HOST_DOCUMENT_CONTEXT_PROFILE_V1: &str = "core-script-document-context-v1";

/// Canonical ambient module identity for the fixed page-host declaration.
/// It is a compiler identity only; the child never loads this through a URL,
/// filesystem, package, import map, or resolver.
pub const PAGE_HOST_DOCUMENT_CONTEXT_DECLARATION_MODULE_ID_V1: &str =
    "blueice:///profiles/core-script-document-context-v1/lib.blueice.d.ts";

/// The host API revision in which the two copied snapshot callbacks appeared.
pub const PAGE_HOST_DOCUMENT_CONTEXT_HOST_API_VERSION_V1: &str = "blueice-core-script-v1";

/// The separate owner-selected profile for a live DOM text route.
pub const PAGE_HOST_DOM_TEXT_PROFILE_V1: &str = "core-script-dom-text-v1";
pub const PAGE_HOST_DOM_TEXT_DECLARATION_MODULE_ID_V1: &str =
    "blueice:///profiles/core-script-dom-text-v1/lib.blueice.d.ts";
pub const PAGE_HOST_DOM_TEXT_HOST_API_VERSION_V1: &str = "blueice-core-script-v2";

const DECLARATION_HEADER: &str = "// Generated from the BlueIce host type surface. Do not edit.\n";

/// One runtime global represented by the fixed page-host profile.
///
/// The stable identity and capability fields let the core's broader host
/// catalog derive its manifest from precisely the schema that the child uses
/// to install callbacks.  The fields are all fixed constants, never request
/// data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PageHostBindingRoleV1 {
    Value,
    Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PageHostDocumentBindingV1 {
    pub stable_id: &'static str,
    pub declaration: &'static str,
    pub role: PageHostBindingRoleV1,
    pub runtime_binding_id: &'static str,
    pub capability: &'static str,
    pub feature_flag: &'static str,
    pub first_host_api_version: &'static str,
}

const PAGE_HOST_DOCUMENT_CONTEXT_BINDINGS_V1: [PageHostDocumentBindingV1; 2] = [
    PageHostDocumentBindingV1 {
        stable_id: "dom.document-origin",
        declaration: "declare function blueiceDocumentOrigin(): string;",
        role: PageHostBindingRoleV1::Value,
        runtime_binding_id: "global.blueiceDocumentOrigin",
        capability: "dom-read",
        feature_flag: "document-origin",
        first_host_api_version: PAGE_HOST_DOCUMENT_CONTEXT_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "dom.document-text",
        declaration: "declare function blueiceDocumentText(): string;",
        role: PageHostBindingRoleV1::Value,
        runtime_binding_id: "global.blueiceDocumentText",
        capability: "dom-read",
        feature_flag: "document-text",
        first_host_api_version: PAGE_HOST_DOCUMENT_CONTEXT_HOST_API_VERSION_V1,
    },
];

const PAGE_HOST_DOM_TEXT_BINDINGS_V1: [PageHostDocumentBindingV1; 5] = [
    PAGE_HOST_DOCUMENT_CONTEXT_BINDINGS_V1[0],
    PAGE_HOST_DOCUMENT_CONTEXT_BINDINGS_V1[1],
    PageHostDocumentBindingV1 {
        stable_id: "dom.live-document",
        declaration: "declare const document: BlueIceDocument;",
        role: PageHostBindingRoleV1::Value,
        runtime_binding_id: "global.document",
        capability: "dom-read",
        feature_flag: "live-dom-text",
        first_host_api_version: PAGE_HOST_DOM_TEXT_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "dom.live-document-method",
        declaration:
            "interface BlueIceDocument { getElementById(id: string): BlueIceNode | null; }",
        role: PageHostBindingRoleV1::Type,
        runtime_binding_id: "document.getElementById",
        capability: "dom-read",
        feature_flag: "live-dom-text",
        first_host_api_version: PAGE_HOST_DOM_TEXT_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "dom.live-node-text",
        declaration: "interface BlueIceNode { textContent: string; }",
        role: PageHostBindingRoleV1::Type,
        runtime_binding_id: "node.textContent",
        capability: "dom-write",
        feature_flag: "live-dom-text",
        first_host_api_version: PAGE_HOST_DOM_TEXT_HOST_API_VERSION_V1,
    },
];

/// Returns the complete fixed binding inventory in deterministic stable-ID
/// order.  There is no API to add, remove, or select records for one page.
pub fn page_host_document_context_bindings_v1() -> &'static [PageHostDocumentBindingV1] {
    &PAGE_HOST_DOCUMENT_CONTEXT_BINDINGS_V1
}

pub fn page_host_dom_text_bindings_v1() -> &'static [PageHostDocumentBindingV1] {
    &PAGE_HOST_DOM_TEXT_BINDINGS_V1
}

pub fn page_host_dom_text_runtime_bindings_v1() -> [PageHostRuntimeBindingV1; 5] {
    PAGE_HOST_DOM_TEXT_BINDINGS_V1.map(|binding| PageHostRuntimeBindingV1 {
        stable_id: binding.stable_id,
        runtime_binding_id: binding.runtime_binding_id,
    })
}

/// A runtime callback identity supplied by a host that wants to compile
/// BlueTS against the fixed profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PageHostRuntimeBindingV1 {
    pub stable_id: &'static str,
    pub runtime_binding_id: &'static str,
}

/// Returns exactly the callback inventory that the launcher child installs.
/// Both installation and compiler admission consume this function so a
/// missing, extra, or renamed callback cannot be typed by accident.
pub fn page_host_document_runtime_bindings_v1() -> [PageHostRuntimeBindingV1; 2] {
    PAGE_HOST_DOCUMENT_CONTEXT_BINDINGS_V1.map(|binding| PageHostRuntimeBindingV1 {
        stable_id: binding.stable_id,
        runtime_binding_id: binding.runtime_binding_id,
    })
}

/// Deterministic generated declaration artifact for the fixed page-host
/// profile.  `declaration_hash` is an identity check, not a cryptographic
/// integrity mechanism; source-selection/integrity policy remains core work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageHostDocumentTypingsV1 {
    pub profile: &'static str,
    pub declaration_source: String,
    pub declaration_hash: String,
    pub runtime_bindings: Vec<PageHostRuntimeBindingV1>,
}

impl PageHostDocumentTypingsV1 {
    /// Generates the exact ambient source from the fixed schema.  Its ordering
    /// matches the core host-typing catalog so the two consumers cannot drift
    /// in declaration bytes or binding order.
    pub fn generate() -> Self {
        Self::generate_from(
            PAGE_HOST_DOCUMENT_CONTEXT_PROFILE_V1,
            page_host_document_context_bindings_v1(),
        )
    }

    pub fn generate_dom_text() -> Self {
        Self::generate_from(
            PAGE_HOST_DOM_TEXT_PROFILE_V1,
            page_host_dom_text_bindings_v1(),
        )
    }

    fn generate_from(profile: &'static str, bindings: &[PageHostDocumentBindingV1]) -> Self {
        let mut declaration_source = DECLARATION_HEADER.to_string();
        let mut runtime_bindings = Vec::with_capacity(bindings.len());
        for binding in bindings {
            declaration_source.push('\n');
            declaration_source.push_str(binding.declaration);
            declaration_source.push('\n');
            runtime_bindings.push(PageHostRuntimeBindingV1 {
                stable_id: binding.stable_id,
                runtime_binding_id: binding.runtime_binding_id,
            });
        }
        let declaration_hash = stable_hash("blueice-page-host-declaration", &declaration_source);
        Self {
            profile,
            declaration_source,
            declaration_hash,
            runtime_bindings,
        }
    }

    /// Checks that a host installs the exact fixed runtime inventory.  The
    /// comparison is set-like only after duplicate rejection, so installation
    /// order cannot change typing.  It fails closed on a missing, extra, or
    /// renamed binding.
    pub fn verify_runtime_bindings(
        &self,
        supplied: &[PageHostRuntimeBindingV1],
    ) -> Result<(), PageHostTypingsError> {
        let mut supplied = supplied.to_vec();
        supplied.sort_unstable();
        if supplied.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(PageHostTypingsError::RuntimeBindingInventoryMismatch);
        }
        let mut expected = self.runtime_bindings.clone();
        expected.sort_unstable();
        if supplied != expected {
            return Err(PageHostTypingsError::RuntimeBindingInventoryMismatch);
        }
        Ok(())
    }

    /// Revalidates generated bytes and the caller's fixed runtime inventory,
    /// then returns the only ambient declaration module the child may pass to
    /// BlueTS.  The page cannot provide a declaration, module ID, profile, or
    /// compiler option at this boundary.
    pub fn verified_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate(),
            PAGE_HOST_DOCUMENT_CONTEXT_DECLARATION_MODULE_ID_V1,
            installed_bindings,
        )
    }

    pub fn verified_dom_text_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate_dom_text(),
            PAGE_HOST_DOM_TEXT_DECLARATION_MODULE_ID_V1,
            installed_bindings,
        )
    }

    fn verified_module_for(
        &self,
        generated: Self,
        module_id: &'static str,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        if self.profile != generated.profile
            || self.declaration_hash != generated.declaration_hash
            || self.declaration_source != generated.declaration_source
            || self.runtime_bindings != generated.runtime_bindings
        {
            return Err(PageHostTypingsError::ArtifactMismatch);
        }
        self.verify_runtime_bindings(installed_bindings)?;
        Ok(ModuleSource::new(
            module_id,
            self.declaration_source.clone(),
        ))
    }
}

/// A failure while deriving the child-fixed ambient declaration surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageHostTypingsError {
    ArtifactMismatch,
    RuntimeBindingInventoryMismatch,
}

impl fmt::Display for PageHostTypingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ArtifactMismatch => f.write_str("page-host typing artifact does not match"),
            Self::RuntimeBindingInventoryMismatch => {
                f.write_str("page-host runtime bindings do not match the typing artifact")
            }
        }
    }
}

impl std::error::Error for PageHostTypingsError {}

fn stable_hash(prefix: &str, source: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    let length = u64::try_from(source.len()).expect("string length fits in u64");
    for byte in prefix
        .bytes()
        .chain(length.to_le_bytes())
        .chain(source.bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{prefix}-{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_document_profile_has_only_the_two_installed_callbacks() {
        let artifact = PageHostDocumentTypingsV1::generate();
        assert_eq!(artifact.profile, PAGE_HOST_DOCUMENT_CONTEXT_PROFILE_V1);
        assert_eq!(
            artifact.declaration_source,
            concat!(
                "// Generated from the BlueIce host type surface. Do not edit.\n\n",
                "declare function blueiceDocumentOrigin(): string;\n\n",
                "declare function blueiceDocumentText(): string;\n"
            )
        );
        assert!(artifact.declaration_source.contains("blueiceDocumentText"));
        assert!(!artifact.declaration_source.contains("document:"));
        assert!(!artifact.declaration_source.contains("fetch"));
        artifact
            .verify_runtime_bindings(&page_host_document_runtime_bindings_v1())
            .unwrap();
    }

    #[test]
    fn missing_extra_or_modified_runtime_bindings_fail_closed() {
        let artifact = PageHostDocumentTypingsV1::generate();
        let bindings = page_host_document_runtime_bindings_v1();
        assert_eq!(
            artifact.verify_runtime_bindings(&bindings[..1]),
            Err(PageHostTypingsError::RuntimeBindingInventoryMismatch)
        );
        let extra = [
            bindings[0],
            bindings[1],
            PageHostRuntimeBindingV1 {
                stable_id: "net.fetch",
                runtime_binding_id: "global.fetch",
            },
        ];
        assert_eq!(
            artifact.verify_runtime_bindings(&extra),
            Err(PageHostTypingsError::RuntimeBindingInventoryMismatch)
        );
        let mut modified = bindings;
        modified[0].runtime_binding_id = "global.documentOrigin";
        assert_eq!(
            artifact.verify_runtime_bindings(&modified),
            Err(PageHostTypingsError::RuntimeBindingInventoryMismatch)
        );
    }

    #[test]
    fn verified_module_is_exact_and_cannot_advertise_uninstalled_bindings() {
        let artifact = PageHostDocumentTypingsV1::generate();
        let declaration = artifact
            .verified_ambient_module(&page_host_document_runtime_bindings_v1())
            .unwrap();
        assert_eq!(
            declaration.id,
            PAGE_HOST_DOCUMENT_CONTEXT_DECLARATION_MODULE_ID_V1
        );
        assert_eq!(declaration.text, artifact.declaration_source);
        let mut modified = artifact.clone();
        modified
            .declaration_source
            .push_str("declare const fetch: unknown;\n");
        assert_eq!(
            modified.verified_ambient_module(&page_host_document_runtime_bindings_v1()),
            Err(PageHostTypingsError::ArtifactMismatch)
        );
    }

    #[test]
    fn live_dom_text_profile_has_an_exact_separate_runtime_inventory() {
        let artifact = PageHostDocumentTypingsV1::generate_dom_text();
        assert_eq!(artifact.profile, PAGE_HOST_DOM_TEXT_PROFILE_V1);
        assert!(artifact
            .declaration_source
            .contains("getElementById(id: string): BlueIceNode | null"));
        assert!(artifact.declaration_source.contains("textContent: string"));
        assert!(!artifact.declaration_source.contains("fetch"));
        let bindings = page_host_dom_text_runtime_bindings_v1();
        let declaration = artifact
            .verified_dom_text_ambient_module(&bindings)
            .unwrap();
        assert_eq!(declaration.id, PAGE_HOST_DOM_TEXT_DECLARATION_MODULE_ID_V1);
        assert_eq!(declaration.text, artifact.declaration_source);
        assert_eq!(
            artifact.verify_runtime_bindings(&bindings[..4]),
            Err(PageHostTypingsError::RuntimeBindingInventoryMismatch)
        );
        assert_eq!(
            artifact.verified_ambient_module(&bindings),
            Err(PageHostTypingsError::ArtifactMismatch)
        );
    }
}
