// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function value(answer: 42): 42; export function value<T>(answer: T): T; export function value(answer: any): any { return answer; } export const result = [value(42)];
