// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

const ERROR_EVENT: &str = r#"
interface BlueIceErrorEvent extends BlueIceEvent {
    readonly message: string; readonly filename: string;
    readonly lineno: number; readonly colno: number; readonly error: unknown;
}
interface BlueIceErrorEventInit extends BlueIceEventInit {
    message?: string; filename?: string; lineno?: number; colno?: number; error?: unknown;
}
declare const ErrorEvent: {
    new(eventType: string, init?: BlueIceErrorEventInit): BlueIceErrorEvent;
    readonly prototype: BlueIceErrorEvent;
};
interface BlueIceWindow extends BlueIceEventTarget {}
"#;

pub(super) fn bindings() -> [PageHostDocumentBindingV1; 58] {
    let old = console::bindings();
    let additions = [
        ("errors.ErrorEvent", "global.ErrorEvent", ERROR_EVENT),
        ("errors.reportError", "global.reportError", "declare function reportError(error: unknown): void;"),
        ("errors.onerror", "global.onerror", "type BlueIceOnError = ((message: string | BlueIceEvent, filename?: string, lineno?: number, colno?: number, error?: unknown) => unknown) | null;\ndeclare let onerror: BlueIceOnError;"),
        ("errors.addEventListener", "global.addEventListener", "declare function addEventListener(eventType: 'error', listener: ((event: BlueIceErrorEvent) => void) | null, options?: boolean | BlueIceDragListenerOptions): void;\ndeclare function addEventListener(eventType: string, listener: BlueIceEventCallback | BlueIceEventListener | null, options?: boolean | BlueIceDragListenerOptions): void;"),
        ("errors.removeEventListener", "global.removeEventListener", "declare function removeEventListener(eventType: 'error', listener: ((event: BlueIceErrorEvent) => void) | null, options?: boolean | BlueIceDragRemovalOptions): void;\ndeclare function removeEventListener(eventType: string, listener: BlueIceEventCallback | BlueIceEventListener | null, options?: boolean | BlueIceDragRemovalOptions): void;"),
        ("errors.dispatchEvent", "global.dispatchEvent", "declare function dispatchEvent(event: BlueIceEvent): boolean;"),
    ];
    std::array::from_fn(|index| {
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
                feature_flag: "page-error-reporting",
                first_host_api_version: "blueice-core-script-v20",
            }
        }
    })
}
