// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export interface Convert { readonly label: string; (value: string): string; (value: number): number; } export interface Factory { readonly label: string; new <T>(value: T): Box<T>; } export interface Box<T> { value: T; } export interface Visitor { read<T>(value: T): Box<T>; } console.log('over-interface');
