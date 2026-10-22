// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original finite ECMAScript type model for G-T7; not upstream library text.
interface SymbolConstructor { readonly asyncIterator: unique symbol; }
interface AsyncIterable<T, TReturn = any, TNext = any> { [Symbol.asyncIterator](): AsyncIterator<T, TReturn, TNext>; }
interface AsyncIterableIterator<T, R = any, N = any> { [Symbol.asyncIterator](): AsyncIterableIterator<T, R, N>; }
interface AsyncGenerator<T = unknown, R = any, N = any> { [Symbol.asyncIterator](): AsyncGenerator<T, R, N>; }
interface Promise<T> { finally(onfinally?: (() => void) | undefined | null): Promise<T>; }
interface RegExpExecArray { groups?: {[key: string]: string}; }
interface RegExpMatchArray { groups?: {[key: string]: string}; }
interface RegExp { readonly dotAll: boolean; }
declare namespace Intl {
    export interface PluralRulesOptions { localeMatcher?: "lookup" | "best fit"; type?: "cardinal" | "ordinal"; minimumIntegerDigits?: number; minimumFractionDigits?: number; maximumFractionDigits?: number; }
    export type LDMLPluralRule = "zero" | "one" | "two" | "few" | "many" | "other";
    export interface PluralRules { select(value: number): Intl.LDMLPluralRule; resolvedOptions(): Intl.ResolvedPluralRulesOptions; }
    export interface ResolvedPluralRulesOptions { locale: string; pluralCategories: Intl.LDMLPluralRule[]; type: "cardinal" | "ordinal"; minimumIntegerDigits: number; minimumFractionDigits: number; maximumFractionDigits: number; }
    export interface PluralRulesConstructor { new (locales?: string | readonly string[], options?: Intl.PluralRulesOptions): Intl.PluralRules; (locales?: string | readonly string[], options?: Intl.PluralRulesOptions): Intl.PluralRules; supportedLocalesOf(locales: string | readonly string[], options?: Intl.PluralRulesOptions): string[]; }
    export const PluralRules: Intl.PluralRulesConstructor;
}
