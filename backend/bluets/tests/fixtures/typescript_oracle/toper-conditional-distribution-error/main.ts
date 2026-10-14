// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

type Choose<T> = T extends string ? T : never; export const value: Choose<'a' | 1> = 1;
