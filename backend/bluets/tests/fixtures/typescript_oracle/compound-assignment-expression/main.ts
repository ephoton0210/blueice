// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

let value: number = 2;
value **= 3;
value <<= 1;
value |= 1;
value ^= 3;
value &= 14;
value >>= 1;
value >>>= 0;
let missing: number | undefined = undefined;
missing ??= 42;
let zero: number = 0;
zero ||= 5;
let present: number = 7;
present &&= 2;
console.log(`${value}:${missing}:${zero}:${present}`);
