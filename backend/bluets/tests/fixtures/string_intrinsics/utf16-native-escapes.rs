// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Pinned Native TypeScript5.9.3 parser values, independent of BlueTS.
pub const ESCAPES: &[(&str, &[u16])] = &[
    (
        r###""\u{000000000000010428}word""###,
        &[0xd801, 0xdc28, 0x0077, 0x006f, 0x0072, 0x0064],
    ),
    (
        r###""\u{00000000D800}word""###,
        &[0xd800, 0x0077, 0x006f, 0x0072, 0x0064],
    ),
    (
        r###""wo\
rd""###,
        &[0x0077, 0x006f, 0x0072, 0x0064],
    ),
];
