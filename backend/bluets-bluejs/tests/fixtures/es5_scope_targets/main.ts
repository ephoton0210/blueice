// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const callbacks: (() => number)[] = [];
let value = 1;
{ let value = 20; callbacks.push(() => value); }
{ const value = 22; callbacks.push(() => value); }
for (let index = 0; index < 3; index++) { callbacks.push(() => index); }
export const result = callbacks[0]() + callbacks[1]();
export const outside = value;
export function observed(): number[] {
    return [callbacks[0](), callbacks[1](), callbacks[2](), callbacks[3](), callbacks[4]()];
}
export function receiver(this: {value:number}): number {
    const read = () => this.value;
    return read();
}
