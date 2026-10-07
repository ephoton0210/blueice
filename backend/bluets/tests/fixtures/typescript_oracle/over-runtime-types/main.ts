// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export interface Box { value: string | number; } export type Convert = { readonly label: string; (value: string): string; (value: number): number; }; export type Factory = { readonly label: string; new (value: string): Box; new (value: number): Box; }; export interface Visitor { visit(kind: 'text', callback: (value: string) => number): number; visit(kind: 'count', callback: (value: number) => number): number; } console.log('overload-types');
