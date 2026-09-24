;; This Source Code Form is subject to the terms of the Mozilla Public
;; License, v. 2.0. If a copy of the MPL was not distributed with this
;; file, You can obtain one at https://mozilla.org/MPL/2.0/.

;; A no-WASI BlueIce extension. Compile this source to extension.wasm before
;; loading extension.json. It creates a native toolbar button at startup and
;; shows fixed, non-interactive native text when the user activates it.
(module
  (import "blueice" "runtime_event_kind" (func $event_kind (result i32)))
  (import "blueice" "runtime_event_tab_id" (func $event_tab_id (result i64)))
  (import "blueice" "set_toolbar_button_utf8" (func $set_toolbar (param i32 i32) (result i32)))
  (import "blueice" "show_popup_utf8" (func $show_popup (param i64 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "Example")
  (data (i32.const 16) "Hello")
  (data (i32.const 32) "Ready")
  (func (export "blueice_start")
    call $event_kind
    i32.const 0
    i32.eq
    if
      i32.const 0
      i32.const 7
      call $set_toolbar
      i32.const 0
      i32.ne
      if unreachable end
    else
      call $event_kind
      i32.const 2
      i32.eq
      if
        call $event_tab_id
        i32.const 16
        i32.const 5
        i32.const 32
        i32.const 5
        call $show_popup
        i32.const 0
        i32.ne
        if unreachable end
      end
    end))
