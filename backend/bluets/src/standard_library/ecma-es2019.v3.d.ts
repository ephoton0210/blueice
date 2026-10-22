// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original finite ECMAScript type model for G-T7; not upstream library text.
interface Array<T> { flatMap<U>(callback: (value: T, index: number, array: T[]) => U | readonly U[], thisArg?: any): U[]; }
interface ReadonlyArray<T> { flatMap<U>(callback: (value: T, index: number, array: readonly T[]) => U | readonly U[], thisArg?: any): U[]; }
interface ObjectConstructor { fromEntries<T = any>(entries: Iterable<readonly [PropertyKey, T]>): {[k: string]: T}; fromEntries(entries: Iterable<readonly any[]>): any; }
interface Symbol { readonly description: string | undefined; }
