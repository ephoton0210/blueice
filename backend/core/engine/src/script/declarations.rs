// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Discovery of explicit BlueTS page-script declarations in a parsed document.
//!
//! This module only reports script declarations. It never opens an external
//! `src`, chooses an origin, constructs a module graph, or evaluates source;
//! those remain host-policy decisions at the direct-page admission boundary.

use super::direct_page::DirectPageScriptKind;
use blueice_dom::{Document, NodeData, NodeId};

/// The standard JavaScript script forms supported by the first BlueJS page
/// host. This is intentionally separate from the non-standard BlueTS kinds:
/// ordinary `<script>` tags belong to the JavaScript pipeline and must never
/// be reinterpreted as TypeScript declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlueJsPageScriptKind {
    Classic,
    Module,
}

/// The explicit, non-portable HTML type for an opt-in classic BlueTS script.
pub const BLUE_TS_CLASSIC_SCRIPT_TYPE: &str = "application/x-blueice-typescript";

/// The explicit, non-portable HTML type for an opt-in BlueTS module script.
pub const BLUE_TS_MODULE_SCRIPT_TYPE: &str = "application/x-blueice-typescript-module";

/// One direct BlueTS script declaration in document order.
///
/// `ordinal` is stable for the exact parsed document and provides a future
/// page loader with a deterministic input when it mints a policy-approved
/// canonical source identity. It is not an origin, URL, capability, or runtime
/// program handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlueTsPageScriptDeclaration {
    Inline {
        ordinal: u32,
        kind: DirectPageScriptKind,
        source: String,
    },
    External {
        ordinal: u32,
        kind: DirectPageScriptKind,
        src: String,
    },
}

/// One standard JavaScript page-script declaration in document order.
///
/// The declaration is a parsed-document record only. It supplies no source
/// loading, URL resolution, origin, DOM, or execution authority. In
/// particular, an external `src` is not fetched unless a core-owned JavaScript
/// source authorizer later returns a closed graph for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlueJsPageScriptDeclaration {
    Inline {
        ordinal: u32,
        kind: BlueJsPageScriptKind,
        source: String,
    },
    External {
        ordinal: u32,
        kind: BlueJsPageScriptKind,
        src: String,
    },
}

/// One supported page-script declaration in document order across the two
/// explicitly separate language lanes. This is used only by a host that owns
/// one realm for both lanes: it does not make JavaScript a BlueTS input or
/// grant source-loading authority to either declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CombinedPageScriptDeclaration {
    Inline {
        ordinal: u32,
        language: CombinedPageScriptLanguage,
        source: String,
    },
    External {
        ordinal: u32,
        language: CombinedPageScriptLanguage,
        src: String,
    },
}

/// The parser-selected language and grammar of a combined page declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombinedPageScriptLanguage {
    JavaScript(BlueJsPageScriptKind),
    BlueTs(DirectPageScriptKind),
}

/// Returns every explicit BlueTS script declaration in document order.
///
/// Ordinary JavaScript, `text/typescript`, and unknown `type` values are not
/// reinterpreted as BlueTS. A `src` attribute wins over inline text, matching
/// the browser script-loading shape while leaving source acquisition to an
/// authorized page loader.
pub fn discover_blue_ts_page_scripts(doc: &Document) -> Vec<BlueTsPageScriptDeclaration> {
    let mut declarations = Vec::new();
    collect(doc, doc.root(), &mut declarations);
    declarations
}

/// Returns the supported standard JavaScript declarations in document order.
///
/// This first host recognizes classic scripts with no `type`, an empty type,
/// or one of the common JavaScript MIME types, plus `type="module"`. Unknown
/// types and every explicit BlueTS type are ignored. This classification does
/// not make JavaScript execution the default: the core must explicitly enable
/// its JavaScript page executor.
pub fn discover_blue_js_page_scripts(doc: &Document) -> Vec<BlueJsPageScriptDeclaration> {
    let mut declarations = Vec::new();
    collect_blue_js(doc, doc.root(), &mut declarations);
    declarations
}

/// Returns the supported standard-JavaScript and explicit-BlueTS declarations
/// under one ordinal sequence that preserves their original DOM order.
///
/// This is intentionally a declaration inventory, not a loader. In
/// particular, an `External` item still carries no fetched source, origin,
/// resolver, capability, or execution permission.
pub fn discover_combined_page_scripts(doc: &Document) -> Vec<CombinedPageScriptDeclaration> {
    let mut declarations = Vec::new();
    collect_combined(doc, doc.root(), &mut declarations);
    declarations
}

