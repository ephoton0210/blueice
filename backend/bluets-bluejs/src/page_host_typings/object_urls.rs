// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
const URL: &str = r#"
interface BlueIceURL {
    href: string; readonly origin: string; protocol: string; username: string; password: string;
    host: string; hostname: string; port: string; pathname: string; search: string; hash: string;
    toString(): string; toJSON(): string;
}
declare const URL: {
    new(url: string, base?: string): BlueIceURL; readonly prototype: BlueIceURL;
    canParse(url: string, base?: string): boolean; parse(url: string, base?: string): BlueIceURL | null;
    createObjectURL(blob: BlueIceBlob): string; revokeObjectURL(url: string): void;
};
"#;

pub(super) fn bindings() -> [PageHostDocumentBindingV1; 34] {
    let old = streams::bindings();
    std::array::from_fn(|index| {
        if index < old.len() {
            old[index]
        } else {
            PageHostDocumentBindingV1 {
                stable_id: "object-url.global-URL",
                declaration: URL,
                role: PageHostBindingRoleV1::Value,
                runtime_binding_id: "global.URL",
                capability: "object-url-store",
                feature_flag: "live-dom-object-urls",
                first_host_api_version: "blueice-core-script-v10",
            }
        }
    })
}
