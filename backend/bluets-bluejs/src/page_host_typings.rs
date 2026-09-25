// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed, host-owned BlueTS declarations for the isolated page host.
//!
//! This belongs at the BlueTS-to-BlueJS boundary rather than in either the
//! core or launcher crate: both need the same generated declaration bytes and
//! runtime inventory, while neither may depend on the other. The default
//! profile contains only copied snapshots; a distinct owner-selected profile
//! can describe bounded live DOM text or creation/append routes. None is
//! `lib.dom.d.ts`.

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

/// A separate owner-selected profile for bounded live DOM creation/append.
/// The text-v1 artifact remains immutable for existing embedders.
pub const PAGE_HOST_DOM_MUTATION_PROFILE_V1: &str = "core-script-dom-mutation-v1";
pub const PAGE_HOST_DOM_MUTATION_DECLARATION_MODULE_ID_V1: &str =
    "blueice:///profiles/core-script-dom-mutation-v1/lib.blueice.d.ts";
pub const PAGE_HOST_DOM_MUTATION_HOST_API_VERSION_V1: &str = "blueice-core-script-v3";

/// An owner-selected superset with VM-rooted, exact-node click listeners.
pub const PAGE_HOST_DOM_EVENT_PROFILE_V1: &str = "core-script-dom-event-v1";
pub const PAGE_HOST_DOM_EVENT_DECLARATION_MODULE_ID_V1: &str =
    "blueice:///profiles/core-script-dom-event-v1/lib.blueice.d.ts";
pub const PAGE_HOST_DOM_EVENT_HOST_API_VERSION_V1: &str = "blueice-core-script-v4";

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

const PAGE_HOST_DOM_MUTATION_BINDINGS_V1: [PageHostDocumentBindingV1; 8] = [
    PAGE_HOST_DOCUMENT_CONTEXT_BINDINGS_V1[0],
    PAGE_HOST_DOCUMENT_CONTEXT_BINDINGS_V1[1],
    PageHostDocumentBindingV1 {
        stable_id: "dom.live-document",
        declaration: "interface BlueIceNode extends BlueIceNodeAppend, BlueIceNodeText {}\ninterface BlueIceDocument extends BlueIceDocumentCreateElement, BlueIceDocumentCreateTextNode, BlueIceDocumentLookup {}\ndeclare const document: BlueIceDocument;",
        role: PageHostBindingRoleV1::Value,
        runtime_binding_id: "global.document",
        capability: "dom-read",
        feature_flag: "live-dom-mutation",
        first_host_api_version: PAGE_HOST_DOM_MUTATION_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "dom.live-document-create-element",
        declaration: "interface BlueIceDocumentCreateElement { createElement(tagName: string): BlueIceNode; }",
        role: PageHostBindingRoleV1::Type,
        runtime_binding_id: "document.createElement",
        capability: "dom-write",
        feature_flag: "live-dom-mutation",
        first_host_api_version: PAGE_HOST_DOM_MUTATION_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "dom.live-document-create-text-node",
        declaration: "interface BlueIceDocumentCreateTextNode { createTextNode(data: string): BlueIceNode; }",
        role: PageHostBindingRoleV1::Type,
        runtime_binding_id: "document.createTextNode",
        capability: "dom-write",
        feature_flag: "live-dom-mutation",
        first_host_api_version: PAGE_HOST_DOM_MUTATION_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "dom.live-document-method",
        declaration: "interface BlueIceDocumentLookup { getElementById(id: string): BlueIceNode | null; }",
        role: PageHostBindingRoleV1::Type,
        runtime_binding_id: "document.getElementById",
        capability: "dom-read",
        feature_flag: "live-dom-mutation",
        first_host_api_version: PAGE_HOST_DOM_MUTATION_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "dom.live-node-append",
        declaration: "interface BlueIceNodeAppend { appendChild(child: BlueIceNode): BlueIceNode; }",
        role: PageHostBindingRoleV1::Type,
        runtime_binding_id: "node.appendChild",
        capability: "dom-write",
        feature_flag: "live-dom-mutation",
        first_host_api_version: PAGE_HOST_DOM_MUTATION_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "dom.live-node-text",
        declaration: "interface BlueIceNodeText { textContent: string; }",
        role: PageHostBindingRoleV1::Type,
        runtime_binding_id: "node.textContent",
        capability: "dom-write",
        feature_flag: "live-dom-mutation",
        first_host_api_version: PAGE_HOST_DOM_MUTATION_HOST_API_VERSION_V1,
    },
];

