// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original finite ECMAScript type model for G-T7; not upstream library text.
interface PromiseConstructor { any<T extends readonly unknown[] | []>(values: T): Promise<Awaited<T[number]>>; any<T>(values: Iterable<T | PromiseLike<T>>): Promise<Awaited<T>>; }
interface AggregateErrorConstructor { new (errors: Iterable<any>, message?: string): AggregateError; (errors: Iterable<any>, message?: string): AggregateError; readonly prototype: AggregateError; }
declare const AggregateError: AggregateErrorConstructor;
declare namespace Intl {
    export interface ListFormatOptions { localeMatcher?: "lookup" | "best fit"; type?: "conjunction" | "disjunction" | "unit"; style?: "long" | "short" | "narrow"; }
    export interface ListFormat { format(list: Iterable<string>): string; resolvedOptions(): Intl.ResolvedListFormatOptions; }
    export interface ResolvedListFormatOptions { locale: string; type: "conjunction" | "disjunction" | "unit"; style: "long" | "short" | "narrow"; }
    export interface ListFormatConstructor { new (locales?: string | readonly string[], options?: Intl.ListFormatOptions): Intl.ListFormat; supportedLocalesOf(locales: string | readonly string[], options?: Intl.ListFormatOptions): string[]; }
    export const ListFormat: Intl.ListFormatConstructor;
}
