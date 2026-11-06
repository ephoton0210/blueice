// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function __blueice_target_generator(receiver, entry, body) {
    var started = false, completed = false, busy = false;
    var input, operation, delegate;
    var returnSignal = {};
    var regions = [];
    var context = {
        label: entry,
        exception: void 0,
        sent: function () {
            if (operation === "throw") throw input;
            if (operation === "return") throw returnSignal;
            return input;
        }
    };
    var iterator = {
        next: function (value) { return resume("next", value); },
        return: function (value) { return resume("return", value); },
        throw: function (value) { return resume("throw", value); }
    };
    if (typeof Symbol === "function" && Symbol.iterator) {
        iterator[Symbol.iterator] = function () { return this; };
    }
    function abrupt(kind, value) {
        while (regions.length) {
            if (kind === "jump" && regions.length <= value.depth) {
                context.label = value.label;
                return null;
            }
            var region = regions[regions.length - 1];
            if (region.phase === "finally") { regions.pop(); continue; }
            if (kind === "throw" && region.phase === "body" && region.caught >= 0) {
                region.phase = "catch";
                context.exception = value;
                context.label = region.caught;
                return null;
            }
            if (region.finalizer >= 0) {
                region.phase = "finally";
                region.pending = { kind: kind, value: value };
                context.label = region.finalizer;
                return null;
            }
            regions.pop();
        }
        if (kind === "jump") { context.label = value.label; return null; }
        completed = true;
        if (kind === "throw") throw value;
        return { value: value, done: true };
    }
    function resume(kind, value) {
        if (busy) throw new TypeError("Generator is already executing");
        if (completed || !started && kind !== "next") {
            completed = true;
            if (kind === "throw") throw value;
            return { value: kind === "return" ? value : void 0, done: true };
        }
        busy = true;
        operation = kind;
        input = started ? value : void 0;
        started = true;
        try {
            while (!completed) {
                var action, result;
                try {
                    if (delegate) {
                        var method = operation === "next" ? delegate.iterator.next : delegate.iterator[operation];
                        if (!method) {
                            if (operation === "throw") {
                                var closing = delegate.iterator.return;
                                if (closing) closing.call(delegate.iterator);
                                throw input;
                            }
                            delegate = null;
                            if (operation === "return") {
                                result = abrupt("return", input);
                                if (result) return result;
                                operation = "next"; input = void 0;
                                continue;
                            }
                        } else {
                            var step = __blueice_target_step_result(method.call(delegate.iterator, input));
                            if (!step.done) return step;
                            delegate = null;
                            if (operation === "return") {
                                result = abrupt("return", step.value);
                                if (result) return result;
                                operation = "next"; input = void 0;
                                continue;
                            }
                            input = step.value;
                            operation = "next";
                        }
                    }
                    action = body.call(receiver, context);
                } catch (error) {
                    delegate = null;
                    result = abrupt(error === returnSignal ? "return" : "throw",
                        error === returnSignal ? input : error);
                    if (result) return result;
                    operation = "next"; input = void 0;
                    continue;
                }
                if (action.kind === "yield") return { value: action.value, done: false };
                if (action.kind === "delegate") {
                    try { delegate = __blueice_target_iterator(action.value); }
                    catch (error) {
                        result = abrupt("throw", error);
                        if (result) return result;
                    }
                    operation = "next"; input = void 0;
                    continue;
                }
                if (action.kind === "enter") {
                    regions.push({ caught: action.caught, finalizer: action.finalizer,
                        after: action.after, phase: "body", pending: null });
                    context.label = action.entry;
                } else if (action.kind === "leave") {
                    var region = regions[regions.length - 1];
                    if (region.finalizer >= 0) {
                        region.phase = "finally";
                        region.pending = { kind: "normal", label: region.after };
                        context.label = region.finalizer;
                    } else { regions.pop(); context.label = region.after; }
                } else if (action.kind === "finish") {
                    var pending = regions.pop().pending;
                    if (pending.kind === "normal") context.label = pending.label;
                    else {
                        result = abrupt(pending.kind, pending.value);
                        if (result) return result;
                    }
                } else {
                    result = abrupt(action.kind, action.kind === "jump" ? action : action.value);
                    if (result) return result;
                }
                operation = "next"; input = void 0;
            }
            return { value: void 0, done: true };
        } finally { busy = false; }
    }
    return iterator;
}
