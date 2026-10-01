// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace Util {
    export function identity<T>(value: T): T { return value; }
    export function first<T>(items: T[]): T { return items[0]; }
}
const n: number = Util.identity(3);
const s: string = Util.first(["a", "b"]);
console.log(n, s);
