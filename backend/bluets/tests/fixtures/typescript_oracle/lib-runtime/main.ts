// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(...values: any[]): void };
const values: number[] = [1, 2];
const label: string = "blue";
const pattern: RegExp = new RegExp("b");
const epoch: Date = new Date(0);
console.log(values.push(3), label.repeat(2), Math.sqrt(9), pattern.test(label), epoch.getTime());
export {};