fn collect(doc: &Document, node: NodeId, declarations: &mut Vec<BlueTsPageScriptDeclaration>) {
    if let NodeData::Element {
        tag_name,
        attributes,
    } = doc.data(node)
    {
        if tag_name == "script" {
            if let Some(kind) = script_kind(attributes) {
                let ordinal = u32::try_from(declarations.len())
                    .expect("a document cannot contain more than u32::MAX script declarations");
                if let Some(src) = attribute(attributes, "src") {
                    declarations.push(BlueTsPageScriptDeclaration::External {
                        ordinal,
                        kind,
                        src: src.to_string(),
                    });
                } else {
                    declarations.push(BlueTsPageScriptDeclaration::Inline {
                        ordinal,
                        kind,
                        source: text_content(doc, node),
                    });
                }
            }
        }
    }
    for child in doc.children(node) {
        collect(doc, child, declarations);
    }
}

fn collect_blue_js(
    doc: &Document,
    node: NodeId,
    declarations: &mut Vec<BlueJsPageScriptDeclaration>,
) {
    if let NodeData::Element {
        tag_name,
        attributes,
    } = doc.data(node)
    {
        if tag_name == "script" {
            if let Some(kind) = blue_js_script_kind(attributes) {
                let ordinal = u32::try_from(declarations.len())
                    .expect("a document cannot contain more than u32::MAX JavaScript scripts");
                if let Some(src) = attribute(attributes, "src") {
                    declarations.push(BlueJsPageScriptDeclaration::External {
                        ordinal,
                        kind,
                        src: src.to_string(),
                    });
                } else {
                    declarations.push(BlueJsPageScriptDeclaration::Inline {
                        ordinal,
                        kind,
                        source: text_content(doc, node),
                    });
                }
            }
        }
    }
    for child in doc.children(node) {
        collect_blue_js(doc, child, declarations);
    }
}

fn collect_combined(
    doc: &Document,
    node: NodeId,
    declarations: &mut Vec<CombinedPageScriptDeclaration>,
) {
    if let NodeData::Element {
        tag_name,
        attributes,
    } = doc.data(node)
    {
        if tag_name == "script" {
            let language = script_kind(attributes)
                .map(CombinedPageScriptLanguage::BlueTs)
                .or_else(|| {
                    blue_js_script_kind(attributes).map(CombinedPageScriptLanguage::JavaScript)
                });
            if let Some(language) = language {
                let ordinal = u32::try_from(declarations.len())
                    .expect("a document cannot contain more than u32::MAX supported scripts");
                if let Some(src) = attribute(attributes, "src") {
                    declarations.push(CombinedPageScriptDeclaration::External {
                        ordinal,
                        language,
                        src: src.to_string(),
                    });
                } else {
                    declarations.push(CombinedPageScriptDeclaration::Inline {
                        ordinal,
                        language,
                        source: text_content(doc, node),
                    });
                }
            }
        }
    }
    for child in doc.children(node) {
        collect_combined(doc, child, declarations);
    }
}

fn script_kind(attributes: &[(String, String)]) -> Option<DirectPageScriptKind> {
    let script_type = attribute(attributes, "type")?;
    if script_type
        .trim()
        .eq_ignore_ascii_case(BLUE_TS_CLASSIC_SCRIPT_TYPE)
    {
        Some(DirectPageScriptKind::Classic)
    } else if script_type
        .trim()
        .eq_ignore_ascii_case(BLUE_TS_MODULE_SCRIPT_TYPE)
    {
        Some(DirectPageScriptKind::Module)
    } else {
        None
    }
}

fn blue_js_script_kind(attributes: &[(String, String)]) -> Option<BlueJsPageScriptKind> {
    let script_type = attribute(attributes, "type").map(str::trim);
    match script_type {
        None | Some("") => Some(BlueJsPageScriptKind::Classic),
        Some(value) if value.eq_ignore_ascii_case("module") => Some(BlueJsPageScriptKind::Module),
        Some(value)
            if [
                "text/javascript",
                "application/javascript",
                "text/ecmascript",
                "application/ecmascript",
            ]
            .iter()
            .any(|mime| value.eq_ignore_ascii_case(mime)) =>
        {
            Some(BlueJsPageScriptKind::Classic)
        }
        Some(_) => None,
    }
}

