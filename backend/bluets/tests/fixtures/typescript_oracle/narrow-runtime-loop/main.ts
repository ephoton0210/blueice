// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function measure(value: string | number): number { while (typeof value === "string") { value = value.length; } return value; }
console.log(measure("abc"),measure(8));
