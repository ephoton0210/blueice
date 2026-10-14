// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function add(this: void, value: number): number { return value + 1; }
const box = {
    base: 20,
    add(this: {base: number}, value: number): number { return this.base + value; },
    read(this: {base: number}): number { return this.base; }
};
export const direct: number = add(41);
export const method: number = box.add(22);
export const read: number = box.read();
export const directArity: number = add.length;
export const methodArity: number = box.add.length;
export const receiverOnlyArity: number = box.read.length;
