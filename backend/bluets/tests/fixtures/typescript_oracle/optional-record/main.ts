// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface OptionalName { name?: string }
const source: OptionalName = {};
const value: string | number | undefined = source.name;
console.log(value === undefined ? "missing" : value);
