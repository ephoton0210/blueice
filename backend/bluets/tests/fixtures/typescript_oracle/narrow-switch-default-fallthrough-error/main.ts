// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function read(value: 1 | 2 | 3): number { switch (value) { default: case 2: { const expected: 2 = value; return expected; } case 1: return 1; } }
