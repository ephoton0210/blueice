// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import generate from "./dep.ts";
async function run(): Promise<void> {
    const iterator = generate();
    const first = await iterator.next();
    const second = await iterator.next();
    console.log(generate.name, first.value, second.value);
}
run();
