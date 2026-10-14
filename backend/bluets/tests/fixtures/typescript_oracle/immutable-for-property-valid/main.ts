// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const receiver: {for(value: boolean): void}; const key: string = "value"; declare const holder: {value: number}; receiver.for(key in holder);
