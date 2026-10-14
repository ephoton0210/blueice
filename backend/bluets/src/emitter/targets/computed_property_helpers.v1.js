// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function __blueice_target_property_key(value) {
    if (value !== null && (typeof value === "object" || typeof value === "function")) {
        var exotic = typeof Symbol === "function" && Symbol.toPrimitive;
        var method = exotic ? value[exotic] : void 0;
        var primitive;
        var converted = false;
        if (method !== null && method !== void 0) {
            primitive = method.call(value, "string");
            converted = primitive === null || (typeof primitive !== "object" && typeof primitive !== "function");
            if (!converted) throw new TypeError("Property key conversion returned an object");
        } else {
            var names = ["toString", "valueOf"];
            for (var index = 0; index < names.length && !converted; index++) {
                method = value[names[index]];
                if (typeof method === "function") {
                    primitive = method.call(value);
                    converted = primitive === null || (typeof primitive !== "object" && typeof primitive !== "function");
                }
            }
            if (!converted) throw new TypeError("Cannot convert property key to primitive");
        }
        value = primitive;
    }
    return typeof value === "symbol" ? value : String(value);
}

function __blueice_target_computed_property(target, key, value) {
    Object.defineProperty(target, key, {
        value: value, writable: true, enumerable: true, configurable: true
    });
    return target;
}
