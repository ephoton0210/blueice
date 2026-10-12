// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
const NODE: &str = r#"
type BlueIceDragCallback = (event: BlueIceDragEvent) => void;
interface BlueIceDragListenerOptions { capture?: boolean; once?: boolean; passive?: boolean; signal?: BlueIceAbortSignal; }
interface BlueIceDragEventListener { handleEvent(event: BlueIceDragEvent): void; }
interface BlueIceDragRemovalOptions { capture?: boolean; }
interface BlueIceNode { textContent: string; appendChild(child: BlueIceNode): BlueIceNode; readonly files: BlueIceFileList | null; value: string | undefined; reset(): void; addEventListener(eventType: 'click', listener: (event: BlueIceClickEvent) => void, options?: boolean | BlueIceDragListenerOptions): void; addEventListener(eventType: 'input' | 'change' | 'cancel', listener: (event: BlueIceSelectionEvent) => void, options?: boolean | BlueIceDragListenerOptions): void; addEventListener(eventType: 'dragstart' | 'drag' | 'dragenter' | 'dragleave' | 'dragover' | 'drop' | 'dragend', listener: BlueIceDragCallback | BlueIceDragEventListener | null, options?: boolean | BlueIceDragListenerOptions): void; removeEventListener(eventType: 'click', listener: (event: BlueIceClickEvent) => void, options?: boolean | BlueIceDragListenerOptions): void; removeEventListener(eventType: 'input' | 'change' | 'cancel', listener: (event: BlueIceSelectionEvent) => void, options?: boolean | BlueIceDragListenerOptions): void; removeEventListener(eventType: 'dragstart' | 'drag' | 'dragenter' | 'dragleave' | 'dragover' | 'drop' | 'dragend', listener: BlueIceDragCallback | BlueIceDragEventListener | null, options?: boolean | BlueIceDragRemovalOptions): void; }
interface BlueIceDocument { textContent: string; appendChild(child: BlueIceNode): BlueIceNode; readonly files: BlueIceFileList | null; value: string | undefined; reset(): void; addEventListener(eventType: 'click', listener: (event: BlueIceClickEvent) => void, options?: boolean | BlueIceDragListenerOptions): void; addEventListener(eventType: 'input' | 'change' | 'cancel', listener: (event: BlueIceSelectionEvent) => void, options?: boolean | BlueIceDragListenerOptions): void; addEventListener(eventType: 'dragstart' | 'drag' | 'dragenter' | 'dragleave' | 'dragover' | 'drop' | 'dragend', listener: BlueIceDragCallback | BlueIceDragEventListener | null, options?: boolean | BlueIceDragListenerOptions): void; removeEventListener(eventType: 'click', listener: (event: BlueIceClickEvent) => void, options?: boolean | BlueIceDragListenerOptions): void; removeEventListener(eventType: 'input' | 'change' | 'cancel', listener: (event: BlueIceSelectionEvent) => void, options?: boolean | BlueIceDragListenerOptions): void; removeEventListener(eventType: 'dragstart' | 'drag' | 'dragenter' | 'dragleave' | 'dragover' | 'drop' | 'dragend', listener: BlueIceDragCallback | BlueIceDragEventListener | null, options?: boolean | BlueIceDragRemovalOptions): void;  getElementById(id: string): BlueIceNode | null; createElement(tagName: string): BlueIceNode; createTextNode(data: string): BlueIceNode; }
declare const document: BlueIceDocument;
"#;

const CONTROLLER: &str = r#"
interface BlueIceAbortController { readonly signal: BlueIceAbortSignal; abort(reason?: unknown): void; }
declare const AbortController: { new(): BlueIceAbortController; readonly prototype: BlueIceAbortController; };
"#;
const SIGNAL: &str = r#"
interface BlueIceAbortSignal extends BlueIceEventTarget {
    readonly aborted: boolean; readonly reason: unknown; throwIfAborted(): void;
    onabort: BlueIceEventCallback | null;
}
declare const AbortSignal: {
    readonly prototype: BlueIceAbortSignal;
    abort(reason?: unknown): BlueIceAbortSignal;
    any(signals: BlueIceAbortSignal[]): BlueIceAbortSignal;
    timeout(milliseconds: number): BlueIceAbortSignal;
};
"#;
pub(super) fn bindings() -> [PageHostDocumentBindingV1; 36] {
    let old = object_urls::bindings();
    let mut bindings = std::array::from_fn(|index| {
        if index < old.len() {
            old[index]
        } else {
            let (stable_id, runtime_binding_id, declaration) = if index == 34 {
                (
                    "abort.global-AbortController",
                    "global.AbortController",
                    CONTROLLER,
                )
            } else {
                ("abort.global-AbortSignal", "global.AbortSignal", SIGNAL)
            };
            PageHostDocumentBindingV1 {
                stable_id,
                declaration,
                role: PageHostBindingRoleV1::Value,
                runtime_binding_id,
                capability: "dom-event",
                feature_flag: "live-dom-abort",
                first_host_api_version: "blueice-core-script-v11",
            }
        }
    });
    bindings[2].declaration = NODE;
    bindings[2].first_host_api_version = "blueice-core-script-v11";
    bindings
}
