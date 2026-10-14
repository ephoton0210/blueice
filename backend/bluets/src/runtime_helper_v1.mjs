// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The v1 emitted boundary supports only reviewed primitive-string contracts.
// It uses no replaceable globals or prototype methods while inspecting a value.
export const BLUE_TS_RUNTIME_HELPER_VERSION = "bluets-runtime-helper-v1";

const REJECTED = "BlueTS runtime contract rejected the value";

export function validateStringV1(value, maxBytes) {
    if (typeof value !== "string"
        || typeof maxBytes !== "number"
        || maxBytes < 0
        || maxBytes > 9007199254740991
        || maxBytes % 1 !== 0) {
        throw REJECTED;
    }

    let bytes = 0;
    for (let index = 0; index < value.length; index += 1) {
        const unit = value[index];
        if (unit < "\u0080") {
            bytes += 1;
        } else if (unit < "\u0800") {
            bytes += 2;
        } else if (unit >= "\uD800" && unit <= "\uDBFF") {
            if (index + 1 >= value.length) {
                throw REJECTED;
            }
            const low = value[index + 1];
            if (low < "\uDC00" || low > "\uDFFF") {
                throw REJECTED;
            }
            bytes += 4;
            index += 1;
        } else if (unit >= "\uDC00" && unit <= "\uDFFF") {
            throw REJECTED;
        } else {
            bytes += 3;
        }
        if (bytes > maxBytes) {
            throw REJECTED;
        }
    }
    return value;
}
