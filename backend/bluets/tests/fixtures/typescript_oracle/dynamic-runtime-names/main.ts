// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export const Anonymous = class { value = 3; }; export const Named = class Inner { value = 4; }; console.log(Anonymous.name, Named.name, new Anonymous().value + new Named().value);
