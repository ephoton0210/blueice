// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace G {
    export async function twice(n: number): Promise<number> { return n * 2; }
    export const base: number = 21;
}
async function main(): Promise<void> {
    console.log(await G.twice(G.base));
}
main();
