// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function value<T>(answer: T): T { return answer; } export function run(value: (answer: 42) => 42) { return [value(42)]; } export const result = run((answer: 42): 42 => answer);
