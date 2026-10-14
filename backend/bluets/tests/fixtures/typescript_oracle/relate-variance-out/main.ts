// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Producer<out T> {get: () => T} declare const source: Producer<string>; export const value: Producer<unknown> = source;
