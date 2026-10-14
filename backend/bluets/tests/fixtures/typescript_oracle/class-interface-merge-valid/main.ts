// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Box { extra: string; readonly tag: number }
class Box { v: number = 1; }
const b: Box = new Box();
const s: string = b.extra;
const n: number = b.v + b.tag;
console.log(n, s === undefined);
