// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function counter(): number { return counter.count; }
namespace counter {
    export let count: number = 5;
}
counter.count = counter.count + 1;
console.log(counter(), counter.count);
