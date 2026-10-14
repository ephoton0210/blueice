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

mod abort;
mod animation_frames;
mod custom_events;
mod file_reader;
mod iteration;
mod node_events;
mod object_urls;
mod streams;
mod tee;
mod timers;
mod window_aliases;

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

pub const PAGE_HOST_DOM_FILE_PROFILE_V1: &str = "core-script-dom-file-v1";
pub const PAGE_HOST_DOM_FILE_DECLARATION_MODULE_ID_V1: &str =
    "blueice:///profiles/core-script-dom-file-v1/lib.blueice.d.ts";
pub const PAGE_HOST_DOM_FILE_HOST_API_VERSION_V1: &str = "blueice-core-script-v5";

/// Owner-selected profile for the VM drag/event API installed with live files.
/// The earlier file profile keeps its declaration bytes and runtime inventory.
pub const PAGE_HOST_DOM_DRAG_PROFILE_V1: &str = "core-script-dom-drag-v1";
pub const PAGE_HOST_DOM_DRAG_DECLARATION_MODULE_ID_V1: &str =
    "blueice:///profiles/core-script-dom-drag-v1/lib.blueice.d.ts";
pub const PAGE_HOST_DOM_DRAG_HOST_API_VERSION_V1: &str = "blueice-core-script-v6";
pub const PAGE_HOST_DOM_DRAG_IMAGE_PROFILE_V2: &str = "core-script-dom-drag-image-v2";
pub const PAGE_HOST_DOM_DRAG_IMAGE_DECLARATION_MODULE_ID_V2: &str =
    "blueice:///profiles/core-script-dom-drag-image-v2/lib.blueice.d.ts";
pub const PAGE_HOST_DOM_FILE_READER_PROFILE_V3: &str = "core-script-dom-file-reader-v3";
pub const PAGE_HOST_DOM_FILE_READER_DECLARATION_MODULE_ID_V3: &str =
    "blueice:///profiles/core-script-dom-file-reader-v3/lib.blueice.d.ts";
pub const PAGE_HOST_DOM_STREAM_PROFILE_V4: &str = "core-script-dom-stream-v4";
pub const PAGE_HOST_DOM_STREAM_DECLARATION_MODULE_ID_V4: &str =
    "blueice:///profiles/core-script-dom-stream-v4/lib.blueice.d.ts";

pub const PAGE_HOST_DOM_OBJECT_URL_PROFILE_V5: &str = "core-script-dom-object-url-v5";
pub const PAGE_HOST_DOM_OBJECT_URL_DECLARATION_MODULE_ID_V5: &str =
    "blueice:///profiles/core-script-dom-object-url-v5/lib.blueice.d.ts";

pub const PAGE_HOST_DOM_ABORT_PROFILE_V6: &str = "core-script-dom-abort-v6";
pub const PAGE_HOST_DOM_ABORT_DECLARATION_MODULE_ID_V6: &str =
    "blueice:///profiles/core-script-dom-abort-v6/lib.blueice.d.ts";

pub const PAGE_HOST_DOM_CUSTOM_EVENT_PROFILE_V7: &str = "core-script-dom-custom-event-v7";
pub const PAGE_HOST_DOM_CUSTOM_EVENT_DECLARATION_MODULE_ID_V7: &str =
    "blueice:///profiles/core-script-dom-custom-event-v7/lib.blueice.d.ts";

const DECLARATION_HEADER: &str = "// Generated from the BlueIce host type surface. Do not edit.\n";

pub const PAGE_HOST_DOM_TEE_PROFILE_V9: &str = "core-script-dom-stream-tee-v9";
pub const PAGE_HOST_DOM_ITERATION_PROFILE_V10: &str = "core-script-dom-stream-iteration-v10";
pub const PAGE_HOST_DOM_ITERATION_DECLARATION_MODULE_ID_V10: &str =
    "blueice:///profiles/core-script-dom-stream-iteration-v10/lib.blueice.d.ts";
pub const PAGE_HOST_DOM_TIMERS_PROFILE_V11: &str = "core-script-dom-timers-v11";
pub const PAGE_HOST_DOM_TIMERS_DECLARATION_MODULE_ID_V11: &str =
    "blueice:///profiles/core-script-dom-timers-v11/lib.blueice.d.ts";
pub const PAGE_HOST_DOM_ANIMATION_FRAME_PROFILE_V12: &str = "core-script-dom-animation-frame-v12";
pub const PAGE_HOST_DOM_ANIMATION_FRAME_DECLARATION_MODULE_ID_V12: &str =
    "blueice:///profiles/core-script-dom-animation-frame-v12/lib.blueice.d.ts";
pub const PAGE_HOST_DOM_WINDOW_PROFILE_V13: &str = "core-script-dom-window-v13";
pub const PAGE_HOST_DOM_WINDOW_DECLARATION_MODULE_ID_V13: &str =
    "blueice:///profiles/core-script-dom-window-v13/lib.blueice.d.ts";
pub const PAGE_HOST_DOM_TEE_DECLARATION_MODULE_ID_V9: &str =
    "blueice:///profiles/core-script-dom-stream-tee-v9/lib.blueice.d.ts";

pub const PAGE_HOST_DOM_NODE_EVENT_PROFILE_V8: &str = "core-script-dom-node-event-v8";
pub const PAGE_HOST_DOM_NODE_EVENT_DECLARATION_MODULE_ID_V8: &str =
    "blueice:///profiles/core-script-dom-node-event-v8/lib.blueice.d.ts";

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

