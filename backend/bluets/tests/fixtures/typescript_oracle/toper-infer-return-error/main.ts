// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

type Result<T> = T extends (...args: any[]) => infer R ? R : never; export const value: Result<(value: number) => string> = 1;
