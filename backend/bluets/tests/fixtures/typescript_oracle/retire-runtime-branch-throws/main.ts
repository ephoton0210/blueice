// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export class Box { value: number; constructor(flag: boolean) { if (flag) { this.value = 3; } else { throw new Error("stop"); } } } console.log(new Box(true).value);
