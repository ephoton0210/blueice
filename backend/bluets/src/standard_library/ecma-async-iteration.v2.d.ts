// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original declarations shared by every library profile from ES2018.
interface AsyncIterator<T, R = any, N = any> {
    next(value?: N): Promise<IteratorResult<T, R>>;
    return(value?: R): Promise<IteratorResult<T, R>>;
    throw(error?: any): Promise<IteratorResult<T, R>>;
}

interface AsyncGenerator<T = unknown, R = any, N = any> {
    next(value?: N): Promise<IteratorResult<T, R>>;
    return(value: R): Promise<IteratorResult<T, R>>;
    throw(error: any): Promise<IteratorResult<T, R>>;
}

interface AsyncIterableIterator<T, R = any, N = any> {
    next(value?: N): Promise<IteratorResult<T, R>>;
    return(value?: R): Promise<IteratorResult<T, R>>;
    throw(error?: any): Promise<IteratorResult<T, R>>;
}

interface AsyncIterable<T> { }
