// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare function takeNumber(value: number): void;
declare function takeString(value: string): void;
function probe(value: string | number): void { if (typeof value === "number") { { let value: string = "shadow"; takeString(value); } takeNumber(value); } }
export {};
