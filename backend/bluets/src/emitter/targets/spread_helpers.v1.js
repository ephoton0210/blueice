// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function __blueice_target_spread_array(target, source, spread, iterate) {
    if (spread && iterate) {
        var record = __blueice_target_iterator(source);
        var result;
        while (!(result = __blueice_target_step_result(record.next.call(record.iterator))).done) {
            target[target.length] = result.value;
        }
    } else {
        var offset = target.length;
        var length = source.length;
        target.length = offset + length;
        for (var index = 0; index < length; index++) {
            if (spread || index in source) target[offset + index] = source[index];
        }
    }
    return target;
}

function __blueice_target_spread_call(callee, receiver, values) {
    return Function.prototype.apply.call(callee, receiver, values);
}
