// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original promise driver for the generator's ordinary completion protocol.
// The first step is synchronous; each suspended value resumes in a promise job.
function __blueice_target_async(receiver, argumentsList, body) {
    return new Promise(function (resolve, reject) {
        var iterator;
        try { iterator = body.apply(receiver, argumentsList); }
        catch (error) { reject(error); return; }
        function resume(method, value) {
            var completion;
            try { completion = iterator[method](value); }
            catch (error) { reject(error); return; }
            if (completion.done) { resolve(completion.value); return; }
            Promise.resolve(completion.value).then(function (value) {
                resume("next", value);
            }, function (error) {
                resume("throw", error);
            });
        }
        resume("next", void 0);
    });
}
