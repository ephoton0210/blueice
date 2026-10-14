// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export interface Shape<T> { value: T; read(): T } export abstract class Base implements Shape<number> { abstract readonly value: number; protected abstract hidden(): number; abstract read(): number; abstract get current(): number; abstract set current(value: number); } export type Ctor = abstract new (value: number) => Base;