// File declarations use complete records because BlueTS rejects ambient
// interface merging. The older proof profiles keep their exact inventories.
const PAGE_HOST_DOM_FILE_BINDINGS_V1: [PageHostDocumentBindingV1; 16] = [
    PAGE_HOST_DOM_EVENT_BINDINGS_V1[0],
    PAGE_HOST_DOM_EVENT_BINDINGS_V1[1],
    PageHostDocumentBindingV1 {
        declaration: "interface BlueIceNode { textContent: string; appendChild(child: BlueIceNode): BlueIceNode; readonly files: BlueIceFileList | null; value: string | undefined; reset(): void; addEventListener(eventType: 'click', listener: (event: BlueIceClickEvent) => void): void; addEventListener(eventType: 'input' | 'change' | 'cancel', listener: (event: BlueIceSelectionEvent) => void): void; removeEventListener(eventType: 'click', listener: (event: BlueIceClickEvent) => void): void; removeEventListener(eventType: 'input' | 'change' | 'cancel', listener: (event: BlueIceSelectionEvent) => void): void; }\ninterface BlueIceDocument { getElementById(id: string): BlueIceNode | null; createElement(tagName: string): BlueIceNode; createTextNode(data: string): BlueIceNode; textContent: string; appendChild(child: BlueIceNode): BlueIceNode; readonly files: BlueIceFileList | null; value: string | undefined; reset(): void; addEventListener(eventType: 'click', listener: (event: BlueIceClickEvent) => void): void; addEventListener(eventType: 'input' | 'change' | 'cancel', listener: (event: BlueIceSelectionEvent) => void): void; removeEventListener(eventType: 'click', listener: (event: BlueIceClickEvent) => void): void; removeEventListener(eventType: 'input' | 'change' | 'cancel', listener: (event: BlueIceSelectionEvent) => void): void; }\ndeclare const document: BlueIceDocument;",
        feature_flag: "live-dom-file",
        first_host_api_version: PAGE_HOST_DOM_FILE_HOST_API_VERSION_V1,
        ..PAGE_HOST_DOM_EVENT_BINDINGS_V1[2]
    },
    PAGE_HOST_DOM_EVENT_BINDINGS_V1[3],
    PAGE_HOST_DOM_EVENT_BINDINGS_V1[4],
    PAGE_HOST_DOM_EVENT_BINDINGS_V1[5],
    PageHostDocumentBindingV1 {
        declaration: "interface BlueIceClickEvent { readonly type: 'click'; readonly target: BlueIceNode; readonly currentTarget: BlueIceNode; preventDefault(): void; }",
        feature_flag: "live-dom-file",
        first_host_api_version: PAGE_HOST_DOM_FILE_HOST_API_VERSION_V1,
        ..PAGE_HOST_DOM_EVENT_BINDINGS_V1[6]
    },
    PAGE_HOST_DOM_EVENT_BINDINGS_V1[7],
    PageHostDocumentBindingV1 {
        declaration: "interface BlueIceSelectionEvent { readonly type: 'input' | 'change' | 'cancel'; readonly target: BlueIceNode; readonly currentTarget: BlueIceNode | null; readonly bubbles: boolean; readonly cancelable: boolean; readonly composed: boolean; readonly isTrusted: boolean; readonly defaultPrevented: boolean; preventDefault(): void; stopPropagation(): void; stopImmediatePropagation(): void; }",
        feature_flag: "live-dom-file",
        first_host_api_version: PAGE_HOST_DOM_FILE_HOST_API_VERSION_V1,
        ..PAGE_HOST_DOM_EVENT_BINDINGS_V1[8]
    },
    PAGE_HOST_DOM_EVENT_BINDINGS_V1[9],
    PageHostDocumentBindingV1 {
        stable_id: "file.blob",
        declaration: "interface BlueIceFilePromise<T> { then(onfulfilled: (value: T) => unknown): BlueIceFilePromise<unknown>; catch(onrejected: (reason: unknown) => unknown): BlueIceFilePromise<unknown>; finally(onfinally: () => void): BlueIceFilePromise<T>; }\ninterface BlueIceBlob { readonly size: number; readonly type: string; slice(start?: number, end?: number, contentType?: string): BlueIceBlob; text(): BlueIceFilePromise<string>; arrayBuffer(): BlueIceFilePromise<unknown>; bytes(): BlueIceFilePromise<unknown>; }\ndeclare const Blob: { new(parts?: unknown[], options?: { type?: string; endings?: 'transparent' | 'native' }): BlueIceBlob; readonly prototype: BlueIceBlob; };",
        role: PageHostBindingRoleV1::Value,
        runtime_binding_id: "global.Blob",
        capability: "selected-file-read",
        feature_flag: "live-dom-file",
        first_host_api_version: PAGE_HOST_DOM_FILE_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "file.file",
        declaration: "interface BlueIceFile extends BlueIceBlob { readonly name: string; readonly lastModified: number; }\ndeclare const File: { new(parts: unknown[], name: string, options?: { type?: string; endings?: 'transparent' | 'native'; lastModified?: number }): BlueIceFile; readonly prototype: BlueIceFile; };",
        role: PageHostBindingRoleV1::Value,
        runtime_binding_id: "global.File",
        capability: "selected-file-read",
        feature_flag: "live-dom-file",
        first_host_api_version: PAGE_HOST_DOM_FILE_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "file.file-list",
        declaration: "interface BlueIceFileList { readonly length: number; item(index: number): BlueIceFile | null; }\ndeclare const FileList: { readonly prototype: BlueIceFileList; };",
        role: PageHostBindingRoleV1::Value,
        runtime_binding_id: "global.FileList",
        capability: "selected-file-read",
        feature_flag: "live-dom-file",
        first_host_api_version: PAGE_HOST_DOM_FILE_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "dom.live-node-files",
        declaration: "interface BlueIceNodeFiles { readonly files: BlueIceFileList | null; }",
        role: PageHostBindingRoleV1::Type,
        runtime_binding_id: "node.files",
        capability: "selected-file-read",
        feature_flag: "live-dom-file",
        first_host_api_version: PAGE_HOST_DOM_FILE_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "dom.live-node-file-value",
        declaration: "interface BlueIceNodeFileValue { value: string | undefined; }",
        role: PageHostBindingRoleV1::Type,
        runtime_binding_id: "node.value",
        capability: "selected-file-write",
        feature_flag: "live-dom-file",
        first_host_api_version: PAGE_HOST_DOM_FILE_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "dom.live-node-form-reset",
        declaration: "interface BlueIceNodeFormReset { reset(): void; }",
        role: PageHostBindingRoleV1::Type,
        runtime_binding_id: "node.reset",
        capability: "dom-write",
        feature_flag: "live-dom-file",
        first_host_api_version: PAGE_HOST_DOM_FILE_HOST_API_VERSION_V1,
    },
];

