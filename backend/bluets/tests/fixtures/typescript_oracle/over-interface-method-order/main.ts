// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Reader { read(value: string): 'first'; read(value: string): 'second'; } declare const reader: Reader; export const answer: 'first' = reader.read('Ada');
