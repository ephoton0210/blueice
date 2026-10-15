// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
const CONSOLE: &str = r#"
interface BlueIceConsole {
    assert(condition?: boolean, ...data: unknown[]): void; clear(): void;
    debug(...data: unknown[]): void; error(...data: unknown[]): void;
    info(...data: unknown[]): void; log(...data: unknown[]): void;
    table(tabularData?: unknown, properties?: string[]): void;
    trace(...data: unknown[]): void; warn(...data: unknown[]): void;
    dir(item?: unknown, options?: object | null): void; dirxml(...data: unknown[]): void;
    count(label?: string): void; countReset(label?: string): void;
    group(...data: unknown[]): void; groupCollapsed(...data: unknown[]): void; groupEnd(): void;
    time(label?: string): void; timeLog(label?: string, ...data: unknown[]): void; timeEnd(label?: string): void;
}
declare const console: BlueIceConsole;
"#;
pub(super) fn bindings() -> [PageHostDocumentBindingV1; 52] {
    let old = window_aliases::bindings();
    std::array::from_fn(|index| {
        if index < old.len() {
            old[index]
        } else {
            PageHostDocumentBindingV1 {
                stable_id: "console.namespace",
                declaration: CONSOLE,
                role: PageHostBindingRoleV1::Value,
                runtime_binding_id: "global.console",
                capability: "script-diagnostics",
                feature_flag: "page-console",
                first_host_api_version: "blueice-core-script-v19",
            }
        }
    })
}
