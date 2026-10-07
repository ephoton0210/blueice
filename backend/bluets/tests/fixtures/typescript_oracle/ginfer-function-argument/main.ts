// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function apply<T, U>(value: T, transform: (value: T) => U): U { return transform(value); } function increment(value: number): number { return value + 1; } export const answer: number = apply(41, increment);
