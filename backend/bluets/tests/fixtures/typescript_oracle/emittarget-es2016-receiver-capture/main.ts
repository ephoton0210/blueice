// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
export class Box {
  constructor(public value: number) {}
  read(extra: number): number {
    const add = (): number => this.value + arguments[0];
    return add();
  }
}
console.log(new Box(20).read(22));
