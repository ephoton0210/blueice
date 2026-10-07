// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare function fail(): never; function sink(value: void): void {} export function read(value: string | null): string { if (value === null) { sink(fail()); } return value; }
