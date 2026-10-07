// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

type Item = { left: number } | { right: string }; export function measure(value: Item): number { if ("left" in value) { return value.left; } return value.right.length; }
console.log(measure({left: 4}), measure({right: "abc"}));
