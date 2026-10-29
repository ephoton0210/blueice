// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function __blueice_target_binding_read(source, count) {
    var record = __blueice_target_binding_iterator(source);
    var values = [], step, failed = false, error;
    try {
        while (count === null || values.length < count) {
            step = __blueice_target_binding_step(record.next.call(record.iterator));
            if (step.done) break;
            values.push(step.value);
        }
    } catch (caught) { failed = true; error = caught; }
    finally {
        try {
            if (step && !step.done) {
                var closing = record.iterator.return;
                if (closing !== null && closing !== void 0) {
                    __blueice_target_binding_step(closing.call(record.iterator));
                }
            }
        } finally { if (failed) throw error; }
    }
    return values;
}
