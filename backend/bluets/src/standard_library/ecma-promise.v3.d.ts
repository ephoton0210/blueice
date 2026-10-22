// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original finite ECMAScript type model for G-T7; not upstream library text.
interface PromiseLike<out T> { then<TResult1 = T, TResult2 = never>(onfulfilled?: ((value: T) => TResult1 | PromiseLike<TResult1>) | undefined | null, onrejected?: ((reason: any) => TResult2 | PromiseLike<TResult2>) | undefined | null): PromiseLike<TResult1 | TResult2>; }
type Awaited<T> = T extends null | undefined ? T : T extends object ? T extends Promise<infer U> ? Awaited<U> : T extends PromiseLike<infer U> ? Awaited<U> : T : T;
interface Promise<out T> {
    then<TResult1 = T, TResult2 = never>(onfulfilled?: ((value: T) => TResult1 | PromiseLike<TResult1>) | undefined | null, onrejected?: ((reason: any) => TResult2 | PromiseLike<TResult2>) | undefined | null): Promise<TResult1 | TResult2>;
    catch<TResult = never>(onrejected?: ((reason: any) => TResult | PromiseLike<TResult>) | undefined | null): Promise<T | TResult>;
}
interface PromiseConstructor {
    new <T>(executor: (resolve: (value: T | PromiseLike<T>) => void, reject: (reason?: any) => void) => void): Promise<T>;
    readonly prototype: Promise<any>;
    resolve(): Promise<void>;
    resolve<T>(value: T): Promise<Awaited<T>>;
    resolve<T>(value: T | PromiseLike<T>): Promise<Awaited<T>>;
    reject<T = never>(reason?: any): Promise<T>;
    all<T extends readonly unknown[] | []>(values: T): Promise<{ -readonly [P in keyof T]: Awaited<T[P]> }>;
    all<T>(values: Iterable<T | PromiseLike<T>>): Promise<Awaited<T>[]>;
    race<T extends readonly unknown[] | []>(values: T): Promise<Awaited<T[number]>>;
    race<T>(values: Iterable<T | PromiseLike<T>>): Promise<Awaited<T>>;
}
declare const Promise: PromiseConstructor;
