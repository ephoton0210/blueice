// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original versioned declarations for the owned ECMAScript surface.
interface Promise<T> { finally(onfinally?: () => void): Promise<T>; }

declare namespace Symbol { export const asyncIterator: symbol; }
