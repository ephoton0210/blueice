// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
interface Props { a: number; b: string }
export const f = ({ a, b }: Props): string => b + a;
export const g = ({ a }: Props, extra: number): number => a + extra;
export function h({ a, b }: Props): number { return a; }
export const k = ({ a: renamed }: Props): number => renamed;
export const m = ([x, y]: [number, number]): number => x + y;
export const n = ({ a = 1 }: { a?: number }): number => a;
