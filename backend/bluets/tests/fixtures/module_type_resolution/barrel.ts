// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export type { Item as ImportItem } from "conditional-types" with { "resolution-mode": "import" };
export type { Item as RequireItem } from "conditional-types" with { "resolution-mode": "require" };
