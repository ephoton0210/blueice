// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original System binding publication; no global loader or I/O is acquired.
var __SYS_values = Object.create(null), __SYS_getters = Object.create(null);
function __SYS_publish(name, value) {
    if (Object.prototype.hasOwnProperty.call(__SYS_values, name)) {
        var previous = __SYS_values[name];
        if (previous === value || (previous !== previous && value !== value)) return value;
    }
    __SYS_values[name] = value;
    return __SYS_export(name, value);
}
function __SYS_refresh() {
    for (var name in __SYS_getters) __SYS_publish(name, __SYS_getters[name]());
}
function __SYS_slot(name) {
    Object.defineProperty(exports, name, {
        enumerable: true, configurable: true,
        get: function () { return __SYS_values[name]; },
        set: function (value) { __SYS_publish(name, value); __SYS_refresh(); }
    });
}
function __SYS_define(owner, name, descriptor) {
    if (name === '__esModule') return owner;
    if (descriptor.get) {
        __SYS_getters[name] = descriptor.get;
        Object.defineProperty(owner, name, descriptor);
        __SYS_publish(name, descriptor.get());
    } else {
        if (!Object.prototype.hasOwnProperty.call(owner, name)) __SYS_slot(name);
        owner[name] = descriptor.value;
    }
    return owner;
}
function __SYS_changed(value) { __SYS_refresh(); return value; }
