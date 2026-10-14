// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const entries: Map<string, number>;
export const entry = entries.get("key");
declare const members: Set<number>;
export const present = members.has(1);
