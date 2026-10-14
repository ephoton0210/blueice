// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function* values() { yield 1; return 'done'; }
export function* receive() { const text: string = yield 1; return text; }
export function* empty() { return; }
export function* delegated() { yield* [1, 2]; }
