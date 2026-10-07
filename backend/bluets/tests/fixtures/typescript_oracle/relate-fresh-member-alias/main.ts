// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

type Shape = {value: number};
let holder: {item: Shape} = {item: {value: 1}};
const source = {value: 2, extra: true};
holder.item = source;
