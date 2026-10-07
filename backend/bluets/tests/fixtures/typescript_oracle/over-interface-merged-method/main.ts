// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Visitor { read(value: unknown): 'first'; } interface Visitor { read(value: string): 'second'; } declare const visitor: Visitor; export const answer: 'second' = visitor.read('Ada');
