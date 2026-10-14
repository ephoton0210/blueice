// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Box { value: string | null = null; hasValue(): this is this & { value: string } { return this.value !== null; } } export function read(box: Box): string { if (box.hasValue()) { return box.value; } return ''; }
