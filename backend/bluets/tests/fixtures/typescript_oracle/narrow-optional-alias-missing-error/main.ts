// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

type Maybe = { value: number } | null; export function read(receiver: Maybe): number { return receiver?.missing ?? 0; }
