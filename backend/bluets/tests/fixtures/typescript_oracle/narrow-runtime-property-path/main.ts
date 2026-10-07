// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function read(box: { inner: { value: string | number } }): number { if (typeof box.inner.value === 'string') { return box.inner.value.length; } return box.inner.value; } console.log(read({ inner: { value: 'Ada' } }), read({ inner: { value: 42 } }));
