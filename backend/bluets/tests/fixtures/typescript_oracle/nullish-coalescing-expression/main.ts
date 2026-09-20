// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function choose(value: number | null): number | null { return value; }
const optional: string | undefined = undefined;
const label: string = optional ?? 'guest';
const count: number = choose(null) ?? 42;
console.log(`${label}:${count}`);
