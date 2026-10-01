// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace R {
    export let counter: number = 0;
    export const label: string = "L";
    export function inc(): number { counter += 1; return counter; }
    export function pair(): { counter: number; label: string } {
        return { counter, label };
    }
    export function text(): string { return `${label}:${counter}`; }
    export function kind(): string { return typeof counter; }
    export class Holder {
        counter: number = 100;
        read(): number { return this.counter + counter; }
    }
    export const keyed: number = { counter: 5 }.counter;
}
R.inc();
R.inc();
console.log(R.counter, R.pair(), R.text(), R.kind(), new R.Holder().read(), R.keyed);
