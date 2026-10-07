// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

type Tail<T> = T extends `tag-${infer U}` ? U : never; export const value: Tail<'tag-answer'> = 'wrong';
