// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

const EVENT: &str = r#"
interface BlueIcePromiseRejectionEvent extends BlueIceEvent {
    readonly promise: object; readonly reason: unknown;
}
interface BlueIcePromiseRejectionEventInit extends BlueIceEventInit {
    promise: object; reason?: unknown;
}
interface BlueIcePromiseRejectionListener { handleEvent(event: BlueIcePromiseRejectionEvent): void; }
type BlueIceRejectionHandler = ((event: BlueIcePromiseRejectionEvent) => unknown) | null;
declare const PromiseRejectionEvent: {
    new(eventType: string, init: BlueIcePromiseRejectionEventInit): BlueIcePromiseRejectionEvent;
    readonly prototype: BlueIcePromiseRejectionEvent;
};
"#;

pub(super) fn bindings() -> [PageHostDocumentBindingV1; 61] {
    let old = errors::bindings();
    let additions = [
        (
            "rejections.PromiseRejectionEvent",
            "global.PromiseRejectionEvent",
            EVENT,
        ),
        (
            "rejections.onunhandledrejection",
            "global.onunhandledrejection",
            "declare let onunhandledrejection: BlueIceRejectionHandler;",
        ),
        (
            "rejections.onrejectionhandled",
            "global.onrejectionhandled",
            "declare let onrejectionhandled: BlueIceRejectionHandler;",
        ),
    ];
    let mut bindings = std::array::from_fn(|index| {
        if index < old.len() {
            old[index]
        } else {
            let (stable_id, runtime_binding_id, declaration) = additions[index - old.len()];
            PageHostDocumentBindingV1 {
                stable_id,
                runtime_binding_id,
                declaration,
                role: PageHostBindingRoleV1::Value,
                capability: "script-diagnostics",
                feature_flag: "page-rejection-tracking",
                first_host_api_version: "blueice-core-script-v21",
            }
        }
    });
    bindings[55].declaration=concat!(
        "declare function addEventListener(eventType: 'unhandledrejection' | 'rejectionhandled', listener: ((event: BlueIcePromiseRejectionEvent) => void) | BlueIcePromiseRejectionListener | null, options?: boolean | BlueIceDragListenerOptions): void;\n",
        "declare function addEventListener(eventType: 'error', listener: ((event: BlueIceErrorEvent) => void) | null, options?: boolean | BlueIceDragListenerOptions): void;\ndeclare function addEventListener(eventType: string, listener: BlueIceEventCallback | BlueIceEventListener | null, options?: boolean | BlueIceDragListenerOptions): void;"
    );
    bindings[56].declaration=concat!(
        "declare function removeEventListener(eventType: 'unhandledrejection' | 'rejectionhandled', listener: ((event: BlueIcePromiseRejectionEvent) => void) | BlueIcePromiseRejectionListener | null, options?: boolean | BlueIceDragRemovalOptions): void;\n",
        "declare function removeEventListener(eventType: 'error', listener: ((event: BlueIceErrorEvent) => void) | null, options?: boolean | BlueIceDragRemovalOptions): void;\ndeclare function removeEventListener(eventType: string, listener: BlueIceEventCallback | BlueIceEventListener | null, options?: boolean | BlueIceDragRemovalOptions): void;"
    );
    bindings[55].first_host_api_version = "blueice-core-script-v21";
    bindings[56].first_host_api_version = "blueice-core-script-v21";
    bindings
}