const PAGE_HOST_DOM_EVENT_BINDINGS_V1: [PageHostDocumentBindingV1; 10] = [
    PAGE_HOST_DOM_MUTATION_BINDINGS_V1[0],
    PAGE_HOST_DOM_MUTATION_BINDINGS_V1[1],
    PageHostDocumentBindingV1 {
        stable_id: "dom.live-document",
        declaration: "interface BlueIceNode extends BlueIceNodeAppend, BlueIceNodeText, BlueIceNodeClickAdd, BlueIceNodeClickRemove {}\ninterface BlueIceDocument extends BlueIceDocumentCreateElement, BlueIceDocumentCreateTextNode, BlueIceDocumentLookup {}\ndeclare const document: BlueIceDocument;",
        role: PageHostBindingRoleV1::Value,
        runtime_binding_id: "global.document",
        capability: "dom-read",
        feature_flag: "live-dom-click",
        first_host_api_version: PAGE_HOST_DOM_EVENT_HOST_API_VERSION_V1,
    },
    PAGE_HOST_DOM_MUTATION_BINDINGS_V1[3],
    PAGE_HOST_DOM_MUTATION_BINDINGS_V1[4],
    PAGE_HOST_DOM_MUTATION_BINDINGS_V1[5],
    PageHostDocumentBindingV1 {
        stable_id: "dom.live-node-add-click-listener",
        declaration: "interface BlueIceNodeClickAdd { addEventListener(eventType: 'click', listener: (event: BlueIceClickEvent) => void): void; }\ninterface BlueIceClickEvent { readonly type: 'click'; readonly target: BlueIceNode; readonly currentTarget: BlueIceNode; preventDefault(): void; }",
        role: PageHostBindingRoleV1::Type,
        runtime_binding_id: "node.addEventListener",
        capability: "dom-event",
        feature_flag: "live-dom-click",
        first_host_api_version: PAGE_HOST_DOM_EVENT_HOST_API_VERSION_V1,
    },
    PAGE_HOST_DOM_MUTATION_BINDINGS_V1[6],
    PageHostDocumentBindingV1 {
        stable_id: "dom.live-node-remove-click-listener",
        declaration: "interface BlueIceNodeClickRemove { removeEventListener(eventType: 'click', listener: (event: BlueIceClickEvent) => void): void; }",
        role: PageHostBindingRoleV1::Type,
        runtime_binding_id: "node.removeEventListener",
        capability: "dom-event",
        feature_flag: "live-dom-click",
        first_host_api_version: PAGE_HOST_DOM_EVENT_HOST_API_VERSION_V1,
    },
    PAGE_HOST_DOM_MUTATION_BINDINGS_V1[7],
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

pub fn page_host_dom_mutation_bindings_v1() -> &'static [PageHostDocumentBindingV1] {
    &PAGE_HOST_DOM_MUTATION_BINDINGS_V1
}

pub fn page_host_dom_mutation_runtime_bindings_v1() -> [PageHostRuntimeBindingV1; 8] {
    PAGE_HOST_DOM_MUTATION_BINDINGS_V1.map(|binding| PageHostRuntimeBindingV1 {
        stable_id: binding.stable_id,
        runtime_binding_id: binding.runtime_binding_id,
    })
}

pub fn page_host_dom_event_bindings_v1() -> &'static [PageHostDocumentBindingV1] {
    &PAGE_HOST_DOM_EVENT_BINDINGS_V1
}

