// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function choose(flag: boolean): { value: number } | null {
    return flag ? { value: 41 } : null;
}

const full: { value: number } | null = choose(true);
const empty: { value: number } | null = choose(false);
const first: number = full?.value ?? 0;
const second: number = empty?.value ?? 0;
console.log(first + second);
