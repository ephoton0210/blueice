// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original finite ECMAScript type model for G-T7; not upstream library text.
interface ObjectConstructor { hasOwn(value: object, key: PropertyKey): boolean; }
interface ErrorOptions { cause?: unknown; }
interface Error { cause?: unknown; }
interface ErrorConstructor { new (message?: string, options?: ErrorOptions): Error; (message?: string, options?: ErrorOptions): Error; }
interface EvalError { cause?: unknown; }
interface EvalErrorConstructor { new (message?: string, options?: ErrorOptions): EvalError; (message?: string, options?: ErrorOptions): EvalError; }
interface RangeError { cause?: unknown; }
interface RangeErrorConstructor { new (message?: string, options?: ErrorOptions): RangeError; (message?: string, options?: ErrorOptions): RangeError; }
interface ReferenceError { cause?: unknown; }
interface ReferenceErrorConstructor { new (message?: string, options?: ErrorOptions): ReferenceError; (message?: string, options?: ErrorOptions): ReferenceError; }
interface SyntaxError { cause?: unknown; }
interface SyntaxErrorConstructor { new (message?: string, options?: ErrorOptions): SyntaxError; (message?: string, options?: ErrorOptions): SyntaxError; }
interface TypeError { cause?: unknown; }
interface TypeErrorConstructor { new (message?: string, options?: ErrorOptions): TypeError; (message?: string, options?: ErrorOptions): TypeError; }
interface URIError { cause?: unknown; }
interface URIErrorConstructor { new (message?: string, options?: ErrorOptions): URIError; (message?: string, options?: ErrorOptions): URIError; }
interface AggregateError { cause?: unknown; }
interface AggregateErrorConstructor { new (errors: Iterable<any>, message?: string, options?: ErrorOptions): AggregateError; (errors: Iterable<any>, message?: string, options?: ErrorOptions): AggregateError; }
interface RegExpIndicesArray extends Array<[number, number]> { groups?: {[key: string]: [number, number]}; }
interface RegExpExecArray { indices?: RegExpIndicesArray; }
interface RegExpMatchArray { indices?: RegExpIndicesArray; }
interface RegExp { readonly hasIndices: boolean; }
