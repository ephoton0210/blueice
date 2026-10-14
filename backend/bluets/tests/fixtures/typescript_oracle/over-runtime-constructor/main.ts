// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export class Box { value: string | number; constructor(value: string); constructor(value: number); constructor(value: string | number) { this.value = value; } } console.log(new Box('Ada').value, new Box(42).value);
