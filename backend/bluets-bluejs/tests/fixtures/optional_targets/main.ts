// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

let reads = 0;
let gets = 0;
let fallbacks = 0;
function create(present: boolean): {value:number} | undefined {
    reads++;
    if (!present) { return undefined; }
    return { get value(): number { gets++; return 42; } };
}
function fallback(): number { fallbacks++; return 20; }
export const absent = create(false)?.value ?? fallback();
export const present = create(true)?.value ?? fallback();
export const nested = (create(true)?.value ?? fallback()) + 0;
export const effects = [reads, gets, fallbacks];
