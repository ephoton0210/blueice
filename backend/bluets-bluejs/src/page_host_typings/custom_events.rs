// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

const CUSTOM_EVENT: &str = r#"
interface BlueIceCustomEvent extends BlueIceEvent {
    readonly detail: unknown;
    initCustomEvent(eventType: string, bubbles?: boolean, cancelable?: boolean, detail?: unknown): void;
}
declare const CustomEvent: {
    new(eventType: string, options?: { bubbles?: boolean; cancelable?: boolean; composed?: boolean; detail?: unknown; }): BlueIceCustomEvent;
    readonly prototype: BlueIceCustomEvent;
};
"#;

pub(super) fn bindings() -> [PageHostDocumentBindingV1; 37] {
    let old = abort::bindings();
    std::array::from_fn(|index| {
        if index < old.len() {
            old[index]
        } else {
            PageHostDocumentBindingV1 {
                stable_id: "event.global-CustomEvent",
                declaration: CUSTOM_EVENT,
                role: PageHostBindingRoleV1::Value,
                runtime_binding_id: "global.CustomEvent",
                capability: "dom-event",
                feature_flag: "live-dom-custom-event",
                first_host_api_version: "blueice-core-script-v12",
            }
        }
    })
}
