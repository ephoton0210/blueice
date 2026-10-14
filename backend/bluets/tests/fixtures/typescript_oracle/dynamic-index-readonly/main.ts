// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class C { readonly [key: string]: number; value = 3; } const c = new C(); c["other"] = 4;