pub fn page_host_dom_file_runtime_bindings_v1() -> [PageHostRuntimeBindingV1; 16] {
    PAGE_HOST_DOM_FILE_BINDINGS_V1.map(|binding| PageHostRuntimeBindingV1 {
        stable_id: binding.stable_id,
        runtime_binding_id: binding.runtime_binding_id,
    })
}

// BlueTS has no numeric interface index signatures. The bounded types use
// actual includes/join and item-list methods; JS keeps native indexed access.
const PAGE_HOST_DOM_DRAG_BINDINGS_V1: [PageHostDocumentBindingV1; 24] = [
    PAGE_HOST_DOM_FILE_BINDINGS_V1[0],
    PAGE_HOST_DOM_FILE_BINDINGS_V1[1],
    PageHostDocumentBindingV1 {
        declaration: "type BlueIceDragCallback = (event: BlueIceDragEvent) => void;\ninterface BlueIceDragListenerOptions { capture?: boolean; once?: boolean; passive?: boolean; }\ninterface BlueIceDragEventListener { handleEvent(event: BlueIceDragEvent): void; }\ninterface BlueIceDragRemovalOptions { capture?: boolean; }\ninterface BlueIceNode { textContent: string; appendChild(child: BlueIceNode): BlueIceNode; readonly files: BlueIceFileList | null; value: string | undefined; reset(): void; addEventListener(eventType: 'click', listener: (event: BlueIceClickEvent) => void): void; addEventListener(eventType: 'input' | 'change' | 'cancel', listener: (event: BlueIceSelectionEvent) => void): void; addEventListener(eventType: 'dragstart' | 'drag' | 'dragenter' | 'dragleave' | 'dragover' | 'drop' | 'dragend', listener: BlueIceDragCallback | BlueIceDragEventListener | null, options?: boolean | BlueIceDragListenerOptions): void; removeEventListener(eventType: 'click', listener: (event: BlueIceClickEvent) => void): void; removeEventListener(eventType: 'input' | 'change' | 'cancel', listener: (event: BlueIceSelectionEvent) => void): void; removeEventListener(eventType: 'dragstart' | 'drag' | 'dragenter' | 'dragleave' | 'dragover' | 'drop' | 'dragend', listener: BlueIceDragCallback | BlueIceDragEventListener | null, options?: boolean | BlueIceDragRemovalOptions): void; }\ninterface BlueIceDocument { textContent: string; appendChild(child: BlueIceNode): BlueIceNode; readonly files: BlueIceFileList | null; value: string | undefined; reset(): void; addEventListener(eventType: 'click', listener: (event: BlueIceClickEvent) => void): void; addEventListener(eventType: 'input' | 'change' | 'cancel', listener: (event: BlueIceSelectionEvent) => void): void; addEventListener(eventType: 'dragstart' | 'drag' | 'dragenter' | 'dragleave' | 'dragover' | 'drop' | 'dragend', listener: BlueIceDragCallback | BlueIceDragEventListener | null, options?: boolean | BlueIceDragListenerOptions): void; removeEventListener(eventType: 'click', listener: (event: BlueIceClickEvent) => void): void; removeEventListener(eventType: 'input' | 'change' | 'cancel', listener: (event: BlueIceSelectionEvent) => void): void; removeEventListener(eventType: 'dragstart' | 'drag' | 'dragenter' | 'dragleave' | 'dragover' | 'drop' | 'dragend', listener: BlueIceDragCallback | BlueIceDragEventListener | null, options?: boolean | BlueIceDragRemovalOptions): void;  getElementById(id: string): BlueIceNode | null; createElement(tagName: string): BlueIceNode; createTextNode(data: string): BlueIceNode; }\ndeclare const document: BlueIceDocument;",
        feature_flag: "live-dom-drag",
        first_host_api_version: PAGE_HOST_DOM_DRAG_HOST_API_VERSION_V1,
        ..PAGE_HOST_DOM_FILE_BINDINGS_V1[2]
    },
    PAGE_HOST_DOM_FILE_BINDINGS_V1[3],
    PAGE_HOST_DOM_FILE_BINDINGS_V1[4],
    PAGE_HOST_DOM_FILE_BINDINGS_V1[5],
    PAGE_HOST_DOM_FILE_BINDINGS_V1[6],
    PAGE_HOST_DOM_FILE_BINDINGS_V1[7],
    PAGE_HOST_DOM_FILE_BINDINGS_V1[8],
    PAGE_HOST_DOM_FILE_BINDINGS_V1[9],
    PAGE_HOST_DOM_FILE_BINDINGS_V1[10],
    PAGE_HOST_DOM_FILE_BINDINGS_V1[11],
    PAGE_HOST_DOM_FILE_BINDINGS_V1[12],
    PAGE_HOST_DOM_FILE_BINDINGS_V1[13],
    PAGE_HOST_DOM_FILE_BINDINGS_V1[14],
    PAGE_HOST_DOM_FILE_BINDINGS_V1[15],
    PageHostDocumentBindingV1 {
        stable_id: "drag.global-Event",
        declaration: "interface BlueIceEventInit { bubbles?: boolean; cancelable?: boolean; composed?: boolean; }\ninterface BlueIceUIEventInit extends BlueIceEventInit { view?: null; detail?: number; }\ninterface BlueIceMouseEventInit extends BlueIceUIEventInit { ctrlKey?: boolean; shiftKey?: boolean; altKey?: boolean; metaKey?: boolean; modifierAltGraph?: boolean; modifierCapsLock?: boolean; modifierFn?: boolean; modifierFnLock?: boolean; modifierHyper?: boolean; modifierNumLock?: boolean; modifierScrollLock?: boolean; modifierSuper?: boolean; modifierSymbol?: boolean; modifierSymbolLock?: boolean; screenX?: number; screenY?: number; clientX?: number; clientY?: number; button?: number; buttons?: number; relatedTarget?: BlueIceNode | null; }\ninterface BlueIceDragEventInit extends BlueIceMouseEventInit { dataTransfer?: BlueIceDataTransfer | null; }\ninterface BlueIceEvent { readonly type: string; readonly target: BlueIceNode | null; readonly srcElement: BlueIceNode | null; readonly currentTarget: BlueIceNode | null; readonly eventPhase: number; readonly bubbles: boolean; readonly cancelable: boolean; readonly composed: boolean; readonly defaultPrevented: boolean; readonly isTrusted: boolean; readonly timeStamp: number; cancelBubble: boolean; returnValue: boolean; preventDefault(): void; stopPropagation(): void; stopImmediatePropagation(): void; composedPath(): BlueIceNode[]; initEvent(eventType: string, bubbles?: boolean, cancelable?: boolean): void; }\ndeclare const Event: { new(eventType: string, init?: BlueIceEventInit): BlueIceEvent; readonly prototype: BlueIceEvent; readonly NONE: number; readonly CAPTURING_PHASE: number; readonly AT_TARGET: number; readonly BUBBLING_PHASE: number; };",
        role: PageHostBindingRoleV1::Value,
        runtime_binding_id: "global.Event",
        capability: "dom-read",
        feature_flag: "live-dom-drag",
        first_host_api_version: PAGE_HOST_DOM_DRAG_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "drag.global-UIEvent",
        declaration: "interface BlueIceUIEvent extends BlueIceEvent { readonly view: null; readonly detail: number; readonly which: number; initUIEvent(eventType: string, bubbles?: boolean, cancelable?: boolean, view?: null, detail?: number): void; }\ndeclare const UIEvent: { new(eventType: string, init?: BlueIceUIEventInit): BlueIceUIEvent; readonly prototype: BlueIceUIEvent; };",
        role: PageHostBindingRoleV1::Value,
        runtime_binding_id: "global.UIEvent",
        capability: "dom-read",
        feature_flag: "live-dom-drag",
        first_host_api_version: PAGE_HOST_DOM_DRAG_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "drag.global-MouseEvent",
        declaration: "interface BlueIceMouseEvent extends BlueIceUIEvent { readonly screenX: number; readonly screenY: number; readonly clientX: number; readonly clientY: number; readonly x: number; readonly y: number; readonly pageX: number; readonly pageY: number; readonly offsetX: number; readonly offsetY: number; readonly ctrlKey: boolean; readonly shiftKey: boolean; readonly altKey: boolean; readonly metaKey: boolean; readonly button: number; readonly buttons: number; readonly relatedTarget: BlueIceNode | null; getModifierState(key: string): boolean; initMouseEvent(eventType: string, bubbles?: boolean, cancelable?: boolean, view?: null, detail?: number, screenX?: number, screenY?: number, clientX?: number, clientY?: number, ctrl?: boolean, alt?: boolean, shift?: boolean, meta?: boolean, button?: number, related?: BlueIceNode | null): void; }\ndeclare const MouseEvent: { new(eventType: string, init?: BlueIceMouseEventInit): BlueIceMouseEvent; readonly prototype: BlueIceMouseEvent; };",
        role: PageHostBindingRoleV1::Value,
        runtime_binding_id: "global.MouseEvent",
        capability: "dom-read",
        feature_flag: "live-dom-drag",
        first_host_api_version: PAGE_HOST_DOM_DRAG_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "drag.global-DragEvent",
        declaration: "interface BlueIceDragEvent extends BlueIceMouseEvent { readonly dataTransfer: BlueIceDataTransfer | null; }\ndeclare const DragEvent: { new(eventType: string, init?: BlueIceDragEventInit): BlueIceDragEvent; readonly prototype: BlueIceDragEvent; };",
        role: PageHostBindingRoleV1::Value,
        runtime_binding_id: "global.DragEvent",
        capability: "dom-read",
        feature_flag: "live-dom-drag",
        first_host_api_version: PAGE_HOST_DOM_DRAG_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "drag.global-DataTransfer",
        declaration: "interface BlueIceDataTransferTypes { readonly length: number; includes(format: string, fromIndex?: number): boolean; join(separator?: string): string; }\ninterface BlueIceDataTransfer { dropEffect: string; effectAllowed: string; readonly items: BlueIceDataTransferItemList; readonly files: BlueIceFileList; readonly types: BlueIceDataTransferTypes; getData(format: string): string; setData(format: string, data: string): void; clearData(format?: string): void; }\ndeclare const DataTransfer: { new(): BlueIceDataTransfer; readonly prototype: BlueIceDataTransfer; };",
        role: PageHostBindingRoleV1::Value,
        runtime_binding_id: "global.DataTransfer",
        capability: "dom-read",
        feature_flag: "live-dom-drag",
        first_host_api_version: PAGE_HOST_DOM_DRAG_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "drag.global-DataTransferItemList",
        declaration: "interface BlueIceDataTransferItemList { readonly length: number; add(data: string, format: string): BlueIceDataTransferItem | null; add(file: BlueIceFile): BlueIceDataTransferItem | null; remove(index: number): void; clear(): void; }\ndeclare const DataTransferItemList: { readonly prototype: BlueIceDataTransferItemList; };",
        role: PageHostBindingRoleV1::Value,
        runtime_binding_id: "global.DataTransferItemList",
        capability: "dom-read",
        feature_flag: "live-dom-drag",
        first_host_api_version: PAGE_HOST_DOM_DRAG_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "drag.global-DataTransferItem",
        declaration: "type BlueIceDragStringCallback = (data: string) => void;\ninterface BlueIceDataTransferItem { readonly kind: string; readonly type: string; getAsFile(): BlueIceFile | null; getAsString(callback: BlueIceDragStringCallback | null): void; }\ndeclare const DataTransferItem: { readonly prototype: BlueIceDataTransferItem; };",
        role: PageHostBindingRoleV1::Value,
        runtime_binding_id: "global.DataTransferItem",
        capability: "dom-read",
        feature_flag: "live-dom-drag",
        first_host_api_version: PAGE_HOST_DOM_DRAG_HOST_API_VERSION_V1,
    },
    PageHostDocumentBindingV1 {
        stable_id: "drag.global-DOMException",
        declaration: "interface BlueIceDOMException { readonly name: string; readonly message: string; readonly code: number; }\ndeclare const DOMException: { new(message?: string, name?: string): BlueIceDOMException; readonly prototype: BlueIceDOMException; };",
        role: PageHostBindingRoleV1::Value,
        runtime_binding_id: "global.DOMException",
        capability: "dom-read",
        feature_flag: "live-dom-drag",
        first_host_api_version: PAGE_HOST_DOM_DRAG_HOST_API_VERSION_V1,
    },
];

