// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

type Left = {label: string; (value: number): number;}; type Right = {readonly label: string; (value: number): number;}; declare const left: Left; export const value: Right = left;
