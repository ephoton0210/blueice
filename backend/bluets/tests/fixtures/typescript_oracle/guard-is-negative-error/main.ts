// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function isString(value: string | number): value is string { return typeof value === 'string'; } function takesString(value: string): void {} export function read(value: string | number): void { if (!isString(value)) { takesString(value); } }
