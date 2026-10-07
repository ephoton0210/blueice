// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Reader {read(value: string): number} const reader: Reader = {read(value: string): number {return value.length;}}; export function run(): number {return reader.read('answer');} console.log(run());
