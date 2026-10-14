// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export let current = 20; export function bump(): void { current += 22; } export function unbound(this: unknown): boolean { return this === undefined; } export default function base(): number { return 20; }
