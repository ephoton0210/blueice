// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const holder: {readonly fixed: number; value: number} = {fixed: 1, value: 1}; const key = "fixed"; [holder[key]] = [2];
