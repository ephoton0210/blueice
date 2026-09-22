// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function label<T extends string>(value: T): T;
function label(value: string): string { return value; }
console.log(label("Ada"));
