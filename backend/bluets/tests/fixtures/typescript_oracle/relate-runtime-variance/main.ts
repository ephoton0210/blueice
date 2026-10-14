// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export interface Producer<out T> {get(): T;}
export interface Consumer<in T> {set(value: T): void;}
export interface Cell<in out T> {value: T;}
export type Source<out T> = {readonly value: T};
export function value(): number {return 42;}
export const source: Producer<number> = {get: value};
console.log(source.get());
