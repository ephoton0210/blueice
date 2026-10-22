// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original finite ECMAScript type model for G-T7; not upstream library text.
type Readonly<T> = { readonly [P in keyof T]: T[P] };
type Required<T> = { [P in keyof T]-?: T[P] };
type Record<K extends string | number | symbol, T> = { [P in K]: T };
type PropertyKey = string | number | symbol;
interface ArrayLike<out T> { readonly length: number; readonly [index: number]: T; }
interface Array<T> {
    [index: number]: T;
    push(...items: T[]): number;
    unshift(...items: T[]): number;
    map<U>(callback: (value: T, index: number, array: T[]) => U, thisArg?: any): U[];
    filter<S extends T>(predicate: (value: T, index: number, array: T[]) => value is S, thisArg?: any): S[];
    filter(predicate: (value: T, index: number, array: T[]) => unknown, thisArg?: any): T[];
    every(predicate: (value: T, index: number, items: T[]) => unknown, thisArg?: any): boolean;
    some(predicate: (value: T, index: number, items: T[]) => unknown, thisArg?: any): boolean;
    concat(...items: (T | ConcatArray<T>)[]): T[];
    splice(start: number, deleteCount?: number): T[];
    splice(start: number, deleteCount: number, ...items: T[]): T[];
}
interface ConcatArray<out T> { readonly length: number; readonly [index: number]: T; join(separator?: string): string; slice(start?: number, end?: number): T[]; }
interface ReadonlyArray<T> {
    readonly [index: number]: T;
    lastIndexOf(value: T, start?: number): number;
    map<U>(callback: (value: T, index: number, array: readonly T[]) => U, thisArg?: any): U[];
    filter<S extends T>(predicate: (value: T, index: number, array: readonly T[]) => value is S, thisArg?: any): S[];
    filter(predicate: (value: T, index: number, array: readonly T[]) => unknown, thisArg?: any): T[];
    every(predicate: (value: T, index: number, items: readonly T[]) => unknown, thisArg?: any): boolean;
    some(predicate: (value: T, index: number, items: readonly T[]) => unknown, thisArg?: any): boolean;
    forEach(callback: (value: T, index: number, items: readonly T[]) => void, thisArg?: any): void;
    concat(...items: (T | ConcatArray<T>)[]): T[];
    reduce(callback: (previousValue: T, currentValue: T, currentIndex: number, array: readonly T[]) => T): T;
    reduce(callback: (previousValue: T, currentValue: T, currentIndex: number, array: readonly T[]) => T, initialValue: T): T;
    reduce<U>(callback: (previousValue: U, currentValue: T, currentIndex: number, array: readonly T[]) => U, initialValue: U): U;
}
interface ArrayConstructor {
    new (length?: number): any[];
    new <T>(length: number): T[];
    new <T>(...items: T[]): T[];
    (length?: number): any[];
    <T>(...items: T[]): T[];
    readonly prototype: any[];
    isArray(value: any): value is any[];
}
declare const Array: ArrayConstructor;
interface Function {
    apply(this: Function, thisArg: any, argArray?: any): any;
    call(this: Function, thisArg: any, ...args: any[]): any;
    bind(this: Function, thisArg: any, ...args: any[]): any;
}
interface FunctionConstructor { new (...args: string[]): Function; (...args: string[]): Function; readonly prototype: Function; }
declare const Function: FunctionConstructor;
interface BooleanConstructor { new (value?: any): Boolean; (value?: any): boolean; readonly prototype: Boolean; }
declare const Boolean: BooleanConstructor;
interface NumberConstructor {
    new (value?: any): Number;
    (value?: any): number;
    readonly prototype: Number;
    readonly MAX_VALUE: number;
    readonly MIN_VALUE: number;
    readonly NaN: number;
    readonly POSITIVE_INFINITY: number;
    readonly NEGATIVE_INFINITY: number;
}
declare const Number: NumberConstructor;
interface StringConstructor { new (value?: any): String; (value?: any): string; fromCharCode(...codes: number[]): string; readonly prototype: String; }
declare const String: StringConstructor;
interface String {
    readonly [index: number]: string;
    localeCompare(other: string, locales?: string | string[], options?: Intl.CollatorOptions): number;
    replace(search: string | RegExp, replacement: string): string;
    replace(search: string | RegExp, replacement: (substring: string, ...args: any[]) => string): string;
    match(expression: string | RegExp): RegExpMatchArray | null;
}
interface Number { toLocaleString(locales?: string | string[], options?: Intl.NumberFormatOptions): string; }
interface ObjectConstructor {
    (value?: any): any;
    new (value?: any): Object;
    readonly prototype: Object;
    keys(value: object | string): string[];
    getOwnPropertyNames(value: any): string[];
    getPrototypeOf(value: any): any;
    create(prototype: object | null): any;
    create(prototype: object | null, properties: PropertyDescriptorMap & ThisType<any>): any;
    defineProperty<T>(value: T, key: PropertyKey, descriptor: PropertyDescriptor): T;
    defineProperties<T>(value: T, properties: PropertyDescriptorMap & ThisType<any>): T;
    getOwnPropertyDescriptor(value: any, key: PropertyKey): PropertyDescriptor | undefined;
    seal<T>(value: T): T;
    freeze<T extends Function>(value: T): T;
    freeze<T extends {[key: string]: U | null | undefined | object}, U extends string | bigint | number | boolean | symbol>(value: T): Readonly<T>;
    freeze<T>(value: T): Readonly<T>;
    preventExtensions<T>(value: T): T;
    isSealed(value: any): boolean;
    isFrozen(value: any): boolean;
    isExtensible(value: any): boolean;
}
interface ThisType<T> {}
interface PropertyDescriptorMap { [key: string]: PropertyDescriptor; }
declare const Object: ObjectConstructor;
interface JSON {
    parse(text: string, reviver?: (this: any, key: string, value: any) => any): any;
    stringify(value: any, replacer?: ((this: any, key: string, value: any) => any) | (string | number)[] | null, space?: string | number): string;
}
declare const JSON: JSON;
declare namespace Math {
    export const LN2: number;
    export const LN10: number;
    export const LOG2E: number;
    export const LOG10E: number;
    export const SQRT1_2: number;
    export const SQRT2: number;
    export function acos(value: number): number;
    export function asin(value: number): number;
    export function atan(value: number): number;
    export function cos(value: number): number;
    export function exp(value: number): number;
    export function log(value: number): number;
    export function sin(value: number): number;
    export function tan(value: number): number;
    export function atan2(y: number, x: number): number;
}
interface ErrorConstructor { new (message?: string): Error; (message?: string): Error; readonly prototype: Error; }
declare const Error: ErrorConstructor;
interface EvalErrorConstructor { new (message?: string): EvalError; (message?: string): EvalError; readonly prototype: EvalError; }
declare const EvalError: EvalErrorConstructor;
interface RangeErrorConstructor { new (message?: string): RangeError; (message?: string): RangeError; readonly prototype: RangeError; }
declare const RangeError: RangeErrorConstructor;
interface ReferenceErrorConstructor { new (message?: string): ReferenceError; (message?: string): ReferenceError; readonly prototype: ReferenceError; }
declare const ReferenceError: ReferenceErrorConstructor;
interface SyntaxErrorConstructor { new (message?: string): SyntaxError; (message?: string): SyntaxError; readonly prototype: SyntaxError; }
declare const SyntaxError: SyntaxErrorConstructor;
interface TypeErrorConstructor { new (message?: string): TypeError; (message?: string): TypeError; readonly prototype: TypeError; }
declare const TypeError: TypeErrorConstructor;
interface URIErrorConstructor { new (message?: string): URIError; (message?: string): URIError; readonly prototype: URIError; }
declare const URIError: URIErrorConstructor;
interface DateConstructor {
    new (): Date;
    new (value: number | string): Date;
    new (year: number, month: number, date?: number, hours?: number, minutes?: number, seconds?: number, ms?: number): Date;
    (): string;
    readonly prototype: Date;
    now(): number;
    parse(value: string): number;
    UTC(year: number, month: number, date?: number, hours?: number, minutes?: number, seconds?: number, ms?: number): number;
}
declare const Date: DateConstructor;
interface Date {
    getUTCFullYear(): number;
    getUTCMonth(): number;
    getUTCDate(): number;
    getUTCDay(): number;
    getUTCHours(): number;
    getUTCMinutes(): number;
    getUTCSeconds(): number;
    getMilliseconds(): number;
    getUTCMilliseconds(): number;
    getTimezoneOffset(): number;
    setTime(time: number): number;
    setMilliseconds(ms: number): number;
    setUTCMilliseconds(ms: number): number;
    setSeconds(seconds: number, ms?: number): number;
    setUTCSeconds(seconds: number, ms?: number): number;
    setMinutes(minutes: number, seconds?: number, ms?: number): number;
    setUTCMinutes(minutes: number, seconds?: number, ms?: number): number;
    setHours(hours: number, minutes?: number, seconds?: number, ms?: number): number;
    setUTCHours(hours: number, minutes?: number, seconds?: number, ms?: number): number;
    setDate(date: number): number;
    setUTCDate(date: number): number;
    setMonth(month: number, date?: number): number;
    setUTCMonth(month: number, date?: number): number;
    setFullYear(year: number, month?: number, date?: number): number;
    setUTCFullYear(year: number, month?: number, date?: number): number;
    toDateString(): string;
    toTimeString(): string;
    toUTCString(): string;
    toJSON(key?: any): string;
    toLocaleString(locales?: string | string[], options?: Intl.DateTimeFormatOptions): string;
    toLocaleDateString(locales?: string | string[], options?: Intl.DateTimeFormatOptions): string;
    toLocaleTimeString(locales?: string | string[], options?: Intl.DateTimeFormatOptions): string;
}
interface RegExpConstructor { new (pattern: string | RegExp, flags?: string): RegExp; (pattern: string | RegExp, flags?: string): RegExp; readonly prototype: RegExp; }
declare const RegExp: RegExpConstructor;
interface RegExpExecArray extends Array<string> { index: number; input: string; }
interface RegExpMatchArray extends Array<string> { index?: number; input?: string; }
interface RegExp { exec(text: string): RegExpExecArray | null; }
declare namespace Intl {
    export interface CollatorOptions { usage?: "sort" | "search"; localeMatcher?: "lookup" | "best fit"; numeric?: boolean; caseFirst?: "upper" | "lower" | "false"; sensitivity?: "base" | "accent" | "case" | "variant"; ignorePunctuation?: boolean; }
    export interface Collator { compare(left: string, right: string): number; resolvedOptions(): Intl.ResolvedCollatorOptions; }
    export interface ResolvedCollatorOptions { locale: string; usage: string; sensitivity: string; ignorePunctuation: boolean; collation: string; caseFirst: string; numeric: boolean; }
    export interface CollatorConstructor { new (locales?: string | string[], options?: Intl.CollatorOptions): Intl.Collator; (locales?: string | string[], options?: Intl.CollatorOptions): Intl.Collator; supportedLocalesOf(locales: string | string[], options?: Intl.CollatorOptions): string[]; }
    export const Collator: Intl.CollatorConstructor;
    export interface NumberFormatOptions { localeMatcher?: "lookup" | "best fit"; style?: "decimal" | "percent" | "currency"; currency?: string; useGrouping?: boolean; minimumIntegerDigits?: number; minimumFractionDigits?: number; maximumFractionDigits?: number; minimumSignificantDigits?: number; maximumSignificantDigits?: number; }
    export interface NumberFormat { format(value: number): string; resolvedOptions(): Intl.ResolvedNumberFormatOptions; }
    export interface ResolvedNumberFormatOptions { locale: string; numberingSystem: string; style: string; minimumIntegerDigits: number; minimumFractionDigits?: number; maximumFractionDigits?: number; useGrouping: boolean; }
    export interface NumberFormatConstructor { new (locales?: string | string[], options?: Intl.NumberFormatOptions): Intl.NumberFormat; (locales?: string | string[], options?: Intl.NumberFormatOptions): Intl.NumberFormat; supportedLocalesOf(locales: string | string[], options?: Intl.NumberFormatOptions): string[]; }
    export const NumberFormat: Intl.NumberFormatConstructor;
    export interface DateTimeFormatOptions { localeMatcher?: "lookup" | "best fit"; weekday?: "long" | "short" | "narrow"; era?: "long" | "short" | "narrow"; year?: "numeric" | "2-digit"; month?: "numeric" | "2-digit" | "long" | "short" | "narrow"; day?: "numeric" | "2-digit"; hour?: "numeric" | "2-digit"; minute?: "numeric" | "2-digit"; second?: "numeric" | "2-digit"; timeZoneName?: "short" | "long"; hour12?: boolean; timeZone?: string; }
    export interface DateTimeFormat { format(value?: number | Date): string; resolvedOptions(): Intl.ResolvedDateTimeFormatOptions; }
    export interface ResolvedDateTimeFormatOptions { locale: string; calendar: string; numberingSystem: string; timeZone: string; }
    export interface DateTimeFormatConstructor { new (locales?: string | string[], options?: Intl.DateTimeFormatOptions): Intl.DateTimeFormat; (locales?: string | string[], options?: Intl.DateTimeFormatOptions): Intl.DateTimeFormat; supportedLocalesOf(locales: string | string[], options?: Intl.DateTimeFormatOptions): string[]; }
    export const DateTimeFormat: Intl.DateTimeFormatConstructor;
}
