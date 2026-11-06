// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Private namespace imports follow TypeScript 5.9.3 own-key and descriptor roles.
var __importStar = (this && this.__importStar) || (function () {
    var names = function (value) {
        names = Object.getOwnPropertyNames || function (value) {
            var result = [];
            for (var key in value) {
                if (Object.prototype.hasOwnProperty.call(value, key)) result.push(key);
            }
            return result;
        };
        return names(value);
    };
    var bind = Object.create ? function (target, source, key) {
        var descriptor = Object.getOwnPropertyDescriptor(source, key);
        if (!descriptor || ("get" in descriptor ? !source.__esModule : descriptor.writable || descriptor.configurable)) {
            descriptor = { enumerable: true, get: function () { return source[key]; } };
        }
        Object.defineProperty(target, key, descriptor);
    } : function (target, source, key) { target[key] = source[key]; };
    var setDefault = Object.create ? function (target, source) {
        Object.defineProperty(target, "default", { enumerable: true, value: source });
    } : function (target, source) { target["default"] = source; };
    return function (value) {
        if (value && value.__esModule) return value;
        var result = {};
        if (value != null) {
            var keys = names(value);
            for (var index = 0; index < keys.length; index++) {
                if (keys[index] !== "default") bind(result, value, keys[index]);
            }
        }
        setDefault(result, value);
        return result;
    };
})();
