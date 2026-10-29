// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original per-function continuation adapter for the published __generator ABI.
// The external helper owns iterator, delegation and exception completion state.
__BLUEICE_PENDING_BRANCH__
var __BLUEICE_SOURCE__ = {
    label: __BLUEICE_NATIVE__.label === 0 ? __BLUEICE_ENTRY__ : __BLUEICE_COUNT__ - __BLUEICE_NATIVE__.label,
    sent: __BLUEICE_NATIVE__.sent
};
var __BLUEICE_ACTION__ = (function () {
    while (true) {
        switch (__BLUEICE_SOURCE__.label) {
            __BLUEICE_CASES__
            default: throw new Error("Invalid generator continuation");
        }
    }
}).call(this);
switch (__BLUEICE_ACTION__.kind) {
    case "yield":
        __BLUEICE_NATIVE__.label = __BLUEICE_COUNT__ - __BLUEICE_SOURCE__.label - 1;
        __BLUEICE_YIELD__
    __BLUEICE_DELEGATE__
    case "enter":
        __BLUEICE_NATIVE__.trys.push([
            __BLUEICE_NATIVE__.label,
            __BLUEICE_ACTION__.caught < 0 ? void 0 : __BLUEICE_COUNT__ - __BLUEICE_ACTION__.caught,
            __BLUEICE_ACTION__.finalizer < 0 ? void 0 : __BLUEICE_COUNT__ - __BLUEICE_ACTION__.finalizer,
            __BLUEICE_EXITS__[__BLUEICE_ACTION__.after] || __BLUEICE_COUNT__ - __BLUEICE_ACTION__.after
        ]);
        return [3, __BLUEICE_COUNT__ - __BLUEICE_ACTION__.entry];
    case "leave":
        return [3, __BLUEICE_NATIVE__.trys[__BLUEICE_NATIVE__.trys.length - 1][3]];
    case "finish":
        return [7];
    case "return":
        return [2, __BLUEICE_ACTION__.value];
    case "throw":
        return [6, __BLUEICE_ACTION__.value];
    case "jump":
        return [3, __BLUEICE_EXITS__[__BLUEICE_ACTION__.label] || __BLUEICE_COUNT__ - __BLUEICE_ACTION__.label];
    default:
        throw new Error("Invalid generator action");
}