pub fn page_host_dom_drag_runtime_bindings_v1() -> [PageHostRuntimeBindingV1; 24] {
    PAGE_HOST_DOM_DRAG_BINDINGS_V1.map(|binding| PageHostRuntimeBindingV1 {
        stable_id: binding.stable_id,
        runtime_binding_id: binding.runtime_binding_id,
    })
}

const PAGE_HOST_DOM_DRAG_IMAGE_BINDINGS_V2: [PageHostDocumentBindingV1; 24] = {
    let mut bindings = PAGE_HOST_DOM_DRAG_BINDINGS_V1;
    bindings[20].declaration = "interface BlueIceDataTransferTypes { readonly length: number; includes(format: string, fromIndex?: number): boolean; join(separator?: string): string; }\ninterface BlueIceDataTransfer { dropEffect: string; effectAllowed: string; readonly items: BlueIceDataTransferItemList; readonly files: BlueIceFileList; readonly types: BlueIceDataTransferTypes; getData(format: string): string; setData(format: string, data: string): void; clearData(format?: string): void; setDragImage(image: BlueIceNode, x: number, y: number): void; }\ndeclare const DataTransfer: { new(): BlueIceDataTransfer; readonly prototype: BlueIceDataTransfer; };";
    bindings[20].first_host_api_version = "blueice-core-script-v7";
    bindings
};

