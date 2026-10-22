// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original finite ECMAScript type model for G-T7; not upstream library text.
interface ArrayBuffer { readonly byteLength: number; slice(begin: number, end?: number): ArrayBuffer; }
interface ArrayBufferTypes { ArrayBuffer: ArrayBuffer; }
type ArrayBufferLike = ArrayBufferTypes[keyof ArrayBufferTypes];
interface ArrayBufferConstructor { new (byteLength: number): ArrayBuffer; readonly prototype: ArrayBuffer; isView(value: any): value is ArrayBufferView; }
declare const ArrayBuffer: ArrayBufferConstructor;
interface ArrayBufferView<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> { readonly buffer: TArrayBuffer; readonly byteLength: number; readonly byteOffset: number; }
interface DataView<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> {
    readonly buffer: TArrayBuffer;
    readonly byteLength: number;
    readonly byteOffset: number;
    getInt8(byteOffset: number): number;
    setInt8(byteOffset: number, value: number): void;
    getUint8(byteOffset: number): number;
    setUint8(byteOffset: number, value: number): void;
    getInt16(byteOffset: number , littleEndian?: boolean): number;
    setInt16(byteOffset: number, value: number , littleEndian?: boolean): void;
    getUint16(byteOffset: number , littleEndian?: boolean): number;
    setUint16(byteOffset: number, value: number , littleEndian?: boolean): void;
    getInt32(byteOffset: number , littleEndian?: boolean): number;
    setInt32(byteOffset: number, value: number , littleEndian?: boolean): void;
    getUint32(byteOffset: number , littleEndian?: boolean): number;
    setUint32(byteOffset: number, value: number , littleEndian?: boolean): void;
    getFloat32(byteOffset: number , littleEndian?: boolean): number;
    setFloat32(byteOffset: number, value: number , littleEndian?: boolean): void;
    getFloat64(byteOffset: number , littleEndian?: boolean): number;
    setFloat64(byteOffset: number, value: number , littleEndian?: boolean): void;
}
interface DataViewConstructor { new <TArrayBuffer extends ArrayBufferLike>(buffer: TArrayBuffer, byteOffset?: number, byteLength?: number): DataView<TArrayBuffer>; readonly prototype: DataView; }
declare const DataView: DataViewConstructor;
interface Int8Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> {
    [index: number]: number;
    readonly buffer: TArrayBuffer;
    readonly byteLength: number;
    readonly byteOffset: number;
    readonly length: number;
    readonly BYTES_PER_ELEMENT: number;
    copyWithin(target: number, start: number, end?: number): Int8Array<TArrayBuffer>;
    fill(value: number, start?: number, end?: number): Int8Array<TArrayBuffer>;
    filter(predicate: (value: number, index: number, array: Int8Array<TArrayBuffer>) => any, thisArg?: any): Int8Array<ArrayBuffer>;
    find(predicate: (value: number, index: number, array: Int8Array<TArrayBuffer>) => unknown, thisArg?: any): number | undefined;
    findIndex(predicate: (value: number, index: number, array: Int8Array<TArrayBuffer>) => unknown, thisArg?: any): number;
    every(predicate: (value: number, index: number, array: Int8Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    some(predicate: (value: number, index: number, array: Int8Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    forEach(callback: (value: number, index: number, array: Int8Array<TArrayBuffer>) => void, thisArg?: any): void;
    indexOf(value: number, start?: number): number;
    lastIndexOf(value: number, start?: number): number;
    join(separator?: string): string;
    map(callback: (value: number, index: number, array: Int8Array<TArrayBuffer>) => number, thisArg?: any): Int8Array<ArrayBuffer>;
    reduce(callback: (previous: number, value: number, index: number, array: Int8Array<TArrayBuffer>) => number, initialValue?: number): number;
    reduce<U>(callback: (previous: U, value: number, index: number, array: Int8Array<TArrayBuffer>) => U, initialValue: U): U;
    reverse(): Int8Array<TArrayBuffer>;
    set(array: ArrayLike<number>, offset?: number): void;
    slice(start?: number, end?: number): Int8Array<ArrayBuffer>;
    sort(compare?: (left: number, right: number) => number): Int8Array<TArrayBuffer>;
    subarray(begin?: number, end?: number): Int8Array<TArrayBuffer>;
    toString(): string;
    toLocaleString(): string;
}
interface Int8ArrayConstructor {
    new (length: number): Int8Array<ArrayBuffer>;
    new (array: ArrayLike<number>): Int8Array<ArrayBuffer>;
    new <TArrayBuffer extends ArrayBufferLike = ArrayBuffer>(buffer: TArrayBuffer, byteOffset?: number, length?: number): Int8Array<TArrayBuffer>;
    readonly prototype: Int8Array<ArrayBufferLike>;
    readonly BYTES_PER_ELEMENT: number;
    of(...items: number[]): Int8Array<ArrayBuffer>;
    from(array: ArrayLike<number>): Int8Array<ArrayBuffer>;
    from<T>(array: ArrayLike<T>, callback: (value: T, index: number) => number, thisArg?: any): Int8Array<ArrayBuffer>;
}
declare const Int8Array: Int8ArrayConstructor;
interface Uint8Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> {
    [index: number]: number;
    readonly buffer: TArrayBuffer;
    readonly byteLength: number;
    readonly byteOffset: number;
    readonly length: number;
    readonly BYTES_PER_ELEMENT: number;
    copyWithin(target: number, start: number, end?: number): Uint8Array<TArrayBuffer>;
    fill(value: number, start?: number, end?: number): Uint8Array<TArrayBuffer>;
    filter(predicate: (value: number, index: number, array: Uint8Array<TArrayBuffer>) => any, thisArg?: any): Uint8Array<ArrayBuffer>;
    find(predicate: (value: number, index: number, array: Uint8Array<TArrayBuffer>) => unknown, thisArg?: any): number | undefined;
    findIndex(predicate: (value: number, index: number, array: Uint8Array<TArrayBuffer>) => unknown, thisArg?: any): number;
    every(predicate: (value: number, index: number, array: Uint8Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    some(predicate: (value: number, index: number, array: Uint8Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    forEach(callback: (value: number, index: number, array: Uint8Array<TArrayBuffer>) => void, thisArg?: any): void;
    indexOf(value: number, start?: number): number;
    lastIndexOf(value: number, start?: number): number;
    join(separator?: string): string;
    map(callback: (value: number, index: number, array: Uint8Array<TArrayBuffer>) => number, thisArg?: any): Uint8Array<ArrayBuffer>;
    reduce(callback: (previous: number, value: number, index: number, array: Uint8Array<TArrayBuffer>) => number, initialValue?: number): number;
    reduce<U>(callback: (previous: U, value: number, index: number, array: Uint8Array<TArrayBuffer>) => U, initialValue: U): U;
    reverse(): Uint8Array<TArrayBuffer>;
    set(array: ArrayLike<number>, offset?: number): void;
    slice(start?: number, end?: number): Uint8Array<ArrayBuffer>;
    sort(compare?: (left: number, right: number) => number): Uint8Array<TArrayBuffer>;
    subarray(begin?: number, end?: number): Uint8Array<TArrayBuffer>;
    toString(): string;
    toLocaleString(): string;
}
interface Uint8ArrayConstructor {
    new (length: number): Uint8Array<ArrayBuffer>;
    new (array: ArrayLike<number>): Uint8Array<ArrayBuffer>;
    new <TArrayBuffer extends ArrayBufferLike = ArrayBuffer>(buffer: TArrayBuffer, byteOffset?: number, length?: number): Uint8Array<TArrayBuffer>;
    readonly prototype: Uint8Array<ArrayBufferLike>;
    readonly BYTES_PER_ELEMENT: number;
    of(...items: number[]): Uint8Array<ArrayBuffer>;
    from(array: ArrayLike<number>): Uint8Array<ArrayBuffer>;
    from<T>(array: ArrayLike<T>, callback: (value: T, index: number) => number, thisArg?: any): Uint8Array<ArrayBuffer>;
}
declare const Uint8Array: Uint8ArrayConstructor;
interface Uint8ClampedArray<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> {
    [index: number]: number;
    readonly buffer: TArrayBuffer;
    readonly byteLength: number;
    readonly byteOffset: number;
    readonly length: number;
    readonly BYTES_PER_ELEMENT: number;
    copyWithin(target: number, start: number, end?: number): Uint8ClampedArray<TArrayBuffer>;
    fill(value: number, start?: number, end?: number): Uint8ClampedArray<TArrayBuffer>;
    filter(predicate: (value: number, index: number, array: Uint8ClampedArray<TArrayBuffer>) => any, thisArg?: any): Uint8ClampedArray<ArrayBuffer>;
    find(predicate: (value: number, index: number, array: Uint8ClampedArray<TArrayBuffer>) => unknown, thisArg?: any): number | undefined;
    findIndex(predicate: (value: number, index: number, array: Uint8ClampedArray<TArrayBuffer>) => unknown, thisArg?: any): number;
    every(predicate: (value: number, index: number, array: Uint8ClampedArray<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    some(predicate: (value: number, index: number, array: Uint8ClampedArray<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    forEach(callback: (value: number, index: number, array: Uint8ClampedArray<TArrayBuffer>) => void, thisArg?: any): void;
    indexOf(value: number, start?: number): number;
    lastIndexOf(value: number, start?: number): number;
    join(separator?: string): string;
    map(callback: (value: number, index: number, array: Uint8ClampedArray<TArrayBuffer>) => number, thisArg?: any): Uint8ClampedArray<ArrayBuffer>;
    reduce(callback: (previous: number, value: number, index: number, array: Uint8ClampedArray<TArrayBuffer>) => number, initialValue?: number): number;
    reduce<U>(callback: (previous: U, value: number, index: number, array: Uint8ClampedArray<TArrayBuffer>) => U, initialValue: U): U;
    reverse(): Uint8ClampedArray<TArrayBuffer>;
    set(array: ArrayLike<number>, offset?: number): void;
    slice(start?: number, end?: number): Uint8ClampedArray<ArrayBuffer>;
    sort(compare?: (left: number, right: number) => number): Uint8ClampedArray<TArrayBuffer>;
    subarray(begin?: number, end?: number): Uint8ClampedArray<TArrayBuffer>;
    toString(): string;
    toLocaleString(): string;
}
interface Uint8ClampedArrayConstructor {
    new (length: number): Uint8ClampedArray<ArrayBuffer>;
    new (array: ArrayLike<number>): Uint8ClampedArray<ArrayBuffer>;
    new <TArrayBuffer extends ArrayBufferLike = ArrayBuffer>(buffer: TArrayBuffer, byteOffset?: number, length?: number): Uint8ClampedArray<TArrayBuffer>;
    readonly prototype: Uint8ClampedArray<ArrayBufferLike>;
    readonly BYTES_PER_ELEMENT: number;
    of(...items: number[]): Uint8ClampedArray<ArrayBuffer>;
    from(array: ArrayLike<number>): Uint8ClampedArray<ArrayBuffer>;
    from<T>(array: ArrayLike<T>, callback: (value: T, index: number) => number, thisArg?: any): Uint8ClampedArray<ArrayBuffer>;
}
declare const Uint8ClampedArray: Uint8ClampedArrayConstructor;
interface Int16Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> {
    [index: number]: number;
    readonly buffer: TArrayBuffer;
    readonly byteLength: number;
    readonly byteOffset: number;
    readonly length: number;
    readonly BYTES_PER_ELEMENT: number;
    copyWithin(target: number, start: number, end?: number): Int16Array<TArrayBuffer>;
    fill(value: number, start?: number, end?: number): Int16Array<TArrayBuffer>;
    filter(predicate: (value: number, index: number, array: Int16Array<TArrayBuffer>) => any, thisArg?: any): Int16Array<ArrayBuffer>;
    find(predicate: (value: number, index: number, array: Int16Array<TArrayBuffer>) => unknown, thisArg?: any): number | undefined;
    findIndex(predicate: (value: number, index: number, array: Int16Array<TArrayBuffer>) => unknown, thisArg?: any): number;
    every(predicate: (value: number, index: number, array: Int16Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    some(predicate: (value: number, index: number, array: Int16Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    forEach(callback: (value: number, index: number, array: Int16Array<TArrayBuffer>) => void, thisArg?: any): void;
    indexOf(value: number, start?: number): number;
    lastIndexOf(value: number, start?: number): number;
    join(separator?: string): string;
    map(callback: (value: number, index: number, array: Int16Array<TArrayBuffer>) => number, thisArg?: any): Int16Array<ArrayBuffer>;
    reduce(callback: (previous: number, value: number, index: number, array: Int16Array<TArrayBuffer>) => number, initialValue?: number): number;
    reduce<U>(callback: (previous: U, value: number, index: number, array: Int16Array<TArrayBuffer>) => U, initialValue: U): U;
    reverse(): Int16Array<TArrayBuffer>;
    set(array: ArrayLike<number>, offset?: number): void;
    slice(start?: number, end?: number): Int16Array<ArrayBuffer>;
    sort(compare?: (left: number, right: number) => number): Int16Array<TArrayBuffer>;
    subarray(begin?: number, end?: number): Int16Array<TArrayBuffer>;
    toString(): string;
    toLocaleString(): string;
}
interface Int16ArrayConstructor {
    new (length: number): Int16Array<ArrayBuffer>;
    new (array: ArrayLike<number>): Int16Array<ArrayBuffer>;
    new <TArrayBuffer extends ArrayBufferLike = ArrayBuffer>(buffer: TArrayBuffer, byteOffset?: number, length?: number): Int16Array<TArrayBuffer>;
    readonly prototype: Int16Array<ArrayBufferLike>;
    readonly BYTES_PER_ELEMENT: number;
    of(...items: number[]): Int16Array<ArrayBuffer>;
    from(array: ArrayLike<number>): Int16Array<ArrayBuffer>;
    from<T>(array: ArrayLike<T>, callback: (value: T, index: number) => number, thisArg?: any): Int16Array<ArrayBuffer>;
}
declare const Int16Array: Int16ArrayConstructor;
interface Uint16Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> {
    [index: number]: number;
    readonly buffer: TArrayBuffer;
    readonly byteLength: number;
    readonly byteOffset: number;
    readonly length: number;
    readonly BYTES_PER_ELEMENT: number;
    copyWithin(target: number, start: number, end?: number): Uint16Array<TArrayBuffer>;
    fill(value: number, start?: number, end?: number): Uint16Array<TArrayBuffer>;
    filter(predicate: (value: number, index: number, array: Uint16Array<TArrayBuffer>) => any, thisArg?: any): Uint16Array<ArrayBuffer>;
    find(predicate: (value: number, index: number, array: Uint16Array<TArrayBuffer>) => unknown, thisArg?: any): number | undefined;
    findIndex(predicate: (value: number, index: number, array: Uint16Array<TArrayBuffer>) => unknown, thisArg?: any): number;
    every(predicate: (value: number, index: number, array: Uint16Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    some(predicate: (value: number, index: number, array: Uint16Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    forEach(callback: (value: number, index: number, array: Uint16Array<TArrayBuffer>) => void, thisArg?: any): void;
    indexOf(value: number, start?: number): number;
    lastIndexOf(value: number, start?: number): number;
    join(separator?: string): string;
    map(callback: (value: number, index: number, array: Uint16Array<TArrayBuffer>) => number, thisArg?: any): Uint16Array<ArrayBuffer>;
    reduce(callback: (previous: number, value: number, index: number, array: Uint16Array<TArrayBuffer>) => number, initialValue?: number): number;
    reduce<U>(callback: (previous: U, value: number, index: number, array: Uint16Array<TArrayBuffer>) => U, initialValue: U): U;
    reverse(): Uint16Array<TArrayBuffer>;
    set(array: ArrayLike<number>, offset?: number): void;
    slice(start?: number, end?: number): Uint16Array<ArrayBuffer>;
    sort(compare?: (left: number, right: number) => number): Uint16Array<TArrayBuffer>;
    subarray(begin?: number, end?: number): Uint16Array<TArrayBuffer>;
    toString(): string;
    toLocaleString(): string;
}
interface Uint16ArrayConstructor {
    new (length: number): Uint16Array<ArrayBuffer>;
    new (array: ArrayLike<number>): Uint16Array<ArrayBuffer>;
    new <TArrayBuffer extends ArrayBufferLike = ArrayBuffer>(buffer: TArrayBuffer, byteOffset?: number, length?: number): Uint16Array<TArrayBuffer>;
    readonly prototype: Uint16Array<ArrayBufferLike>;
    readonly BYTES_PER_ELEMENT: number;
    of(...items: number[]): Uint16Array<ArrayBuffer>;
    from(array: ArrayLike<number>): Uint16Array<ArrayBuffer>;
    from<T>(array: ArrayLike<T>, callback: (value: T, index: number) => number, thisArg?: any): Uint16Array<ArrayBuffer>;
}
declare const Uint16Array: Uint16ArrayConstructor;
interface Int32Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> {
    [index: number]: number;
    readonly buffer: TArrayBuffer;
    readonly byteLength: number;
    readonly byteOffset: number;
    readonly length: number;
    readonly BYTES_PER_ELEMENT: number;
    copyWithin(target: number, start: number, end?: number): Int32Array<TArrayBuffer>;
    fill(value: number, start?: number, end?: number): Int32Array<TArrayBuffer>;
    filter(predicate: (value: number, index: number, array: Int32Array<TArrayBuffer>) => any, thisArg?: any): Int32Array<ArrayBuffer>;
    find(predicate: (value: number, index: number, array: Int32Array<TArrayBuffer>) => unknown, thisArg?: any): number | undefined;
    findIndex(predicate: (value: number, index: number, array: Int32Array<TArrayBuffer>) => unknown, thisArg?: any): number;
    every(predicate: (value: number, index: number, array: Int32Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    some(predicate: (value: number, index: number, array: Int32Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    forEach(callback: (value: number, index: number, array: Int32Array<TArrayBuffer>) => void, thisArg?: any): void;
    indexOf(value: number, start?: number): number;
    lastIndexOf(value: number, start?: number): number;
    join(separator?: string): string;
    map(callback: (value: number, index: number, array: Int32Array<TArrayBuffer>) => number, thisArg?: any): Int32Array<ArrayBuffer>;
    reduce(callback: (previous: number, value: number, index: number, array: Int32Array<TArrayBuffer>) => number, initialValue?: number): number;
    reduce<U>(callback: (previous: U, value: number, index: number, array: Int32Array<TArrayBuffer>) => U, initialValue: U): U;
    reverse(): Int32Array<TArrayBuffer>;
    set(array: ArrayLike<number>, offset?: number): void;
    slice(start?: number, end?: number): Int32Array<ArrayBuffer>;
    sort(compare?: (left: number, right: number) => number): Int32Array<TArrayBuffer>;
    subarray(begin?: number, end?: number): Int32Array<TArrayBuffer>;
    toString(): string;
    toLocaleString(): string;
}
interface Int32ArrayConstructor {
    new (length: number): Int32Array<ArrayBuffer>;
    new (array: ArrayLike<number>): Int32Array<ArrayBuffer>;
    new <TArrayBuffer extends ArrayBufferLike = ArrayBuffer>(buffer: TArrayBuffer, byteOffset?: number, length?: number): Int32Array<TArrayBuffer>;
    readonly prototype: Int32Array<ArrayBufferLike>;
    readonly BYTES_PER_ELEMENT: number;
    of(...items: number[]): Int32Array<ArrayBuffer>;
    from(array: ArrayLike<number>): Int32Array<ArrayBuffer>;
    from<T>(array: ArrayLike<T>, callback: (value: T, index: number) => number, thisArg?: any): Int32Array<ArrayBuffer>;
}
declare const Int32Array: Int32ArrayConstructor;
interface Uint32Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> {
    [index: number]: number;
    readonly buffer: TArrayBuffer;
    readonly byteLength: number;
    readonly byteOffset: number;
    readonly length: number;
    readonly BYTES_PER_ELEMENT: number;
    copyWithin(target: number, start: number, end?: number): Uint32Array<TArrayBuffer>;
    fill(value: number, start?: number, end?: number): Uint32Array<TArrayBuffer>;
    filter(predicate: (value: number, index: number, array: Uint32Array<TArrayBuffer>) => any, thisArg?: any): Uint32Array<ArrayBuffer>;
    find(predicate: (value: number, index: number, array: Uint32Array<TArrayBuffer>) => unknown, thisArg?: any): number | undefined;
    findIndex(predicate: (value: number, index: number, array: Uint32Array<TArrayBuffer>) => unknown, thisArg?: any): number;
    every(predicate: (value: number, index: number, array: Uint32Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    some(predicate: (value: number, index: number, array: Uint32Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    forEach(callback: (value: number, index: number, array: Uint32Array<TArrayBuffer>) => void, thisArg?: any): void;
    indexOf(value: number, start?: number): number;
    lastIndexOf(value: number, start?: number): number;
    join(separator?: string): string;
    map(callback: (value: number, index: number, array: Uint32Array<TArrayBuffer>) => number, thisArg?: any): Uint32Array<ArrayBuffer>;
    reduce(callback: (previous: number, value: number, index: number, array: Uint32Array<TArrayBuffer>) => number, initialValue?: number): number;
    reduce<U>(callback: (previous: U, value: number, index: number, array: Uint32Array<TArrayBuffer>) => U, initialValue: U): U;
    reverse(): Uint32Array<TArrayBuffer>;
    set(array: ArrayLike<number>, offset?: number): void;
    slice(start?: number, end?: number): Uint32Array<ArrayBuffer>;
    sort(compare?: (left: number, right: number) => number): Uint32Array<TArrayBuffer>;
    subarray(begin?: number, end?: number): Uint32Array<TArrayBuffer>;
    toString(): string;
    toLocaleString(): string;
}
interface Uint32ArrayConstructor {
    new (length: number): Uint32Array<ArrayBuffer>;
    new (array: ArrayLike<number>): Uint32Array<ArrayBuffer>;
    new <TArrayBuffer extends ArrayBufferLike = ArrayBuffer>(buffer: TArrayBuffer, byteOffset?: number, length?: number): Uint32Array<TArrayBuffer>;
    readonly prototype: Uint32Array<ArrayBufferLike>;
    readonly BYTES_PER_ELEMENT: number;
    of(...items: number[]): Uint32Array<ArrayBuffer>;
    from(array: ArrayLike<number>): Uint32Array<ArrayBuffer>;
    from<T>(array: ArrayLike<T>, callback: (value: T, index: number) => number, thisArg?: any): Uint32Array<ArrayBuffer>;
}
declare const Uint32Array: Uint32ArrayConstructor;
interface Float32Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> {
    [index: number]: number;
    readonly buffer: TArrayBuffer;
    readonly byteLength: number;
    readonly byteOffset: number;
    readonly length: number;
    readonly BYTES_PER_ELEMENT: number;
    copyWithin(target: number, start: number, end?: number): Float32Array<TArrayBuffer>;
    fill(value: number, start?: number, end?: number): Float32Array<TArrayBuffer>;
    filter(predicate: (value: number, index: number, array: Float32Array<TArrayBuffer>) => any, thisArg?: any): Float32Array<ArrayBuffer>;
    find(predicate: (value: number, index: number, array: Float32Array<TArrayBuffer>) => unknown, thisArg?: any): number | undefined;
    findIndex(predicate: (value: number, index: number, array: Float32Array<TArrayBuffer>) => unknown, thisArg?: any): number;
    every(predicate: (value: number, index: number, array: Float32Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    some(predicate: (value: number, index: number, array: Float32Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    forEach(callback: (value: number, index: number, array: Float32Array<TArrayBuffer>) => void, thisArg?: any): void;
    indexOf(value: number, start?: number): number;
    lastIndexOf(value: number, start?: number): number;
    join(separator?: string): string;
    map(callback: (value: number, index: number, array: Float32Array<TArrayBuffer>) => number, thisArg?: any): Float32Array<ArrayBuffer>;
    reduce(callback: (previous: number, value: number, index: number, array: Float32Array<TArrayBuffer>) => number, initialValue?: number): number;
    reduce<U>(callback: (previous: U, value: number, index: number, array: Float32Array<TArrayBuffer>) => U, initialValue: U): U;
    reverse(): Float32Array<TArrayBuffer>;
    set(array: ArrayLike<number>, offset?: number): void;
    slice(start?: number, end?: number): Float32Array<ArrayBuffer>;
    sort(compare?: (left: number, right: number) => number): Float32Array<TArrayBuffer>;
    subarray(begin?: number, end?: number): Float32Array<TArrayBuffer>;
    toString(): string;
    toLocaleString(): string;
}
interface Float32ArrayConstructor {
    new (length: number): Float32Array<ArrayBuffer>;
    new (array: ArrayLike<number>): Float32Array<ArrayBuffer>;
    new <TArrayBuffer extends ArrayBufferLike = ArrayBuffer>(buffer: TArrayBuffer, byteOffset?: number, length?: number): Float32Array<TArrayBuffer>;
    readonly prototype: Float32Array<ArrayBufferLike>;
    readonly BYTES_PER_ELEMENT: number;
    of(...items: number[]): Float32Array<ArrayBuffer>;
    from(array: ArrayLike<number>): Float32Array<ArrayBuffer>;
    from<T>(array: ArrayLike<T>, callback: (value: T, index: number) => number, thisArg?: any): Float32Array<ArrayBuffer>;
}
declare const Float32Array: Float32ArrayConstructor;
interface Float64Array<TArrayBuffer extends ArrayBufferLike = ArrayBufferLike> {
    [index: number]: number;
    readonly buffer: TArrayBuffer;
    readonly byteLength: number;
    readonly byteOffset: number;
    readonly length: number;
    readonly BYTES_PER_ELEMENT: number;
    copyWithin(target: number, start: number, end?: number): Float64Array<TArrayBuffer>;
    fill(value: number, start?: number, end?: number): Float64Array<TArrayBuffer>;
    filter(predicate: (value: number, index: number, array: Float64Array<TArrayBuffer>) => any, thisArg?: any): Float64Array<ArrayBuffer>;
    find(predicate: (value: number, index: number, array: Float64Array<TArrayBuffer>) => unknown, thisArg?: any): number | undefined;
    findIndex(predicate: (value: number, index: number, array: Float64Array<TArrayBuffer>) => unknown, thisArg?: any): number;
    every(predicate: (value: number, index: number, array: Float64Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    some(predicate: (value: number, index: number, array: Float64Array<TArrayBuffer>) => unknown, thisArg?: any): boolean;
    forEach(callback: (value: number, index: number, array: Float64Array<TArrayBuffer>) => void, thisArg?: any): void;
    indexOf(value: number, start?: number): number;
    lastIndexOf(value: number, start?: number): number;
    join(separator?: string): string;
    map(callback: (value: number, index: number, array: Float64Array<TArrayBuffer>) => number, thisArg?: any): Float64Array<ArrayBuffer>;
    reduce(callback: (previous: number, value: number, index: number, array: Float64Array<TArrayBuffer>) => number, initialValue?: number): number;
    reduce<U>(callback: (previous: U, value: number, index: number, array: Float64Array<TArrayBuffer>) => U, initialValue: U): U;
    reverse(): Float64Array<TArrayBuffer>;
    set(array: ArrayLike<number>, offset?: number): void;
    slice(start?: number, end?: number): Float64Array<ArrayBuffer>;
    sort(compare?: (left: number, right: number) => number): Float64Array<TArrayBuffer>;
    subarray(begin?: number, end?: number): Float64Array<TArrayBuffer>;
    toString(): string;
    toLocaleString(): string;
}
interface Float64ArrayConstructor {
    new (length: number): Float64Array<ArrayBuffer>;
    new (array: ArrayLike<number>): Float64Array<ArrayBuffer>;
    new <TArrayBuffer extends ArrayBufferLike = ArrayBuffer>(buffer: TArrayBuffer, byteOffset?: number, length?: number): Float64Array<TArrayBuffer>;
    readonly prototype: Float64Array<ArrayBufferLike>;
    readonly BYTES_PER_ELEMENT: number;
    of(...items: number[]): Float64Array<ArrayBuffer>;
    from(array: ArrayLike<number>): Float64Array<ArrayBuffer>;
    from<T>(array: ArrayLike<T>, callback: (value: T, index: number) => number, thisArg?: any): Float64Array<ArrayBuffer>;
}
declare const Float64Array: Float64ArrayConstructor;
