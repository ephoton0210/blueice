// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function value(answer: number) { return answer; } export namespace value { export const answer = 42; } export const result = [value(value.answer)];
