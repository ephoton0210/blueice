// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

type Shape = {value: number};
let service: {use: (value: Shape) => void} = {use(value: Shape): void {}};
service.use({value: 2, extra: true});
