// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import type { ImportItem, RequireItem } from "./barrel";
export function sum(x: ImportItem, y: RequireItem): number {
    const a: 1 = x.value;
    const b: 2 = y.value;
    return a + b;
}
console.log(sum({value:1}, {value:2}));
