// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original finite ECMAScript type model for G-T7; not upstream library text.
interface Array<T> {
    find<S extends T>(predicate: (value: T, index: number, items: T[]) => value is S, thisArg?: any): S | undefined;
    find(predicate: (value: T, index: number, items: T[]) => unknown, thisArg?: any): T | undefined;
    findIndex(predicate: (value: T, index: number, items: T[]) => unknown, thisArg?: any): number;
    fill(value: T, start?: number, end?: number): T[];
    copyWithin(target: number, start: number, end?: number): T[];
    entries(): ArrayIterator<[number, T]>;
    keys(): ArrayIterator<number>;
    values(): ArrayIterator<T>;
    [Symbol.iterator](): ArrayIterator<T>;
}
interface ReadonlyArray<T> {
    find<S extends T>(predicate: (value: T, index: number, items: readonly T[]) => value is S, thisArg?: any): S | undefined;
    find(predicate: (value: T, index: number, items: readonly T[]) => unknown, thisArg?: any): T | undefined;
    findIndex(predicate: (value: T, index: number, items: readonly T[]) => unknown, thisArg?: any): number;
    entries(): ArrayIterator<[number, T]>;
    keys(): ArrayIterator<number>;
    values(): ArrayIterator<T>;
    [Symbol.iterator](): ArrayIterator<T>;
}
interface ArrayConstructor {
    from<T>(items: Iterable<T> | ArrayLike<T>): T[];
    from<T, U>(items: Iterable<T> | ArrayLike<T>, callback: (value: T, index: number) => U, thisArg?: any): U[];
    of<T>(...items: T[]): T[];
}
interface String { codePointAt(position: number): number | undefined; normalize(form?: "NFC" | "NFD" | "NFKC" | "NFKD"): string; [Symbol.iterator](): StringIterator<string>; }
interface StringConstructor { fromCodePoint(...codes: number[]): string; raw(template: {raw: readonly string[] | ArrayLike<string>}, ...substitutions: any[]): string; }
interface NumberConstructor { isFinite(value: unknown): boolean; isNaN(value: unknown): boolean; isInteger(value: unknown): boolean; isSafeInteger(value: unknown): boolean; parseFloat(value: string): number; parseInt(value: string, radix?: number): number; readonly EPSILON: number; readonly MAX_SAFE_INTEGER: number; readonly MIN_SAFE_INTEGER: number; }
interface ObjectConstructor {
    assign<T extends object, U>(target: T, source: U): T & U;
    assign<T extends object, U, V>(target: T, first: U, second: V): T & U & V;
    assign<T extends object, U, V, W>(target: T, first: U, second: V, third: W): T & U & V & W;
    assign(target: object, ...sources: any[]): any;
    is(left: any, right: any): boolean;
    getOwnPropertySymbols(value: any): symbol[];
    setPrototypeOf(value: any, prototype: object | null): any;
}
interface Iterable<T, TReturn = any, TNext = any> { [Symbol.iterator](): Iterator<T, TReturn, TNext>; }
interface IteratorObject<T, TReturn = unknown, TNext = unknown> extends Iterator<T, TReturn, TNext> { [Symbol.iterator](): IteratorObject<T, TReturn, TNext>; }
type BuiltinIteratorReturn = undefined;
interface ArrayIterator<T> extends IteratorObject<T, BuiltinIteratorReturn, unknown> { [Symbol.iterator](): ArrayIterator<T>; }
interface MapIterator<T> extends IteratorObject<T, BuiltinIteratorReturn, unknown> { [Symbol.iterator](): MapIterator<T>; }
interface SetIterator<T> extends IteratorObject<T, BuiltinIteratorReturn, unknown> { [Symbol.iterator](): SetIterator<T>; }
interface StringIterator<T> extends IteratorObject<T, BuiltinIteratorReturn, unknown> { [Symbol.iterator](): StringIterator<T>; }
interface IterableIterator<T, R = any, N = any> { [Symbol.iterator](): IterableIterator<T, R, N>; }
interface Generator<T = unknown, R = any, N = any> { [Symbol.iterator](): Generator<T, R, N>; }
interface Map<K, V> {
    entries(): MapIterator<[K, V]>;
    keys(): MapIterator<K>;
    values(): MapIterator<V>;
    forEach(callback: (value: V, key: K, map: Map<K, V>) => void, thisArg?: any): void;
    [Symbol.iterator](): MapIterator<[K, V]>;
}
interface MapConstructor {
    new (): Map<any, any>;
    new <K, V>(entries?: readonly (readonly [K, V])[] | null): Map<K, V>;
    new <K, V>(entries?: Iterable<readonly [K, V]> | null): Map<K, V>;
    readonly prototype: Map<any, any>;
}
declare const Map: MapConstructor;
interface Set<T> { entries(): SetIterator<[T, T]>; keys(): SetIterator<T>; values(): SetIterator<T>; forEach(callback: (value: T, key: T, set: Set<T>) => void, thisArg?: any): void; [Symbol.iterator](): SetIterator<T>; }
interface SetConstructor { new <T = any>(values?: readonly T[] | null): Set<T>; new <T>(values?: Iterable<T> | null): Set<T>; readonly prototype: Set<any>; }
declare const Set: SetConstructor;
interface WeakKeyTypes { object: object; }
type WeakKey = WeakKeyTypes[keyof WeakKeyTypes];
interface WeakMapConstructor { new <K extends WeakKey = WeakKey, V = any>(entries?: readonly (readonly [K, V])[] | null): WeakMap<K, V>; new <K extends WeakKey, V>(entries: Iterable<readonly [K, V]>): WeakMap<K, V>; readonly prototype: WeakMap<WeakKey, any>; }
declare const WeakMap: WeakMapConstructor;
interface WeakSet<T extends WeakKey> { add(value: T): WeakSet<T>; has(value: T): boolean; delete(value: T): boolean; }
interface WeakSetConstructor { new <T extends WeakKey = WeakKey>(values?: readonly T[] | null): WeakSet<T>; new <T extends WeakKey>(values: Iterable<T>): WeakSet<T>; readonly prototype: WeakSet<WeakKey>; }
declare const WeakSet: WeakSetConstructor;
interface SymbolConstructor {
    (description?: string | number): symbol;
    readonly prototype: Symbol;
    for(key: string): symbol;
    keyFor(value: symbol): string | undefined;
    readonly iterator: unique symbol;
    readonly hasInstance: unique symbol;
    readonly isConcatSpreadable: unique symbol;
    readonly match: unique symbol;
    readonly replace: unique symbol;
    readonly search: unique symbol;
    readonly species: unique symbol;
    readonly split: unique symbol;
    readonly toPrimitive: unique symbol;
    readonly toStringTag: unique symbol;
    readonly unscopables: unique symbol;
}
declare const Symbol: SymbolConstructor;
interface ArrayBuffer { readonly [Symbol.toStringTag]: "ArrayBuffer"; }
interface Int8Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> { entries(): ArrayIterator<[number, number]>; keys(): ArrayIterator<number>; values(): ArrayIterator<number>; [Symbol.iterator](): ArrayIterator<number>; }
interface Uint8Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> { entries(): ArrayIterator<[number, number]>; keys(): ArrayIterator<number>; values(): ArrayIterator<number>; [Symbol.iterator](): ArrayIterator<number>; }
interface Uint8ClampedArray<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> { entries(): ArrayIterator<[number, number]>; keys(): ArrayIterator<number>; values(): ArrayIterator<number>; [Symbol.iterator](): ArrayIterator<number>; }
interface Int16Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> { entries(): ArrayIterator<[number, number]>; keys(): ArrayIterator<number>; values(): ArrayIterator<number>; [Symbol.iterator](): ArrayIterator<number>; }
interface Uint16Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> { entries(): ArrayIterator<[number, number]>; keys(): ArrayIterator<number>; values(): ArrayIterator<number>; [Symbol.iterator](): ArrayIterator<number>; }
interface Int32Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> { entries(): ArrayIterator<[number, number]>; keys(): ArrayIterator<number>; values(): ArrayIterator<number>; [Symbol.iterator](): ArrayIterator<number>; }
interface Uint32Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> { entries(): ArrayIterator<[number, number]>; keys(): ArrayIterator<number>; values(): ArrayIterator<number>; [Symbol.iterator](): ArrayIterator<number>; }
interface Float32Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> { entries(): ArrayIterator<[number, number]>; keys(): ArrayIterator<number>; values(): ArrayIterator<number>; [Symbol.iterator](): ArrayIterator<number>; }
interface Float64Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> { entries(): ArrayIterator<[number, number]>; keys(): ArrayIterator<number>; values(): ArrayIterator<number>; [Symbol.iterator](): ArrayIterator<number>; }
declare namespace Math {
    export function acosh(value: number): number;
    export function asinh(value: number): number;
    export function atanh(value: number): number;
    export function cbrt(value: number): number;
    export function clz32(value: number): number;
    export function cosh(value: number): number;
    export function expm1(value: number): number;
    export function fround(value: number): number;
    export function log1p(value: number): number;
    export function log2(value: number): number;
    export function log10(value: number): number;
    export function sign(value: number): number;
    export function sinh(value: number): number;
    export function tanh(value: number): number;
    export function hypot(...values: number[]): number;
    export function imul(left: number, right: number): number;
}
interface DateConstructor { new (value: number | string | Date): Date; }
declare namespace Reflect {
    export function apply<T, A extends readonly any[], R>(target: (this: T, ...args: A) => R, thisArgument: T, argumentsList: Readonly<A>): R;
    export function apply(target: Function, thisArgument: any, argumentsList: ArrayLike<any>): any;
    export function construct<A extends readonly any[], R>(target: new (...args: A) => R, argumentsList: Readonly<A>, newTarget?: new (...args: any) => any): R;
    export function construct(target: Function, argumentsList: ArrayLike<any>, newTarget?: Function): any;
    export function get<T extends object, P extends PropertyKey>(target: T, propertyKey: P, receiver?: unknown): P extends keyof T ? T[P] : any;
    export function set(target: object, propertyKey: PropertyKey, value: any, receiver?: any): boolean;
    export function deleteProperty(target: object, propertyKey: PropertyKey): boolean;
    export function defineProperty(target: object, propertyKey: PropertyKey, attributes: PropertyDescriptor): boolean;
    export function getOwnPropertyDescriptor<T extends object, P extends PropertyKey>(target: T, propertyKey: P): TypedPropertyDescriptor<P extends keyof T ? T[P] : any> | undefined;
    export function getPrototypeOf(target: object): object | null;
    export function has(target: object, propertyKey: PropertyKey): boolean;
    export function isExtensible(target: object): boolean;
    export function ownKeys(target: object): (string | symbol)[];
    export function preventExtensions(target: object): boolean;
    export function setPrototypeOf(target: object, proto: object | null): boolean;
}
declare namespace Intl { export function getCanonicalLocales(locales?: string | readonly string[]): string[]; }
