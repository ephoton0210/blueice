// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Counter {
    static read(value: number): number { return value; }
    instance(): number { return 1; }
}
const numberValue: number = Counter.read(1);
const reader: (value: number) => number = Counter.read;
const counter = new Counter();
const instanceValue: number = counter.instance();
