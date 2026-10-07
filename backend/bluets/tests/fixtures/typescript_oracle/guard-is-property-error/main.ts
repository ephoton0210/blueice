// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function isString(value: string | number): value is string { return typeof value === 'string'; } function takesNumber(value: number): void {} export function read(box: { value: string | number }): void { if (isString(box.value)) { takesNumber(box.value); } }
