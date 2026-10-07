// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function assertString(value: unknown): asserts value is string { if (typeof value !== 'string') { throw 'wrong'; } } export function read(value: unknown): string { assertString(value); return value; } console.log(read('Ada')); try { read(42); } catch (error) { console.log(String(error)); }
