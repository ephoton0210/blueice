// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original declarations supplementing the preserved v1 source files.
interface Array<T> {
    reduce(callback: (accumulator: T, item: T, index: number, items: T[]) => T): T;
    reduce(callback: (accumulator: T, item: T, index: number, items: T[]) => T, initialValue: T): T;
    reduce<U>(callback: (accumulator: U, item: T, index: number, items: T[]) => U, initialValue: U): U;
}

declare namespace Symbol {
    export const iterator: symbol;
    export const asyncIterator: symbol;
}

interface BigInt {
    toString(radix?: number): string;
    valueOf(): bigint;
    toLocaleString(): string;
}
