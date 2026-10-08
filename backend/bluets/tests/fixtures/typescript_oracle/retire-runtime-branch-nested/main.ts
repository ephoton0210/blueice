// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export class Box { value: number; constructor(flag: boolean, other: boolean) { if (flag) { if (other) { this.value = 1; } else { this.value = 2; } } else { this.value = 3; } } } console.log(new Box(true, true).value, new Box(true, false).value, new Box(false, false).value);
