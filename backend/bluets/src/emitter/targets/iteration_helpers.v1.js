// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function __blueice_target_iterator(source) {
    var key = typeof Symbol === "function" && Symbol.iterator;
    var method = key ? source[key] : void 0;
    var iterator;
    if (method !== null && method !== void 0) {
        iterator = __blueice_target_step_result(method.call(source));
    } else if (source !== null && source !== void 0 && typeof source.length === "number") {
        var index = 0;
        iterator = {
            next: function () {
                return index < source.length
                    ? { value: source[index++], done: false }
                    : { value: void 0, done: true };
            }
        };
    } else {
        throw new TypeError("Value is not iterable");
    }
    return { iterator: iterator, next: iterator.next };
}

function __blueice_target_step_result(value) {
    if (value === null || (typeof value !== "object" && typeof value !== "function")) {
        throw new TypeError("Iterator result is not an object");
    }
    return value;
}
