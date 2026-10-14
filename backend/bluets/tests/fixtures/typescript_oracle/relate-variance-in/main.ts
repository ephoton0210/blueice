// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Consumer<in T> {set: (value: T) => void} declare const source: Consumer<unknown>; export const value: Consumer<string> = source;
