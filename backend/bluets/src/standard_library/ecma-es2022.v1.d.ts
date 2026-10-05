// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original additions to the minimum ES2020 surface for the ES2022 target.
interface Array<T> { at(index: number): T | undefined; }
interface ReadonlyArray<T> { at(index: number): T | undefined; }
interface String { at(index: number): string | undefined; }
declare const Object: { hasOwn(value: object, key: string | number | symbol): boolean; };
interface AggregateError { name: string; message: string; errors: any[]; stack?: string; }
type AggregateErrorConstructor = (errors: any[], message?: string) => AggregateError;
declare const AggregateError: AggregateErrorConstructor;
