// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export class Box { constructor(private flag: boolean) {} get value() { if (this.flag) { return 1; } else { return "text"; } } } console.log(new Box(true).value, new Box(false).value);