pub fn page_host_dom_drag_image_runtime_bindings_v2() -> [PageHostRuntimeBindingV1; 24] {
    PAGE_HOST_DOM_DRAG_IMAGE_BINDINGS_V2.map(|binding| PageHostRuntimeBindingV1 {
        stable_id: binding.stable_id,
        runtime_binding_id: binding.runtime_binding_id,
    })
}

pub fn page_host_dom_file_reader_runtime_bindings_v3() -> [PageHostRuntimeBindingV1; 27] {
    file_reader::bindings().map(|binding| PageHostRuntimeBindingV1 {
        stable_id: binding.stable_id,
        runtime_binding_id: binding.runtime_binding_id,
    })
}

pub fn page_host_dom_stream_runtime_bindings_v4() -> [PageHostRuntimeBindingV1; 33] {
    streams::bindings().map(|binding| PageHostRuntimeBindingV1 {
        stable_id: binding.stable_id,
        runtime_binding_id: binding.runtime_binding_id,
    })
}

pub fn page_host_dom_object_url_runtime_bindings_v5() -> [PageHostRuntimeBindingV1; 34] {
    object_urls::bindings().map(|binding| PageHostRuntimeBindingV1 {
        stable_id: binding.stable_id,
        runtime_binding_id: binding.runtime_binding_id,
    })
}

