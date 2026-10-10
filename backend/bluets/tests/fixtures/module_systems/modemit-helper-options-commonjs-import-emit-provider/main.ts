// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base { value: number; constructor(value: number) { this.value = value; } }
export class Derived extends Base { constructor() { super(42); } }
export const result = new Derived().value;
