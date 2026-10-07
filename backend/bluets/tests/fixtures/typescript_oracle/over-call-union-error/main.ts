// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function convert(value: string): string; function convert(value: number): number; function convert(value: string | number): string | number { return value; } export function read(value: string | number): void { convert(value); }
