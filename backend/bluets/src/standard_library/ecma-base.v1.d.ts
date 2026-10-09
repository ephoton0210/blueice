// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original declarations for the selected ECMAScript surface, not upstream lib text.
interface Array<T> {
    length: number;
    push(item: T): number;
    pop(): T | undefined;
    shift(): T | undefined;
    unshift(item: T): number;
    includes(item: T, start?: number): boolean;
    indexOf(item: T, start?: number): number;
    lastIndexOf(item: T, start?: number): number;
    join(separator?: string): string;
    slice(start?: number, end?: number): T[];
    reverse(): T[];
    sort(compare?: (left: T, right: T) => number): T[];
    map(callback: (value: T, index: number, items: T[]) => any): any[];
    filter(callback: (value: T, index: number, items: T[]) => unknown): T[];
    forEach(callback: (value: T, index: number, items: T[]) => void): void;
}
interface ReadonlyArray<T> {
    readonly length: number;
    includes(item: T, start?: number): boolean;
    indexOf(item: T, start?: number): number;
    join(separator?: string): string;
    slice(start?: number, end?: number): T[];
}
interface String {
    readonly length: number;
    toString(): string;
    valueOf(): string;
    toUpperCase(): string;
    toLowerCase(): string;
    trim(): string;
    trimStart(): string;
    trimEnd(): string;
    charAt(index: number): string;
    charCodeAt(index: number): number;
    includes(search: string, start?: number): boolean;
    startsWith(search: string, start?: number): boolean;
    endsWith(search: string, end?: number): boolean;
    indexOf(search: string, start?: number): number;
    lastIndexOf(search: string, start?: number): number;
    repeat(count: number): string;
    padStart(length: number, fill?: string): string;
    padEnd(length: number, fill?: string): string;
    slice(start?: number, end?: number): string;
    substring(start: number, end?: number): string;
    split(separator: string | RegExp, limit?: number): string[];
}
interface Number { toString(radix?: number): string; valueOf(): number; toFixed(digits?: number): string; toPrecision(digits?: number): string; toExponential(digits?: number): string; }
interface Boolean { valueOf(): boolean; toString(): string; }
interface Object { toString(): string; valueOf(): Object; hasOwnProperty(key: string | number | symbol): boolean; }
interface Function { readonly length: number; readonly name: string; toString(): string; }
interface Symbol { toString(): string; valueOf(): symbol; }
interface Promise<T> { then(onfulfilled?: (value: T) => any, onrejected?: (reason: any) => any): Promise<any>; catch(onrejected?: (reason: any) => any): Promise<any>; finally(onfinally?: () => void): Promise<T>; }
declare namespace Promise { export function resolve<T>(value: T): Promise<T>; export function reject(reason?: any): Promise<never>; }
interface Map<K, V> { readonly size: number; get(key: K): V | undefined; set(key: K, value: V): Map<K, V>; has(key: K): boolean; delete(key: K): boolean; clear(): void; }
interface Set<T> { readonly size: number; add(value: T): Set<T>; has(value: T): boolean; delete(value: T): boolean; clear(): void; }
interface WeakMap<K extends object, V> { get(key: K): V | undefined; set(key: K, value: V): WeakMap<K, V>; has(key: K): boolean; delete(key: K): boolean; }
interface IteratorYieldResult<T> { done?: false; value: T; }
interface IteratorReturnResult<R> { done: true; value: R; }
type IteratorResult<T, R = any> = IteratorYieldResult<T> | IteratorReturnResult<R>;
interface Iterator<T, R = any, N = any> { next(value?: N): IteratorResult<T, R>; return(value?: R): IteratorResult<T, R>; throw(error?: any): IteratorResult<T, R>; }
interface Generator<T = unknown, R = any, N = any> { next(value?: N): IteratorResult<T, R>; return(value: R): IteratorResult<T, R>; throw(error: any): IteratorResult<T, R>; }
interface IterableIterator<T, R = any, N = any> { next(value?: N): IteratorResult<T, R>; return(value?: R): IteratorResult<T, R>; throw(error?: any): IteratorResult<T, R>; }
interface Iterable<T> { }
declare namespace Math { export const PI: number; export const E: number; export function sqrt(value: number): number; export function abs(value: number): number; export function floor(value: number): number; export function ceil(value: number): number; export function round(value: number): number; export function trunc(value: number): number; export function pow(base: number, exponent: number): number; export function min(...values: number[]): number; export function max(...values: number[]): number; export function random(): number; }
declare const JSON: { parse(text: string): any; stringify(value: any, replacer?: ((key: string, item: any) => any) | (string | number)[] | null, space?: string | number): string; };
declare const Object: { keys(value: object | string): string[]; getOwnPropertyNames(value: any): string[]; };
interface PropertyDescriptor { value?: any; writable?: boolean; configurable?: boolean; enumerable?: boolean; get?: () => any; set?: (value: any) => void; }
declare namespace Object { export function assign<T extends object, U>(target: T, source: U): T & U; export function getPrototypeOf(value: any): any; export function setPrototypeOf(value: any, prototype: object | null): any; export function defineProperty<T>(value: T, key: string | number | symbol, descriptor: PropertyDescriptor): T; }
declare const Symbol: { for(key: string): symbol; keyFor(value: symbol): string | undefined; };
declare function Symbol(description?: string | number): symbol;
interface Error { name: string; message: string; stack?: string;}
type ErrorConstructor = (message?: string) => Error;
declare const Error: ErrorConstructor;
interface EvalError { name: string; message: string; stack?: string;}
type EvalErrorConstructor = (message?: string) => EvalError;
declare const EvalError: EvalErrorConstructor;
interface RangeError { name: string; message: string; stack?: string;}
type RangeErrorConstructor = (message?: string) => RangeError;
declare const RangeError: RangeErrorConstructor;
interface ReferenceError { name: string; message: string; stack?: string;}
type ReferenceErrorConstructor = (message?: string) => ReferenceError;
declare const ReferenceError: ReferenceErrorConstructor;
interface SyntaxError { name: string; message: string; stack?: string;}
type SyntaxErrorConstructor = (message?: string) => SyntaxError;
declare const SyntaxError: SyntaxErrorConstructor;
interface TypeError { name: string; message: string; stack?: string;}
type TypeErrorConstructor = (message?: string) => TypeError;
declare const TypeError: TypeErrorConstructor;
interface URIError { name: string; message: string; stack?: string;}
type URIErrorConstructor = (message?: string) => URIError;
declare const URIError: URIErrorConstructor;
interface Date { getTime(): number; getFullYear(): number; getMonth(): number; getDate(): number; getDay(): number; getHours(): number; getMinutes(): number; getSeconds(): number; toISOString(): string; toString(): string; valueOf(): number;}
type DateConstructor = (value?: number | string | Date) => Date;
declare const Date: DateConstructor & { now(): number; parse(value: string): number; UTC(year: number, month: number, day?: number): number; };
interface RegExp { readonly source: string; readonly flags: string; readonly global: boolean; readonly ignoreCase: boolean; readonly multiline: boolean; lastIndex: number; test(text: string): boolean; toString(): string;}
type RegExpConstructor = (pattern: string | RegExp, flags?: string) => RegExp;
declare const RegExp: RegExpConstructor;
declare function parseInt(text: string, radix?: number): number;
declare function parseFloat(text: string): number;
declare function isNaN(value: number): boolean;
declare function isFinite(value: number): boolean;
declare function decodeURI(value: string): string;
declare function decodeURIComponent(value: string): string;
declare function encodeURI(value: string): string;
declare function encodeURIComponent(value: string): string;
declare function eval(value: string): any;
declare const NaN: number;
declare const Infinity: number;
// Existing unmodeled ECMAScript names retain opaque typing; see the omission inventory.
declare const globalThis: any;
declare const Reflect: any;
declare const Intl: any;
declare const Array: any;
declare const Function: any;
declare const Boolean: any;
declare const Number: any;
declare const String: any;
declare const Map: any;
declare const Set: any;
declare const WeakMap: any;
declare const WeakSet: any;
declare const BigInt: any;
declare const ArrayBuffer: any;
declare const SharedArrayBuffer: any;
declare const DataView: any;
declare const Int8Array: any;
declare const Uint8Array: any;
declare const Uint8ClampedArray: any;
declare const Int16Array: any;
declare const Uint16Array: any;
declare const Int32Array: any;
declare const Uint32Array: any;
declare const Float32Array: any;
declare const Float64Array: any;
declare const BigInt64Array: any;
declare const BigUint64Array: any;
declare const Atomics: any;

interface BigInt {
    toString(radix?: number): string;
    valueOf(): bigint;
    toLocaleString(): string;
}
