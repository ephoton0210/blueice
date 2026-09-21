// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const values: { count: number; index: number } = { count: 1, index: 2 };
const postfix: number = values.count++;
const prefix = ++values["index"];
console.log(`${postfix}:${values.count}:${prefix}:${values["index"]}`);