pub fn page_host_dom_abort_runtime_bindings_v6() -> [PageHostRuntimeBindingV1; 36] {
    abort::bindings().map(|binding| PageHostRuntimeBindingV1 {
        stable_id: binding.stable_id,
        runtime_binding_id: binding.runtime_binding_id,
    })
}

pub fn page_host_dom_custom_event_runtime_bindings_v7() -> [PageHostRuntimeBindingV1; 37] {
    custom_events::bindings().map(|binding| PageHostRuntimeBindingV1 {
        stable_id: binding.stable_id,
        runtime_binding_id: binding.runtime_binding_id,
    })
}

pub fn page_host_dom_node_event_runtime_bindings_v8() -> [PageHostRuntimeBindingV1; 38] {
    node_events::bindings().map(|binding| PageHostRuntimeBindingV1 {
        stable_id: binding.stable_id,
        runtime_binding_id: binding.runtime_binding_id,
    })
}

pub fn page_host_dom_tee_runtime_bindings_v9() -> [PageHostRuntimeBindingV1; 39] {
    tee::bindings().map(|binding| PageHostRuntimeBindingV1 {
        stable_id: binding.stable_id,
        runtime_binding_id: binding.runtime_binding_id,
    })
}

pub fn page_host_dom_iteration_runtime_bindings_v10() -> [PageHostRuntimeBindingV1; 40] {
    iteration::bindings().map(|binding| PageHostRuntimeBindingV1 {
        stable_id: binding.stable_id,
        runtime_binding_id: binding.runtime_binding_id,
    })
}

pub fn page_host_dom_timers_runtime_bindings_v11() -> [PageHostRuntimeBindingV1; 45] {
    timers::bindings().map(|binding| PageHostRuntimeBindingV1 {
        stable_id: binding.stable_id,
        runtime_binding_id: binding.runtime_binding_id,
    })
}

pub fn page_host_dom_animation_frame_runtime_bindings_v12() -> [PageHostRuntimeBindingV1; 47] {
    animation_frames::bindings().map(|binding| PageHostRuntimeBindingV1 {
        stable_id: binding.stable_id,
        runtime_binding_id: binding.runtime_binding_id,
    })
}

