// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export class Counter { get thrown() { throw new Error("bad"); } get looped() { while (true) {} } get bare() { return; } get absent() { return void 0; } }
