// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export class Box { value: string | null = 'Ada'; hasValue(): this is this & { value: string } { return this.value !== null; } assertValue(): asserts this is this & { value: string } { if (this.value === null) { throw 'missing'; } } }
export function read(box: Box): string { box.assertValue(); return box.value; }
console.log(read(new Box()), new Box().hasValue());
