// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare function takeNumber(value: number): void;
declare function takeString(value: string): void;
class Left { left: number = 1; } class Right { right: string = "r"; }
function probe(value: Left | Right): void { if (value instanceof Left) { takeNumber(value.left); } else { takeString(value.right); } }
export {};
