// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Visitor { visit(kind: 'text', callback: (value: string) => number): number; visit(kind: 'count', callback: (value: number) => number): number; } export function read(visitor: Visitor): number { return visitor.visit('text', value => value.length); }
