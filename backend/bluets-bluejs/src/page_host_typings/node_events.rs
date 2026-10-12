// This Source Code Form is subject to the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

const NODE: &str = r#"
type BlueIceDragCallback = (event: BlueIceDragEvent) => void;
interface BlueIceDragListenerOptions { capture?: boolean; once?: boolean; passive?: boolean; signal?: BlueIceAbortSignal; }
interface BlueIceDragEventListener { handleEvent(event: BlueIceDragEvent): void; }
interface BlueIceDragRemovalOptions { capture?: boolean; }
interface BlueIceNode extends BlueIceNodeDispatch {
    textContent: string; appendChild(child: BlueIceNode): BlueIceNode;
    readonly files: BlueIceFileList | null; value: string | undefined; reset(): void;
    addEventListener(eventType: 'click', listener: (event: BlueIceClickEvent) => void, options?: boolean | BlueIceDragListenerOptions): void;
    addEventListener(eventType: 'input' | 'change' | 'cancel', listener: (event: BlueIceSelectionEvent) => void, options?: boolean | BlueIceDragListenerOptions): void;
    addEventListener(eventType: 'dragstart' | 'drag' | 'dragenter' | 'dragleave' | 'dragover' | 'drop' | 'dragend', listener: BlueIceDragCallback | BlueIceDragEventListener | null, options?: boolean | BlueIceDragListenerOptions): void;
    addEventListener(eventType: string, listener: BlueIceEventCallback | BlueIceEventListener | null, options?: boolean | BlueIceDragListenerOptions): void;
    removeEventListener(eventType: 'click', listener: (event: BlueIceClickEvent) => void, options?: boolean | BlueIceDragListenerOptions): void;
    removeEventListener(eventType: 'input' | 'change' | 'cancel', listener: (event: BlueIceSelectionEvent) => void, options?: boolean | BlueIceDragListenerOptions): void;
    removeEventListener(eventType: 'dragstart' | 'drag' | 'dragenter' | 'dragleave' | 'dragover' | 'drop' | 'dragend', listener: BlueIceDragCallback | BlueIceDragEventListener | null, options?: boolean | BlueIceDragRemovalOptions): void;
    removeEventListener(eventType: string, listener: BlueIceEventCallback | BlueIceEventListener | null, options?: boolean | BlueIceDragRemovalOptions): void;
}
interface BlueIceDocument extends BlueIceNode {
    getElementById(id: string): BlueIceNode | null; createElement(tagName: string): BlueIceNode; createTextNode(data: string): BlueIceNode;
}
declare const document: BlueIceDocument;
"#;

pub(super) fn bindings() -> [PageHostDocumentBindingV1; 38] {
    let old = custom_events::bindings();
    let mut bindings = std::array::from_fn(|index| {
        if index < old.len() {
            old[index]
        } else {
            PageHostDocumentBindingV1 {
                stable_id: "dom.live-node-dispatch-event",
                declaration: "interface BlueIceNodeDispatch extends BlueIceEventTarget {}",
                role: PageHostBindingRoleV1::Type,
                runtime_binding_id: "node.dispatchEvent",
                capability: "dom-event",
                feature_flag: "live-dom-node-events",
                first_host_api_version: "blueice-core-script-v13",
            }
        }
    });
    bindings[2].declaration = NODE;
    bindings[2].first_host_api_version = "blueice-core-script-v13";
    bindings
}
