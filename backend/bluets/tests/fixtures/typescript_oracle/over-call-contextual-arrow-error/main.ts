// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare function visit(kind: 'text', callback: (value: string) => number): number; declare function visit(kind: 'count', callback: (value: number) => number): number; export const answer = visit('text', value => value + 1);
