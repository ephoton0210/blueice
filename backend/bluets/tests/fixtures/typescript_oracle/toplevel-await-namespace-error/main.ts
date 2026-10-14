// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
async function load(n: number): Promise<number> { return n; }
export namespace N { export const a: number = await load(1); }