fn attribute<'a>(attributes: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn text_content(doc: &Document, node: NodeId) -> String {
    doc.children(node)
        .filter_map(|child| match doc.data(child) {
            NodeData::Text { data } => Some(data.as_str()),
            NodeData::Document | NodeData::Element { .. } => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_only_explicit_types_in_document_order() {
        let doc = blueice_html::parse(
            r#"
                <script>const js = true;</script>
                <script type="text/typescript">const ignored: number = 0;</script>
                <script type=" APPLICATION/X-BLUEICE-TYPESCRIPT ">const first: number = 1;</script>
                <div><script type="application/x-blueice-typescript-module">export const second: number = 2;</script></div>
            "#,
        );

        assert_eq!(
            discover_blue_ts_page_scripts(&doc),
            vec![
                BlueTsPageScriptDeclaration::Inline {
                    ordinal: 0,
                    kind: DirectPageScriptKind::Classic,
                    source: "const first: number = 1;".to_string(),
                },
                BlueTsPageScriptDeclaration::Inline {
                    ordinal: 1,
                    kind: DirectPageScriptKind::Module,
                    source: "export const second: number = 2;".to_string(),
                },
            ]
        );
    }

    #[test]
    fn preserves_external_declarations_without_loading_their_source() {
        let doc = blueice_html::parse(
            r#"<script type="application/x-blueice-typescript" src="/app.ts">ignored inline text</script>"#,
        );

        assert_eq!(
            discover_blue_ts_page_scripts(&doc),
            vec![BlueTsPageScriptDeclaration::External {
                ordinal: 0,
                kind: DirectPageScriptKind::Classic,
                src: "/app.ts".to_string(),
            }]
        );
    }

    #[test]
    fn discovers_supported_standard_javascript_without_reinterpreting_bluets() {
        let doc = blueice_html::parse(
            r#"
                <script>const first = 1;</script>
                <script type="application/javascript" src="/classic.js">ignored</script>
                <script type="module">export const second = 2;</script>
                <script type="application/x-blueice-typescript">const typed: number = 3;</script>
                <script type="text/typescript">const ignored: number = 4;</script>
                <script type="not-javascript">ignored</script>
            "#,
        );

        assert_eq!(
            discover_blue_js_page_scripts(&doc),
            vec![
                BlueJsPageScriptDeclaration::Inline {
                    ordinal: 0,
                    kind: BlueJsPageScriptKind::Classic,
                    source: "const first = 1;".to_string(),
                },
                BlueJsPageScriptDeclaration::External {
                    ordinal: 1,
                    kind: BlueJsPageScriptKind::Classic,
                    src: "/classic.js".to_string(),
                },
                BlueJsPageScriptDeclaration::Inline {
                    ordinal: 2,
                    kind: BlueJsPageScriptKind::Module,
                    source: "export const second = 2;".to_string(),
                },
            ]
        );
    }

    #[test]
    fn combined_inventory_preserves_document_order_across_language_lanes() {
        let doc = blueice_html::parse(
            r#"
                <script>const first = 1;</script>
                <script type="application/x-blueice-typescript">const second: number = first + 1;</script>
                <script type="module">export const third = 3;</script>
                <script type="application/x-blueice-typescript-module" src="/fourth.ts"></script>
            "#,
        );

        assert_eq!(
            discover_combined_page_scripts(&doc),
            vec![
                CombinedPageScriptDeclaration::Inline {
                    ordinal: 0,
                    language: CombinedPageScriptLanguage::JavaScript(BlueJsPageScriptKind::Classic,),
                    source: "const first = 1;".to_string(),
                },
                CombinedPageScriptDeclaration::Inline {
                    ordinal: 1,
                    language: CombinedPageScriptLanguage::BlueTs(DirectPageScriptKind::Classic,),
                    source: "const second: number = first + 1;".to_string(),
                },
                CombinedPageScriptDeclaration::Inline {
                    ordinal: 2,
                    language: CombinedPageScriptLanguage::JavaScript(BlueJsPageScriptKind::Module,),
                    source: "export const third = 3;".to_string(),
                },
                CombinedPageScriptDeclaration::External {
                    ordinal: 3,
                    language: CombinedPageScriptLanguage::BlueTs(DirectPageScriptKind::Module,),
                    src: "/fourth.ts".to_string(),
                },
            ]
        );
    }
}
