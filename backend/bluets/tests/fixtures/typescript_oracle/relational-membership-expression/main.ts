// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const record: { label: string } = { label: "Ada" };
const hasLabel: boolean = "label" in record;
const value = new Object();
const isObject: boolean = value instanceof Object;
console.log(hasLabel && isObject);
