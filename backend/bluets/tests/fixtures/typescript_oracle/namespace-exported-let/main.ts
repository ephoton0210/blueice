// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace Counter {
    export let count: number = 0;
    export function bump(): number { count = count + 1; return count; }
}
Counter.bump();
Counter.count = Counter.count + 10;
console.log(Counter.count);
