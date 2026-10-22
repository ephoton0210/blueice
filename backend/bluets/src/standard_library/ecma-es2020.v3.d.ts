// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original finite ECMAScript type model for G-T7; not upstream library text.
interface BigIntConstructor { (value: bigint | boolean | number | string): bigint; asIntN(bits: number, value: bigint): bigint; asUintN(bits: number, value: bigint): bigint; readonly prototype: BigInt; }
declare const BigInt: BigIntConstructor;
interface DataView<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> { getBigInt64(byteOffset: number, littleEndian?: boolean): bigint; getBigUint64(byteOffset: number, littleEndian?: boolean): bigint; setBigInt64(byteOffset: number, value: bigint, littleEndian?: boolean): void; setBigUint64(byteOffset: number, value: bigint, littleEndian?: boolean): void; }
interface BigInt64Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> {
    [index: number]: bigint;
    readonly buffer: TArrayBuffer;
    readonly byteLength: number;
    readonly byteOffset: number;
    readonly length: number;
    readonly BYTES_PER_ELEMENT: number;
    copyWithin(target: number, start: number, end?: number): BigInt64Array<TArrayBuffer>;
    fill(value: bigint, start?: number, end?: number): BigInt64Array<TArrayBuffer>;
    filter(predicate: (value: bigint, index: number, array: BigInt64Array<TArrayBuffer>) => any, thisArg?: any): BigInt64Array<ArrayBuffer>;
    find(predicate: (value: bigint, index: number, array: BigInt64Array<TArrayBuffer>) => unknown, thisArg?: any): bigint | undefined;
    findIndex(predicate: (value: bigint, index: number, array: BigInt64Array<TArrayBuffer>) => unknown, thisArg?: any): number;
    every(predicate: (value: bigint, index: number, array: BigInt64Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    some(predicate: (value: bigint, index: number, array: BigInt64Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    forEach(callback: (value: bigint, index: number, array: BigInt64Array<TArrayBuffer>) => void, thisArg?: any): void;
    indexOf(value: bigint, start?: number): number;
    lastIndexOf(value: bigint, start?: number): number;
    join(separator?: string): string;
    map(callback: (value: bigint, index: number, array: BigInt64Array<TArrayBuffer>) => bigint, thisArg?: any): BigInt64Array<ArrayBuffer>;
    reduce(callback: (previous: bigint, value: bigint, index: number, array: BigInt64Array<TArrayBuffer>) => bigint, initialValue?: bigint): bigint;
    reduce<U>(callback: (previous: U, value: bigint, index: number, array: BigInt64Array<TArrayBuffer>) => U, initialValue: U): U;
    reverse(): BigInt64Array<TArrayBuffer>;
    set(array: ArrayLike<bigint>, offset?: number): void;
    slice(start?: number, end?: number): BigInt64Array<ArrayBuffer>;
    sort(compare?: (left: bigint, right: bigint) => number): BigInt64Array<TArrayBuffer>;
    subarray(begin?: number, end?: number): BigInt64Array<TArrayBuffer>;
    toString(): string;
    toLocaleString(): string;
    entries(): ArrayIterator<[number, bigint]>;
    keys(): ArrayIterator<number>;
    values(): ArrayIterator<bigint>;
    [Symbol.iterator](): ArrayIterator<bigint>;
}
interface BigInt64ArrayConstructor {
    new (length: number): BigInt64Array<ArrayBuffer>;
    new (array: ArrayLike<bigint>): BigInt64Array<ArrayBuffer>;
    new <TArrayBuffer extends ArrayBufferLike = ArrayBuffer>(buffer: TArrayBuffer, byteOffset?: number, length?: number): BigInt64Array<TArrayBuffer>;
    readonly prototype: BigInt64Array<ArrayBufferLike>;
    readonly BYTES_PER_ELEMENT: number;
    of(...items: bigint[]): BigInt64Array<ArrayBuffer>;
    from(array: ArrayLike<bigint>): BigInt64Array<ArrayBuffer>;
    from<T>(array: ArrayLike<T>, callback: (value: T, index: number) => bigint, thisArg?: any): BigInt64Array<ArrayBuffer>;
}
declare const BigInt64Array: BigInt64ArrayConstructor;
interface BigUint64Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> {
    [index: number]: bigint;
    readonly buffer: TArrayBuffer;
    readonly byteLength: number;
    readonly byteOffset: number;
    readonly length: number;
    readonly BYTES_PER_ELEMENT: number;
    copyWithin(target: number, start: number, end?: number): BigUint64Array<TArrayBuffer>;
    fill(value: bigint, start?: number, end?: number): BigUint64Array<TArrayBuffer>;
    filter(predicate: (value: bigint, index: number, array: BigUint64Array<TArrayBuffer>) => any, thisArg?: any): BigUint64Array<ArrayBuffer>;
    find(predicate: (value: bigint, index: number, array: BigUint64Array<TArrayBuffer>) => unknown, thisArg?: any): bigint | undefined;
    findIndex(predicate: (value: bigint, index: number, array: BigUint64Array<TArrayBuffer>) => unknown, thisArg?: any): number;
    every(predicate: (value: bigint, index: number, array: BigUint64Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    some(predicate: (value: bigint, index: number, array: BigUint64Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    forEach(callback: (value: bigint, index: number, array: BigUint64Array<TArrayBuffer>) => void, thisArg?: any): void;
    indexOf(value: bigint, start?: number): number;
    lastIndexOf(value: bigint, start?: number): number;
    join(separator?: string): string;
    map(callback: (value: bigint, index: number, array: BigUint64Array<TArrayBuffer>) => bigint, thisArg?: any): BigUint64Array<ArrayBuffer>;
    reduce(callback: (previous: bigint, value: bigint, index: number, array: BigUint64Array<TArrayBuffer>) => bigint, initialValue?: bigint): bigint;
    reduce<U>(callback: (previous: U, value: bigint, index: number, array: BigUint64Array<TArrayBuffer>) => U, initialValue: U): U;
    reverse(): BigUint64Array<TArrayBuffer>;
    set(array: ArrayLike<bigint>, offset?: number): void;
    slice(start?: number, end?: number): BigUint64Array<ArrayBuffer>;
    sort(compare?: (left: bigint, right: bigint) => number): BigUint64Array<TArrayBuffer>;
    subarray(begin?: number, end?: number): BigUint64Array<TArrayBuffer>;
    toString(): string;
    toLocaleString(): string;
    entries(): ArrayIterator<[number, bigint]>;
    keys(): ArrayIterator<number>;
    values(): ArrayIterator<bigint>;
    [Symbol.iterator](): ArrayIterator<bigint>;
}
interface BigUint64ArrayConstructor {
    new (length: number): BigUint64Array<ArrayBuffer>;
    new (array: ArrayLike<bigint>): BigUint64Array<ArrayBuffer>;
    new <TArrayBuffer extends ArrayBufferLike = ArrayBuffer>(buffer: TArrayBuffer, byteOffset?: number, length?: number): BigUint64Array<TArrayBuffer>;
    readonly prototype: BigUint64Array<ArrayBufferLike>;
    readonly BYTES_PER_ELEMENT: number;
    of(...items: bigint[]): BigUint64Array<ArrayBuffer>;
    from(array: ArrayLike<bigint>): BigUint64Array<ArrayBuffer>;
    from<T>(array: ArrayLike<T>, callback: (value: T, index: number) => bigint, thisArg?: any): BigUint64Array<ArrayBuffer>;
}
declare const BigUint64Array: BigUint64ArrayConstructor;
interface PromiseFulfilledResult<T> { status: "fulfilled"; value: T; }
interface PromiseRejectedResult { status: "rejected"; reason: any; }
type PromiseSettledResult<T> = PromiseFulfilledResult<T> | PromiseRejectedResult;
interface PromiseConstructor {
    allSettled<T extends readonly unknown[] | []>(values: T): Promise<{ -readonly [P in keyof T]: PromiseSettledResult<Awaited<T[P]>> }>;
    allSettled<T>(values: Iterable<T | PromiseLike<T>>): Promise<PromiseSettledResult<Awaited<T>>[]>;
}
declare namespace Atomics {
    export function add(typedArray: BigInt64Array | BigUint64Array, index: number, value: bigint): bigint;
    export function and(typedArray: BigInt64Array | BigUint64Array, index: number, value: bigint): bigint;
    export function exchange(typedArray: BigInt64Array | BigUint64Array, index: number, value: bigint): bigint;
    export function or(typedArray: BigInt64Array | BigUint64Array, index: number, value: bigint): bigint;
    export function sub(typedArray: BigInt64Array | BigUint64Array, index: number, value: bigint): bigint;
    export function xor(typedArray: BigInt64Array | BigUint64Array, index: number, value: bigint): bigint;
    export function compareExchange(typedArray: BigInt64Array | BigUint64Array, index: number, expectedValue: bigint, replacementValue: bigint): bigint;
    export function load(typedArray: BigInt64Array | BigUint64Array, index: number): bigint;
    export function store(typedArray: BigInt64Array | BigUint64Array, index: number, value: bigint): bigint;
    export function wait(typedArray: BigInt64Array, index: number, value: bigint, timeout?: number): "ok" | "not-equal" | "timed-out";
    export function notify(typedArray: BigInt64Array, index: number, count?: number): number;
}
declare namespace Intl {
    export type RelativeTimeFormatUnit = "year" | "years" | "quarter" | "quarters" | "month" | "months" | "week" | "weeks" | "day" | "days" | "hour" | "hours" | "minute" | "minutes" | "second" | "seconds";
    export interface RelativeTimeFormatOptions { localeMatcher?: "lookup" | "best fit"; numeric?: "always" | "auto"; style?: "long" | "short" | "narrow"; }
    export interface RelativeTimeFormat { format(value: number, unit: Intl.RelativeTimeFormatUnit): string; resolvedOptions(): Intl.ResolvedRelativeTimeFormatOptions; }
    export interface ResolvedRelativeTimeFormatOptions { locale: string; style: "long" | "short" | "narrow"; numeric: "always" | "auto"; numberingSystem: string; }
    export interface RelativeTimeFormatConstructor { new (locales?: string | readonly string[], options?: Intl.RelativeTimeFormatOptions): Intl.RelativeTimeFormat; supportedLocalesOf(locales: string | readonly string[], options?: Intl.RelativeTimeFormatOptions): string[]; }
    export const RelativeTimeFormat: Intl.RelativeTimeFormatConstructor;
    export interface LocaleOptions { language?: string; script?: string; region?: string; calendar?: string; collation?: string; hourCycle?: "h11" | "h12" | "h23" | "h24"; caseFirst?: "upper" | "lower" | "false"; numeric?: boolean; numberingSystem?: string; }
    export interface Locale { readonly baseName: string; readonly language: string; readonly script?: string; readonly region?: string; maximize(): Intl.Locale; minimize(): Intl.Locale; toString(): string; }
    export interface LocaleConstructor { new (tag: string | Intl.Locale, options?: Intl.LocaleOptions): Intl.Locale; readonly prototype: Intl.Locale; }
    export const Locale: Intl.LocaleConstructor;
}
