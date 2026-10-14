// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function unbox<T>(box: { inner: { value: T } }): T { return box.inner.value; } export const answer: number = unbox({ inner: { value: 42 } });
