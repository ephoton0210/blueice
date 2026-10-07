// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function read(input: string | number, flag: boolean): string { let value: string | number = input; (() => { if (flag) { value = 'one'; } else { value = 'two'; } })(); return value; }
