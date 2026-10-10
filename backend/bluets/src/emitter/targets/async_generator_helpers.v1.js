// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original queued async-generator requests over the native generator protocol.
function __blueice_target_async_generator(receiver, argumentsList, body) {
    var generator = body.apply(receiver, argumentsList);
    var requests = [];
    var iterator = {
        next: function (value) { return submit(this, "next", value); },
        return: function (value) { return submit(this, "return", value); },
        throw: function (value) { return submit(this, "throw", value); }
    };
    iterator[Symbol.asyncIterator] = function () { return this; };
    function submit(receiver, method, value) {
        if (receiver !== iterator) {
            return Promise.reject(new TypeError("Async generator receiver is invalid"));
        }
        return new Promise(function (resolve, reject) {
            requests.push({ method: method, value: value, resolve: resolve, reject: reject });
            if (requests.length === 1) dispatch();
        });
    }
    function dispatch() {
        var request = requests[0];
        if (request.method === "return") {
            Promise.resolve(request.value).then(function (value) {
                resume("return", value);
            }, function (error) { resume("throw", error); });
        } else { resume(request.method, request.value); }
    }
    function finish(rejected, value) {
        var request = requests.shift();
        if (rejected) request.reject(value);
        else request.resolve(value);
        if (requests.length) dispatch();
    }
    function resume(method, value) {
        var completion;
        try { completion = generator[method](value); }
        catch (error) { finish(true, error); return; }
        if (completion.done) {
            finish(false, { value: completion.value, done: true });
            return;
        }
        var suspension = completion.value;
        Promise.resolve(suspension.value).then(function (value) {
            if (suspension.awaiting) resume("next", value);
            else finish(false, { value: value, done: false });
        }, function (error) { resume("throw", error); });
    }
    return iterator;
}

function __blueice_target_generator_suspension(awaiting, value) {
    return { awaiting: awaiting, value: value };
}
