// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function describe(value: string): string;
function describe(value: number): number;
function describe(value: string | number): string | number { return value; }
console.log(describe('Ada') + ':' + describe(2));
