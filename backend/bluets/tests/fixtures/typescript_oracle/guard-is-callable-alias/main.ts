// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function isString(value: unknown): value is string { return typeof value === 'string'; } const check = isString; export function read(value: unknown): string { if (check(value)) { return value; } return ''; }
