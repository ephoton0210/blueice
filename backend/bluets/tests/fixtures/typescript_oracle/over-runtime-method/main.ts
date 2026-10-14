// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export class Box { convert(value: string): string; convert(value: number): number; convert(value: string | number): string | number { return value; } } console.log(new Box().convert('Ada'), new Box().convert(42));
