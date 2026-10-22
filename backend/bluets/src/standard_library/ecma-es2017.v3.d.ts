// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original finite ECMAScript type model for G-T7; not upstream library text.
interface ObjectConstructor { values<T>(value: {[key: string]: T} | ArrayLike<T>): T[]; values(value: {}): any[]; entries<T>(value: {[key: string]: T} | ArrayLike<T>): [string, T][]; entries(value: {}): [string, any][]; getOwnPropertyDescriptors<T>(value: T): { [P in keyof T]: TypedPropertyDescriptor<T[P]> } & {[key: string]: PropertyDescriptor}; }
interface TypedPropertyDescriptor<T> { configurable?: boolean; enumerable?: boolean; value?: T; writable?: boolean; get?: () => T; set?: (value: T) => void; }
interface SharedArrayBuffer { readonly byteLength: number; slice(begin: number, end?: number): SharedArrayBuffer; readonly [Symbol.toStringTag]: "SharedArrayBuffer"; }
interface SharedArrayBufferConstructor { new (byteLength: number): SharedArrayBuffer; readonly prototype: SharedArrayBuffer; }
declare const SharedArrayBuffer: SharedArrayBufferConstructor;
interface ArrayBufferTypes { SharedArrayBuffer: SharedArrayBuffer; }
declare namespace Atomics {
    export function add(typedArray: Int8Array | Uint8Array | Int16Array | Uint16Array | Int32Array | Uint32Array, index: number, value: number): number;
    export function and(typedArray: Int8Array | Uint8Array | Int16Array | Uint16Array | Int32Array | Uint32Array, index: number, value: number): number;
    export function exchange(typedArray: Int8Array | Uint8Array | Int16Array | Uint16Array | Int32Array | Uint32Array, index: number, value: number): number;
    export function or(typedArray: Int8Array | Uint8Array | Int16Array | Uint16Array | Int32Array | Uint32Array, index: number, value: number): number;
    export function sub(typedArray: Int8Array | Uint8Array | Int16Array | Uint16Array | Int32Array | Uint32Array, index: number, value: number): number;
    export function xor(typedArray: Int8Array | Uint8Array | Int16Array | Uint16Array | Int32Array | Uint32Array, index: number, value: number): number;
    export function compareExchange(typedArray: Int8Array | Uint8Array | Int16Array | Uint16Array | Int32Array | Uint32Array, index: number, expectedValue: number, replacementValue: number): number;
    export function load(typedArray: Int8Array | Uint8Array | Int16Array | Uint16Array | Int32Array | Uint32Array, index: number): number;
    export function store(typedArray: Int8Array | Uint8Array | Int16Array | Uint16Array | Int32Array | Uint32Array, index: number, value: number): number;
    export function isLockFree(size: number): boolean;
    export function wait(typedArray: Int32Array, index: number, value: number, timeout?: number): "ok" | "not-equal" | "timed-out";
    export function notify(typedArray: Int32Array, index: number, count?: number): number;
}
interface Int8ArrayConstructor { new (): Int8Array<ArrayBuffer>; }
interface Uint8ArrayConstructor { new (): Uint8Array<ArrayBuffer>; }
interface Uint8ClampedArrayConstructor { new (): Uint8ClampedArray<ArrayBuffer>; }
interface Int16ArrayConstructor { new (): Int16Array<ArrayBuffer>; }
interface Uint16ArrayConstructor { new (): Uint16Array<ArrayBuffer>; }
interface Int32ArrayConstructor { new (): Int32Array<ArrayBuffer>; }
interface Uint32ArrayConstructor { new (): Uint32Array<ArrayBuffer>; }
interface Float32ArrayConstructor { new (): Float32Array<ArrayBuffer>; }
interface Float64ArrayConstructor { new (): Float64Array<ArrayBuffer>; }
