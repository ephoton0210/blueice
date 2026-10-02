// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Registers every host call an extension can import. Do not add WASI or an
/// ambient utility import here: each function is intentionally a bounded,
/// protocol-shaped capability request whose final authorization belongs to
/// core.
pub(super) fn install_blueice_abi(linker: &mut Linker<RuntimeState>) -> Result<(), String> {
    linker
        .func_wrap(
            "blueice",
            "dom_read_utf8",
            |mut caller: Caller<'_, RuntimeState>, tab_id: i64, destination: i32, capacity: i32| {
                dom_read_utf8(&mut caller, tab_id, destination, capacity)
            },
        )
        .map_err(|error| format!("could not define the dom_read_utf8 ABI import: {error}"))?;
    linker
        .func_wrap(
            "blueice",
            "dom_read_ephemeral_utf8",
            |mut caller: Caller<'_, RuntimeState>, tab_id: i64, destination: i32, capacity: i32| {
                dom_read_ephemeral_utf8(&mut caller, tab_id, destination, capacity)
            },
        )
        .map_err(|error| {
            format!("could not define the dom_read_ephemeral_utf8 ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "network_response_utf8",
            |mut caller: Caller<'_, RuntimeState>, tab_id: i64, destination: i32, capacity: i32| {
                network_response_utf8(&mut caller, tab_id, destination, capacity)
            },
        )
        .map_err(|error| {
            format!("could not define the network_response_utf8 ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "network_trace_utf8",
            |mut caller: Caller<'_, RuntimeState>, tab_id: i64, destination: i32, capacity: i32| {
                network_trace_utf8(&mut caller, tab_id, destination, capacity)
            },
        )
        .map_err(|error| format!("could not define the network_trace_utf8 ABI import: {error}"))?;
    linker
        .func_wrap(
            "blueice",
            "set_toolbar_button_utf8",
            |mut caller: Caller<'_, RuntimeState>, pointer: i32, length: i32| {
                set_toolbar_button_utf8(&mut caller, pointer, length)
            },
        )
        .map_err(|error| {
            format!("could not define the set_toolbar_button_utf8 ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "clear_toolbar_button",
            |mut caller: Caller<'_, RuntimeState>| clear_toolbar_button(&mut caller),
        )
        .map_err(|error| {
            format!("could not define the clear_toolbar_button ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "show_popup_utf8",
            |mut caller: Caller<'_, RuntimeState>,
             tab_id: i64,
             title_ptr: i32,
             title_len: i32,
             body_ptr: i32,
             body_len: i32| {
                show_popup_utf8(
                    &mut caller,
                    tab_id,
                    title_ptr,
                    title_len,
                    body_ptr,
                    body_len,
                )
            },
        )
        .map_err(|error| format!("could not define the show_popup_utf8 ABI import: {error}"))?;
    linker
        .func_wrap(
            "blueice",
            "show_popup_action_utf8",
            |mut caller: Caller<'_, RuntimeState>,
             tab_id: i64,
             title_ptr: i32,
             title_len: i32,
             body_ptr: i32,
             body_len: i32,
             action_ptr: i32,
             action_len: i32| {
                show_popup_action_utf8(
                    &mut caller,
                    tab_id,
                    title_ptr,
                    title_len,
                    body_ptr,
                    body_len,
                    action_ptr,
                    action_len,
                )
            },
        )
        .map_err(|error| {
            format!("could not define the show_popup_action_utf8 ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "clear_popup",
            |mut caller: Caller<'_, RuntimeState>| clear_popup(&mut caller),
        )
        .map_err(|error| format!("could not define the clear_popup ABI import: {error}"))?;
    linker
        .func_wrap(
            "blueice",
            "set_text_input_value",
            |mut caller: Caller<'_, RuntimeState>,
             tab_id: i64,
             node_id: i64,
             value_ptr: i32,
             value_len: i32| {
                set_text_input_value(&mut caller, tab_id, node_id, value_ptr, value_len)
            },
        )
        .map_err(|error| {
            format!("could not define the set_text_input_value ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "set_checkbox_checked",
            |mut caller: Caller<'_, RuntimeState>, tab_id: i64, node_id: i64, checked: i32| {
                set_checkbox_checked(&mut caller, tab_id, node_id, checked)
            },
        )
        .map_err(|error| {
            format!("could not define the set_checkbox_checked ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "set_radio_checked",
            |mut caller: Caller<'_, RuntimeState>, tab_id: i64, node_id: i64| {
                set_radio_checked(&mut caller, tab_id, node_id)
            },
        )
        .map_err(|error| format!("could not define the set_radio_checked ABI import: {error}"))?;
    linker
        .func_wrap(
            "blueice",
            "select_option",
            |mut caller: Caller<'_, RuntimeState>, tab_id: i64, node_id: i64| {
                select_option(&mut caller, tab_id, node_id)
            },
        )
        .map_err(|error| format!("could not define the select_option ABI import: {error}"))?;
    linker
        .func_wrap(
            "blueice",
            "set_textarea_value",
            |mut caller: Caller<'_, RuntimeState>,
             tab_id: i64,
             node_id: i64,
             value_ptr: i32,
             value_len: i32| {
                set_textarea_value(&mut caller, tab_id, node_id, value_ptr, value_len)
            },
        )
        .map_err(|error| format!("could not define the set_textarea_value ABI import: {error}"))?;
    linker
        .func_wrap(
            "blueice",
            "set_range_input_value",
            |mut caller: Caller<'_, RuntimeState>, tab_id: i64, node_id: i64, value: i64| {
                set_range_input_value(&mut caller, tab_id, node_id, value)
            },
        )
        .map_err(|error| {
            format!("could not define the set_range_input_value ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "set_visible_leaf_text",
            |mut caller: Caller<'_, RuntimeState>,
             tab_id: i64,
             node_id: i64,
             value_ptr: i32,
             value_len: i32| {
                set_visible_leaf_text(&mut caller, tab_id, node_id, value_ptr, value_len)
            },
        )
        .map_err(|error| {
            format!("could not define the set_visible_leaf_text ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "set_visible_text_content",
            |mut caller: Caller<'_, RuntimeState>,
             tab_id: i64,
             node_id: i64,
             value_ptr: i32,
             value_len: i32| {
                set_visible_text_content(&mut caller, tab_id, node_id, value_ptr, value_len)
            },
        )
        .map_err(|error| {
            format!("could not define the set_visible_text_content ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "register_network_block_url",
            |mut caller: Caller<'_, RuntimeState>, url_ptr: i32, url_len: i32| {
                register_network_block_url(&mut caller, url_ptr, url_len)
            },
        )
        .map_err(|error| {
            format!("could not define the register_network_block_url ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "clear_network_block_urls",
            |mut caller: Caller<'_, RuntimeState>| clear_network_block_urls(&mut caller),
        )
        .map_err(|error| {
            format!("could not define the clear_network_block_urls ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "register_network_block_host",
            |mut caller: Caller<'_, RuntimeState>, host_ptr: i32, host_len: i32| {
                register_network_block_host(&mut caller, host_ptr, host_len)
            },
        )
        .map_err(|error| {
            format!("could not define the register_network_block_host ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "register_network_block_path_prefix",
            |mut caller: Caller<'_, RuntimeState>,
             host_ptr: i32,
             host_len: i32,
             path_ptr: i32,
             path_len: i32| {
                register_network_block_path_prefix(
                    &mut caller,
                    host_ptr,
                    host_len,
                    path_ptr,
                    path_len,
                )
            },
        )
        .map_err(|error| {
            format!("could not define the register_network_block_path_prefix ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "register_network_redirect_url",
            |mut caller: Caller<'_, RuntimeState>,
             source_ptr: i32,
             source_len: i32,
             target_ptr: i32,
             target_len: i32| {
                register_network_redirect_url(
                    &mut caller,
                    source_ptr,
                    source_len,
                    target_ptr,
                    target_len,
                )
            },
        )
        .map_err(|error| {
            format!("could not define the register_network_redirect_url ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "storage_get_utf8",
            |mut caller: Caller<'_, RuntimeState>,
             key_ptr: i32,
             key_len: i32,
             destination: i32,
             capacity: i32| {
                storage_get_utf8(&mut caller, key_ptr, key_len, destination, capacity)
            },
        )
        .map_err(|error| format!("could not define the storage_get_utf8 ABI import: {error}"))?;
    linker
        .func_wrap(
            "blueice",
            "storage_set_utf8",
            |mut caller: Caller<'_, RuntimeState>,
             key_ptr: i32,
             key_len: i32,
             value_ptr: i32,
             value_len: i32| {
                storage_set_utf8(&mut caller, key_ptr, key_len, value_ptr, value_len)
            },
        )
        .map_err(|error| format!("could not define the storage_set_utf8 ABI import: {error}"))?;
    linker
        .func_wrap(
            "blueice",
            "storage_remove_utf8",
            |mut caller: Caller<'_, RuntimeState>, key_ptr: i32, key_len: i32| {
                storage_remove_utf8(&mut caller, key_ptr, key_len)
            },
        )
        .map_err(|error| format!("could not define the storage_remove_utf8 ABI import: {error}"))?;
    linker
        .func_wrap(
            "blueice",
            "durable_storage_get_utf8",
            |mut caller: Caller<'_, RuntimeState>,
             key_ptr: i32,
             key_len: i32,
             destination: i32,
             capacity: i32| {
                storage_get(&mut caller, key_ptr, key_len, destination, capacity, true)
            },
        )
        .map_err(|error| {
            format!("could not define the durable_storage_get_utf8 ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "durable_storage_set_utf8",
            |mut caller: Caller<'_, RuntimeState>,
             key_ptr: i32,
             key_len: i32,
             value_ptr: i32,
             value_len: i32| {
                storage_set(&mut caller, key_ptr, key_len, value_ptr, value_len, true)
            },
        )
        .map_err(|error| {
            format!("could not define the durable_storage_set_utf8 ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "durable_storage_remove_utf8",
            |mut caller: Caller<'_, RuntimeState>, key_ptr: i32, key_len: i32| {
                storage_remove(&mut caller, key_ptr, key_len, true)
            },
        )
        .map_err(|error| {
            format!("could not define the durable_storage_remove_utf8 ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "durable_storage_keys_utf8",
            |mut caller: Caller<'_, RuntimeState>, destination: i32, capacity: i32| {
                durable_storage_keys_utf8(&mut caller, destination, capacity)
            },
        )
        .map_err(|error| {
            format!("could not define the durable_storage_keys_utf8 ABI import: {error}")
        })?;
    linker
        .func_wrap(
            "blueice",
            "runtime_event_kind",
            |caller: Caller<'_, RuntimeState>| runtime_event_kind(&caller),
        )
        .map_err(|error| format!("could not define the runtime_event_kind ABI import: {error}"))?;
    linker
        .func_wrap(
            "blueice",
            "runtime_event_tab_id",
            |caller: Caller<'_, RuntimeState>| runtime_event_tab_id(&caller),
        )
        .map_err(|error| {
            format!("could not define the runtime_event_tab_id ABI import: {error}")
        })?;
    Ok(())
}
