// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function measure(value: string | null | undefined): number { if (value == null) { return 0; } return value.length; }
console.log(measure(null),measure(undefined),measure("abc"));