pub fn page_host_dom_window_runtime_bindings_v13() -> [PageHostRuntimeBindingV1; 51] {
    window_aliases::bindings().map(|binding| PageHostRuntimeBindingV1 {
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

    pub fn generate_dom_file() -> Self {
        Self::generate_from(
            PAGE_HOST_DOM_FILE_PROFILE_V1,
            &PAGE_HOST_DOM_FILE_BINDINGS_V1,
        )
    }

    pub fn generate_dom_drag() -> Self {
        Self::generate_from(
            PAGE_HOST_DOM_DRAG_PROFILE_V1,
            &PAGE_HOST_DOM_DRAG_BINDINGS_V1,
        )
    }
    pub fn generate_dom_drag_image() -> Self {
        Self::generate_from(
            PAGE_HOST_DOM_DRAG_IMAGE_PROFILE_V2,
            &PAGE_HOST_DOM_DRAG_IMAGE_BINDINGS_V2,
        )
    }

    pub fn generate_dom_file_reader() -> Self {
        Self::generate_from(
            PAGE_HOST_DOM_FILE_READER_PROFILE_V3,
            &file_reader::bindings(),
        )
    }

    pub fn generate_dom_stream() -> Self {
        Self::generate_from(PAGE_HOST_DOM_STREAM_PROFILE_V4, &streams::bindings())
    }

    pub fn generate_dom_object_url() -> Self {
        Self::generate_from(
            PAGE_HOST_DOM_OBJECT_URL_PROFILE_V5,
            &object_urls::bindings(),
        )
    }

    pub fn generate_dom_abort() -> Self {
        Self::generate_from(PAGE_HOST_DOM_ABORT_PROFILE_V6, &abort::bindings())
    }

    pub fn generate_dom_custom_event() -> Self {
        Self::generate_from(
            PAGE_HOST_DOM_CUSTOM_EVENT_PROFILE_V7,
            &custom_events::bindings(),
        )
    }

    pub fn generate_dom_node_event() -> Self {
        Self::generate_from(
            PAGE_HOST_DOM_NODE_EVENT_PROFILE_V8,
            &node_events::bindings(),
        )
    }

    pub fn generate_dom_tee() -> Self {
        Self::generate_from(PAGE_HOST_DOM_TEE_PROFILE_V9, &tee::bindings())
    }

    pub fn generate_dom_iteration() -> Self {
        Self::generate_from(PAGE_HOST_DOM_ITERATION_PROFILE_V10, &iteration::bindings())
    }

    pub fn generate_dom_timers() -> Self {
        Self::generate_from(PAGE_HOST_DOM_TIMERS_PROFILE_V11, &timers::bindings())
    }

    pub fn generate_dom_animation_frame() -> Self {
        Self::generate_from(
            PAGE_HOST_DOM_ANIMATION_FRAME_PROFILE_V12,
            &animation_frames::bindings(),
        )
    }

    pub fn generate_dom_window() -> Self {
        let bindings = window_aliases::bindings();
        let members = window_aliases::member_declaration(&bindings[..47]);
        Self::generate_from_with_tail(PAGE_HOST_DOM_WINDOW_PROFILE_V13, &bindings, &members)
    }

    fn generate_from(profile: &'static str, bindings: &[PageHostDocumentBindingV1]) -> Self {
        Self::generate_from_with_tail(profile, bindings, "")
    }

    fn generate_from_with_tail(
        profile: &'static str,
        bindings: &[PageHostDocumentBindingV1],
        tail: &str,
    ) -> Self {
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
        declaration_source.push_str(tail);
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

    pub fn verified_dom_file_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate_dom_file(),
            PAGE_HOST_DOM_FILE_DECLARATION_MODULE_ID_V1,
            installed_bindings,
        )
    }

    pub fn verified_dom_drag_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate_dom_drag(),
            PAGE_HOST_DOM_DRAG_DECLARATION_MODULE_ID_V1,
            installed_bindings,
        )
    }
    pub fn verified_dom_drag_image_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate_dom_drag_image(),
            PAGE_HOST_DOM_DRAG_IMAGE_DECLARATION_MODULE_ID_V2,
            installed_bindings,
        )
    }

    pub fn verified_dom_file_reader_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate_dom_file_reader(),
            PAGE_HOST_DOM_FILE_READER_DECLARATION_MODULE_ID_V3,
            installed_bindings,
        )
    }

    pub fn verified_dom_stream_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate_dom_stream(),
            PAGE_HOST_DOM_STREAM_DECLARATION_MODULE_ID_V4,
            installed_bindings,
        )
    }

    pub fn verified_dom_object_url_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate_dom_object_url(),
            PAGE_HOST_DOM_OBJECT_URL_DECLARATION_MODULE_ID_V5,
            installed_bindings,
        )
    }

    pub fn verified_dom_abort_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate_dom_abort(),
            PAGE_HOST_DOM_ABORT_DECLARATION_MODULE_ID_V6,
            installed_bindings,
        )
    }

    pub fn verified_dom_custom_event_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate_dom_custom_event(),
            PAGE_HOST_DOM_CUSTOM_EVENT_DECLARATION_MODULE_ID_V7,
            installed_bindings,
        )
    }

    pub fn verified_dom_node_event_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate_dom_node_event(),
            PAGE_HOST_DOM_NODE_EVENT_DECLARATION_MODULE_ID_V8,
            installed_bindings,
        )
    }

    pub fn verified_dom_tee_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate_dom_tee(),
            PAGE_HOST_DOM_TEE_DECLARATION_MODULE_ID_V9,
            installed_bindings,
        )
    }

    pub fn verified_dom_iteration_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate_dom_iteration(),
            PAGE_HOST_DOM_ITERATION_DECLARATION_MODULE_ID_V10,
            installed_bindings,
        )
    }

    pub fn verified_dom_timers_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate_dom_timers(),
            PAGE_HOST_DOM_TIMERS_DECLARATION_MODULE_ID_V11,
            installed_bindings,
        )
    }

    pub fn verified_dom_animation_frame_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate_dom_animation_frame(),
            PAGE_HOST_DOM_ANIMATION_FRAME_DECLARATION_MODULE_ID_V12,
            installed_bindings,
        )
    }

    pub fn verified_dom_window_ambient_module(
        &self,
        installed_bindings: &[PageHostRuntimeBindingV1],
    ) -> Result<ModuleSource, PageHostTypingsError> {
        self.verified_module_for(
            Self::generate_dom_window(),
            PAGE_HOST_DOM_WINDOW_DECLARATION_MODULE_ID_V13,
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
    fn file_profile_preserves_click_types_and_checks_selection_metadata() {
        use blueice_bluets::{CompilerOptions, MapLoader, RuntimePolicy};

        let artifact = PageHostDocumentTypingsV1::generate_dom_file();
        let bindings = page_host_dom_file_runtime_bindings_v1();
        assert_eq!(artifact.profile, PAGE_HOST_DOM_FILE_PROFILE_V1);
        assert_eq!(bindings.len(), 16);
        assert!(artifact.verify_runtime_bindings(&bindings[..15]).is_err());
        assert!(artifact
            .verify_runtime_bindings(&page_host_dom_event_runtime_bindings_v1())
            .is_err());
        let ambient = artifact
            .verified_dom_file_ambient_module(&bindings)
            .unwrap();
        assert_eq!(ambient.id, PAGE_HOST_DOM_FILE_DECLARATION_MODULE_ID_V1);
        let compile = |source| {
            crate::compile_direct_script(
                "memory:///file.ts",
                &MapLoader::from([ModuleSource::new("memory:///file.ts", source)]),
                CompilerOptions {
                    runtime_policy: RuntimePolicy::Checked,
                    require_declared_global_calls: true,
                    ambient_declaration_modules: vec![ambient.clone()],
                    ..CompilerOptions::default()
                },
            )
        };
        compile("function onClick(event: BlueIceClickEvent): void { event.preventDefault(); } document.getElementById('link')!.addEventListener('click', onClick);").unwrap();
        compile("function onChange(event: BlueIceSelectionEvent): void { const file = event.target.files!.item(0)!; const size: number = file.size; const name: string = file.name; const changed: number = file.lastModified; event.stopPropagation(); } document.addEventListener('change', onChange);").unwrap();
        compile("const file = new File(['abc'], 'chosen.bin', { lastModified: 123 }); const blob: BlueIceBlob = file.slice(0, 2); document.getElementById('upload')!.value = ''; document.getElementById('form')!.reset();").unwrap();
        for source in [
            "document.getElementById('upload')!.files = null;",
            "document.getElementById('upload')!.files!.item(0)!.name = 'changed';",
            "document.getElementById('upload')!.dispatchEvent('change');",
            "document.getElementById('upload')!.addEventListener('unknown', () => {});",
            "function check(blob: BlueIceBlob): void { blob.stream(); }",
        ] {
            assert!(compile(source).is_err(), "{source}");
        }
    }

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
        compile("function tryQuery(value: any): void { value.querySelector('#link'); } function tryStop(value: any): void { value.stopPropagation(); }").unwrap();
        assert!(
            compile("document.getElementById('link')!.addEventListener('change', () => {});")
                .is_err()
        );
        assert!(
            compile("document.getElementById('link')!.addEventListener('click', 'wrong');")
                .is_err()
        );
        assert!(compile("document.getElementById('link')!.dispatchEvent('click');").is_err());
        for (source, member) in [
            (
                "function onClick(event: BlueIceClickEvent): void { event.stopPropagation(); } document.getElementById('link')!.addEventListener('click', onClick);",
                "stopPropagation",
            ),
            ("document.querySelector('#link');", "querySelector"),
        ] {
            let error = compile(source)
                .err()
                .expect("an uninstalled event-v1 member must fail checking");
            let crate::BridgeError::BlueTs(diagnostics) = error else {
                panic!("{source}: expected BlueTS diagnostics, got {error:?}");
            };
            assert!(
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains(member)),
                "{source}: {diagnostics:#?}"
            );
        }
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
            "interface Slots { event: BlueIceClickEvent; other: number; } function write(slots: Slots, key: string): void { slots[key].type = 'click'; }",
            "interface Other { type: 'click'; } interface Slots { event: BlueIceClickEvent; other: Other; } function write(slots: Slots, key: string): void { const selected = slots[key]; selected.type = 'click'; }",
            "interface Holder { event: BlueIceClickEvent; type: string; } function write(holder: Holder): void { const selected = [holder.event][0]; selected.type = 'click'; }",
            "interface Holder { event: BlueIceClickEvent; type: string; } function write(holder: Holder): void { const selected = { picked: holder.event }; selected.picked.type = 'click'; }",
            "interface Holder { event: BlueIceClickEvent; type: string; } function write(holder: Holder): void { const selected = holder.event as BlueIceClickEvent; selected.type = 'click'; }",
            "interface MutableEvent { type: string; } interface Holder { event: MutableEvent; } interface Source { event: BlueIceClickEvent; } function write(holder: Holder, source: Source): void { const selected = (holder, source.event); selected.type = 'click'; }",
            "interface MutableEvent { type: string; } interface Holder { slot: BlueIceClickEvent; event: MutableEvent; } interface Source { event: BlueIceClickEvent; } function write(holder: Holder, source: Source): void { const selected = (holder.slot = source.event); selected.type = 'click'; }",
            "interface MutableEvent { type: string; } interface Holder { slot: MutableEvent; } interface Source { event: BlueIceClickEvent; } function write(holder: Holder, source: Source): void { const selected = (holder.slot ||= source.event); selected.type = 'click'; }",
            "interface MutableEvent { type: string; } interface Holder { event: BlueIceClickEvent; mutable: MutableEvent; } function write(holder: Holder): void { const selected = holder.mutable || holder.event; selected.type = 'click'; }",
            "interface MutableEvent { type: string; } type EventPair = [BlueIceClickEvent, MutableEvent]; function write(pair: EventPair): void { const selected = [...pair][0]; selected.type = 'click'; }",
            "interface Holder { event: BlueIceClickEvent; } function write(holder: Holder): void { const copied = { ...holder }; copied.event.type = 'click'; }",
            "interface MutableEvent { type: string; } type Holder = { event: BlueIceClickEvent } | { event: MutableEvent }; function write(holder: Holder): void { const copied = { ...holder }; copied.event.type = 'click'; }",
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
        let opaque = "interface Holder { event: BlueIceClickEvent; } function erase(value: Holder): any { return value; } function write(holder: Holder): void { const copied = { ...erase(holder) }; copied.event.type = 'click'; }";
        let error = compile(opaque)
            .err()
            .expect("opaque record spread must fail before runtime");
        let crate::BridgeError::BlueTs(diagnostics) = error else {
            panic!("expected BlueTS diagnostics, got {error:?}");
        };
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("record spread source")),
            "{diagnostics:#?}"
        );

        let opaque_receiver = "declare function pick(value: unknown): unknown; function onClick(event: BlueIceClickEvent): void { pick(event).type = 'click'; } document.getElementById('link')!.addEventListener('click', onClick);";
        let error = compile(opaque_receiver)
            .err()
            .expect("opaque click-event receiver must fail before runtime");
        let crate::BridgeError::BlueTs(diagnostics) = error else {
            panic!("expected BlueTS diagnostics, got {error:?}");
        };
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic
                .message
                .contains("cannot prove a write through an unmodeled receiver")),
            "{diagnostics:#?}"
        );
    }
}
