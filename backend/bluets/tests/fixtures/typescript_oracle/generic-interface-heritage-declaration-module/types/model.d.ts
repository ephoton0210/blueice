// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Envelope<T> { payload: T }
interface Tagged { tag: string }
export interface Labeled<T extends string = string> extends Envelope<T>, Tagged { label: T }
