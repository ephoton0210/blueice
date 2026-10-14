// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Reader {
    read(value: number): number;
    read(value: string): string;
    read(value: number | string): number | string { return value; }
    fromThis(value: number): number { return this.read(value); }
}
const reader = new Reader();
const count: number = reader.read(1);
const text: string = reader.read('x');
