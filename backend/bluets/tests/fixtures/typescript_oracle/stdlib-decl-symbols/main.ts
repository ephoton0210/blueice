// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare function made(): symbol;
export const registry = Symbol.for("x");
export const fresh = Symbol("x");
export const returned = made();
export const alias = registry;
let mutable = registry;
export const indirect = mutable;
export const annotated: symbol = Symbol.for("x");
export let widened = Symbol.for("x");
