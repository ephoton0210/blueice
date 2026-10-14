// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn bindings() -> [PageHostDocumentBindingV1; 47] {
    let old = timers::bindings();
    let frames = [
        ("frames.request", "global.requestAnimationFrame", "declare function requestAnimationFrame(callback: (timestamp: number) => void): number;"),
        ("frames.cancel", "global.cancelAnimationFrame", "declare function cancelAnimationFrame(handle: number): void;"),
    ];
    std::array::from_fn(|index| {
        if index < old.len() {
            old[index]
        } else {
            let (stable_id, runtime_binding_id, declaration) = frames[index - old.len()];
            PageHostDocumentBindingV1 {
                stable_id,
                runtime_binding_id,
                declaration,
                role: PageHostBindingRoleV1::Value,
                capability: "script-scheduling",
                feature_flag: "live-dom-animation-frame",
                first_host_api_version: "blueice-core-script-v17",
            }
        }
    })
}
