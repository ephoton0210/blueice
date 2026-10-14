// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

type Left = {(value: string): string; (value: number): number;}; type Right = {(value: string): string; (value: number): boolean;}; declare const left: Left; export const value: Right = left;
