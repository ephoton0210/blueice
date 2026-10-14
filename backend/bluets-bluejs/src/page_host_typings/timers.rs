// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn bindings() -> [PageHostDocumentBindingV1; 45] {
    let old = iteration::bindings();
    let timers = [
        ("timers.set-timeout", "global.setTimeout", "declare function setTimeout(callback: (...args: any[]) => void, delay?: number, ...args: any[]): number;"),
        ("timers.set-interval", "global.setInterval", "declare function setInterval(callback: (...args: any[]) => void, delay?: number, ...args: any[]): number;"),
        ("timers.clear-timeout", "global.clearTimeout", "declare function clearTimeout(id?: number): void;"),
        ("timers.clear-interval", "global.clearInterval", "declare function clearInterval(id?: number): void;"),
        ("timers.queue-microtask", "global.queueMicrotask", "declare function queueMicrotask(callback: () => void): void;"),
    ];
    std::array::from_fn(|index| {
        if index < old.len() {
            old[index]
        } else {
            let (stable_id, runtime_binding_id, declaration) = timers[index - old.len()];
            PageHostDocumentBindingV1 {
                stable_id,
                declaration,
                role: PageHostBindingRoleV1::Value,
                runtime_binding_id,
                capability: "script-scheduling",
                feature_flag: "live-dom-timers",
                first_host_api_version: "blueice-core-script-v16",
            }
        }
    })
}
