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
}
