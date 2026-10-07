// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

type Item = { kind: "number"; value: number } | { kind: "text"; value: string }; export function measure(value: Item): number { if (value.kind === "number") { return value.value; } return value.value.length; }
console.log(measure({kind:"number",value:6}),measure({kind:"text",value:"abc"}));
