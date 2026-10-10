// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export class Box { private saved: number = 42; get value(): number { return this.saved; } set value(answer: number) { this.saved = answer; } } export const result = [new Box().value];
