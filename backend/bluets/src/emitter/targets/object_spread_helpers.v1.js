// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original implementation: spread copies enumerable values, while ordinary
// literal properties retain their descriptors. Each source is read in order.
function __blueice_target_object_properties(target, source, values, excluded) {
    if (source === null || source === void 0) return target;
    var object = Object(source);
    var keys = Object.getOwnPropertyNames(object);
    if (typeof Object.getOwnPropertySymbols === "function") {
        keys = keys.concat(Object.getOwnPropertySymbols(object));
    }
    for (var index = 0; index < keys.length; index++) {
        var key = keys[index];
        if (excluded && excluded.indexOf(key) >= 0) continue;
        var descriptor = Object.getOwnPropertyDescriptor(object, key);
        if (descriptor && (!values || descriptor.enumerable)) {
            if (values) {
                descriptor = { value: object[key], writable: true,
                    enumerable: true, configurable: true };
            }
            Object.defineProperty(target, key, descriptor);
        }
    }
    return target;
}
