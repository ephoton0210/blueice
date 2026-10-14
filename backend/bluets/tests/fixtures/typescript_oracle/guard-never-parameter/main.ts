// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function unreachable(value: never): never { throw value; } export function read(value: 'one' | 'two'): number { switch (value) { case 'one': return 1; case 'two': return 2; default: return unreachable(value); } }
