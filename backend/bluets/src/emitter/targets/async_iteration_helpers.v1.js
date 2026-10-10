// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original iterator record and asynchronous-from-synchronous value adapter.
function __blueice_target_async_iterator(source) {
    var method = source[Symbol.asyncIterator];
    var iterator;
    if (method !== null && method !== void 0) {
        iterator = __blueice_target_iterator_result(method.call(source));
        return { iterator: iterator, next: iterator.next };
    }
    iterator = __blueice_target_iterator_result(source[Symbol.iterator].call(source));
    var next = iterator.next;
    function adapt(result) {
        result = __blueice_target_iterator_result(result);
        var done = !!result.done;
        var value = result.value;
        return Promise.resolve(value).then(function (value) {
            return { value: value, done: done };
        });
    }
    var adapter = {
        next: function () {
            try { return adapt(next.apply(iterator, arguments)); }
            catch (error) { return Promise.reject(error); }
        },
        return: function (value) {
            try {
                var closing = iterator.return;
                if (closing === null || closing === void 0) {
                    return Promise.resolve({ value: value, done: true });
                }
                return adapt(closing.apply(iterator, arguments));
            } catch (error) { return Promise.reject(error); }
        }
    };
    return { iterator: adapter, next: adapter.next };
}

function __blueice_target_iterator_result(value) {
    if (value === null || (typeof value !== "object" && typeof value !== "function")) {
        throw new TypeError("Iterator result is not an object");
    }
    return value;
}
