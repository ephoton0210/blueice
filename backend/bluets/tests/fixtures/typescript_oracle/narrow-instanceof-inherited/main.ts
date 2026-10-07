// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Parent { value: number = 1; } class Child extends Parent { extra: string = ''; } export function read(value: Parent | Child | string): number { if (value instanceof Parent) { return value.value; } const text: string = value; return text.length; }
