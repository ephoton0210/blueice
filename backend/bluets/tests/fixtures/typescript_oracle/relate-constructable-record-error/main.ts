// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

interface Box {value: number;} type Left = {new (value: number): Box;}; type Right = {new (value: string): Box;}; declare const left: Left; export const value: Right = left;
