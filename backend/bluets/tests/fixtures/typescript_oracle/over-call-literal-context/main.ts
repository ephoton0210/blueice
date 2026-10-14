// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare function visit(kind: string, callback: (value: string) => string): 'wide'; declare function visit(kind: 'count', callback: (value: number) => string): 'narrow'; export const answer: 'narrow' = visit('count', value => value.toString());
