// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(value: string): void };
function count(value: number) { return value + 1; }
function pick(flag: boolean) { if (flag) { return "left"; } return "right"; }
class Counter { value: number = 2; get current() { return this.value; } doubled() { return this.value * 2; } }
console.log(count(1) + ":" + pick(true) + ":" + new Counter().current + ":" + new Counter().doubled());
