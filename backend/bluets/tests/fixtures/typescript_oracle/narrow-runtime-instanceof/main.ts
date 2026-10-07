// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export class Left { left: number = 5; } export class Right { right: string = "ab"; } export function measure(value: Left | Right): number { if (value instanceof Left) { return value.left; } return value.right.length; }
console.log(measure(new Left()), measure(new Right()));
