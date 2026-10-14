// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare function takeNumber(value: number): void;
declare function takeString(value: string): void;
type Packet = { kind: "number"; value: number } | { kind: "text"; value: string };
function probe(value: Packet): void { switch (value.kind) { case "number": takeNumber(value.value); break; case "text": takeString(value.value); break; } }
export {};
