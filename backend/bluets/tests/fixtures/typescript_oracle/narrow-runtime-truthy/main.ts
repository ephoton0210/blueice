// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function measure(value: string | null): number { if (value) { return value.length; } return 0; }
console.log(measure(null),measure(""),measure("ab"));
