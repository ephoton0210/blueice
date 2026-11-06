// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The pinned synchronous generator ABI acquires each next method on resume.
function __blueice_target_iterator(source) {
    var key = typeof Symbol === "function" && Symbol.iterator;
    var method = key && source[key];
    var iterator;
    if (method) {
        iterator = method.call(source);
    } else if (source && typeof source.length === "number") {
        var index = 0;
        var current = source;
        iterator = {
            next: function () {
                if (current && index >= current.length) current = void 0;
                return { value: current && current[index++], done: !current };
            }
        };
    } else {
        throw new TypeError(key ? "Object is not iterable." : "Symbol.iterator is not defined.");
    }
    return iterator ? { iterator: iterator } : null;
}

// Published __generator forwards the actual delegated result without validation.
function __blueice_target_step_result(value) {
    return value;
}
