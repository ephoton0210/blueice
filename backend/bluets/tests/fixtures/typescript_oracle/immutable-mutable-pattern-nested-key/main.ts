// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const holder: {inner: {value: number}} = {inner: {value: 1}}; const key = "inner"; [holder[key].value] = [2];
