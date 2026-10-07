// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function measure(value: "left" | "right"): number { switch(value) { case "left": return 1; default: return 2; } }
console.log(measure("left"),measure("right"));
