// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original versioned declarations for the owned ECMAScript surface.
interface String { includes(search: string, start?: number): boolean;
    startsWith(search: string, start?: number): boolean;
    endsWith(search: string, end?: number): boolean;
    repeat(count: number): string; }

interface Function { readonly name: string; }

interface Promise<T> { then(onfulfilled?: (value: T) => any, onrejected?: (reason: any) => any): Promise<any>; catch(onrejected?: (reason: any) => any): Promise<any>; }

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

declare namespace Math { export function trunc(value: number): number; }

declare namespace Object { export function assign<T extends object, U>(target: T, source: U): T & U; export function setPrototypeOf(value: any, prototype: object | null): any; }

declare const Symbol: { for(key: string): symbol; keyFor(value: symbol): string | undefined; };

declare namespace Symbol { export const iterator: symbol; }

declare function Symbol(description?: string | number): symbol;

interface RegExp { readonly flags: string; }

declare const Reflect: any;

declare const Intl: any;

declare const Map: any;

declare const Set: any;

declare const WeakMap: any;

declare const WeakSet: any;

declare const ArrayBuffer: any;

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