pub fn page_host_dom_event_runtime_bindings_v1() -> [PageHostRuntimeBindingV1; 10] {
    PAGE_HOST_DOM_EVENT_BINDINGS_V1.map(|binding| PageHostRuntimeBindingV1 {
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

    pub fn generate_dom_mutation() -> Self {
        Self::generate_from(
            PAGE_HOST_DOM_MUTATION_PROFILE_V1,
            page_host_dom_mutation_bindings_v1(),
        )
    }

    pub fn generate_dom_event() -> Self {
        Self::generate_from(
            PAGE_HOST_DOM_EVENT_PROFILE_V1,
            page_host_dom_event_bindings_v1(),
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

    pub fn verified_dom_mutation_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate_dom_mutation(),
            PAGE_HOST_DOM_MUTATION_DECLARATION_MODULE_ID_V1,
            installed_bindings,
        )
    }

    pub fn verified_dom_event_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate_dom_event(),
            PAGE_HOST_DOM_EVENT_DECLARATION_MODULE_ID_V1,
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

    #[test]
    fn live_dom_mutation_profile_types_only_installed_creation_and_append() {
        use blueice_bluets::{CompilerOptions, MapLoader, RuntimePolicy};

        let artifact = PageHostDocumentTypingsV1::generate_dom_mutation();
        assert_eq!(artifact.profile, PAGE_HOST_DOM_MUTATION_PROFILE_V1);
        let bindings = page_host_dom_mutation_runtime_bindings_v1();
        let ambient = artifact
            .verified_dom_mutation_ambient_module(&bindings)
            .unwrap();
        assert_eq!(ambient.id, PAGE_HOST_DOM_MUTATION_DECLARATION_MODULE_ID_V1);
        assert_eq!(bindings.len(), 8);
        assert_eq!(
            artifact.verify_runtime_bindings(&bindings[..7]),
            Err(PageHostTypingsError::RuntimeBindingInventoryMismatch)
        );
        let compile = |source| {
            crate::compile_direct_script(
                "memory:///main.ts",
                &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
                CompilerOptions {
                    runtime_policy: RuntimePolicy::Checked,
                    require_declared_global_calls: true,
                    ambient_declaration_modules: vec![ambient.clone()],
                    ..CompilerOptions::default()
                },
            )
        };
        compile(
            "const parent = document.getElementById('target')!;\n\
             const child = document.createElement('span');\n\
             const text = document.createTextNode('rendered');\n\
             child.appendChild(text); parent.appendChild(child);",
        )
        .unwrap();
        for source in [
            "document.createElement(42);",
            "document.createTextNode();",
            "document.createTextNode(42);",
            "const parent = document.getElementById('target')!; parent.appendChild('wrong');",
            "document.getElementById('target')!.appendChild('wrong');",
            "document.fetch('https://example.test/');",
        ] {
            assert!(compile(source).is_err(), "{source}");
        }
    }

    #[test]
    fn click_event_profile_types_only_the_installed_listener_pair() {
        use blueice_bluets::{CompilerOptions, MapLoader, RuntimePolicy};

        let artifact = PageHostDocumentTypingsV1::generate_dom_event();
        let bindings = page_host_dom_event_runtime_bindings_v1();
        assert_eq!(artifact.profile, PAGE_HOST_DOM_EVENT_PROFILE_V1);
        assert_eq!(bindings.len(), 10);
        assert!(artifact.verify_runtime_bindings(&bindings[..9]).is_err());
        let ambient = artifact
            .verified_dom_event_ambient_module(&bindings)
            .unwrap();
        assert_eq!(ambient.id, PAGE_HOST_DOM_EVENT_DECLARATION_MODULE_ID_V1);
        let compile = |source| {
            crate::compile_direct_script(
                "memory:///click.ts",
                &MapLoader::from([ModuleSource::new("memory:///click.ts", source)]),
                CompilerOptions {
                    runtime_policy: RuntimePolicy::Checked,
                    require_declared_global_calls: true,
                    ambient_declaration_modules: vec![ambient.clone()],
                    ..CompilerOptions::default()
                },
            )
        };
        compile("function onClick(event: BlueIceClickEvent): void { event.preventDefault(); } document.getElementById('link')!.addEventListener('click', onClick);").unwrap();
        assert!(
            compile("document.getElementById('link')!.addEventListener('change', () => {});")
                .is_err()
        );
        assert!(
            compile("document.getElementById('link')!.addEventListener('click', 'wrong');")
                .is_err()
        );
        assert!(compile("document.getElementById('link')!.dispatchEvent('click');").is_err());
        for source in [
            "function onClick(event: BlueIceClickEvent): void { event.type = 'click'; }",
            "function onClick(event: BlueIceClickEvent): void { event.target = event.target; }",
            "function onClick(event: BlueIceClickEvent): void { event.currentTarget = event.target; }",
            "function onClick(event: BlueIceClickEvent): void { event['type'] = 'click'; }",
            "function onClick(event: BlueIceClickEvent, key: string): void { event[key] = 'click'; }",
            "function onClick(event: BlueIceClickEvent): void { (event).type = 'click'; }",
            "function getClick(event: BlueIceClickEvent): BlueIceClickEvent { return event; } function onClick(event: BlueIceClickEvent): void { getClick(event).target = event.target; }",
            "function onClick(event: BlueIceClickEvent): void { let other = ''; other = event.type = 'click'; }",
            "function onClick(event: BlueIceClickEvent): void { let changed = event.type++ + 1; }",
            "function write(events: BlueIceClickEvent[], index: number): void { events[index].type = 'click'; }",
            "function write(events: BlueIceClickEvent[]): void { events['0'].type = 'click'; }",
            "function write(events: BlueIceClickEvent[], index: number): void { events[index].currentTarget = events[index].target; }",
            "function consume(value: string): void {} function write(events: BlueIceClickEvent[], index: number): void { consume(events[index].type = 'click'); }",
        ] {
            let error = compile(source).err().expect("readonly write must fail");
            let crate::BridgeError::BlueTs(diagnostics) = error else {
                panic!("{source}: expected BlueTS diagnostics, got {error:?}");
            };
            assert!(
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains("readonly")),
                "{source}: {diagnostics:#?}"
            );
        }
    }
}
