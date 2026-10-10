// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function __blueice_target_inherit_class(derived, base) {
    if (base !== null && typeof base !== "function") throw new TypeError("Base is not a constructor");
    if (Object.setPrototypeOf) Object.setPrototypeOf(derived, base);
    else if (base !== null) {
        var keys = Object.getOwnPropertyNames(base);
        if (Object.getOwnPropertySymbols) keys = keys.concat(Object.getOwnPropertySymbols(base));
        for (var index = 0; index < keys.length; index++) {
            var key = keys[index];
            if (!Object.prototype.hasOwnProperty.call(derived, key)) {
                Object.defineProperty(derived, key, Object.getOwnPropertyDescriptor(base, key));
            }
        }
    }
    derived.prototype = Object.create(base === null ? null : base.prototype);
    Object.defineProperty(derived.prototype, "constructor", {
        value: derived, writable: true, configurable: true
    });
}

function __blueice_target_call_base(base, receiver, values) {
    var result = Function.prototype.apply.call(base, receiver, values);
    return result !== null && (typeof result === "object" || typeof result === "function")
        ? result : receiver;
}
