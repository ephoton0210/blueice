// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Collector { collect(...values: number[]): number; collect(value: string): string; } declare const collector: Collector; export const answer: number = collector.collect(1, 2);
