// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function read(box: { value: string | number }): number { if (typeof box.value === 'string') { const text: string = box.value; return 1; } const count: number = box.value; return count; }
