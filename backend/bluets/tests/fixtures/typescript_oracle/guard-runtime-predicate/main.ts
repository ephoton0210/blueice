// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function isString(value: string | number): value is string { return typeof value === 'string'; } export function read(value: string | number): number { if (isString(value)) { return value.length; } return value; } console.log(read('Ada'), read(42));
