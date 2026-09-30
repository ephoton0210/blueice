// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

let value: number = 1;
const postfix = value++;
const prefix = ++value;
const decremented = --value;
const tail = value--;
console.log(`${postfix}:${prefix}:${decremented}:${tail}:${value}`);
