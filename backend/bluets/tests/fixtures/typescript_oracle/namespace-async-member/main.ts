// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    export async function load(): Promise<number> { return 1; }
}
async function main(): Promise<void> {
    const v: number = await N.load();
    console.log(v);
}
main();
