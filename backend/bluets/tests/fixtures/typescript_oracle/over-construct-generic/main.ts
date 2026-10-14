// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Box<T> { value: T; } type Factory = { new <T>(value: T): Box<T>; }; declare const factory: Factory; export const answer: Box<number> = new factory(42);
