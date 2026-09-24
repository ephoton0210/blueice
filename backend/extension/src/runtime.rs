// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The deliberately small Phase 9 WebAssembly execution boundary.
//!
//! A package is never given WASI, a filesystem preopen, process environment,
//! clocks, randomness, sockets, or arbitrary host imports. Its only imports
//! are the `blueice` functions defined below; each one serializes a request
//! onto the already-authenticated extension connection, so `core` remains the
//! sole capability and gatekeeper enforcement point.
//!
//! This is a one-shot reactor ABI, not a resident service worker: an extension
//! exports `blueice_start: () -> ()`, which runs once after the host's
//! authenticated `Hello` and core's internal runtime-start barrier, then once
//! again for each core-defined lifecycle event. Every event gets a fresh,
//! bounded instance; guest code can query its small, host-defined context but
//! never receives ambient process authority.

use crate::InstalledExtension;
use blueice_ipc::extension::{
    read_extension_reply, write_extension_request, ExtensionReply, ExtensionRequest,
    MAX_NETWORK_BLOCK_URL_BYTES, MAX_NETWORK_BLOCK_HOST_BYTES, MAX_NETWORK_BLOCK_PATH_BYTES, MAX_STORAGE_KEY_BYTES, MAX_STORAGE_VALUE_BYTES,
    MAX_TEXT_WRITE_BYTES, MAX_NETWORK_OBSERVATION_BYTES, MAX_NETWORK_TRACE_BYTES,
    MAX_EXTENSION_TOOLBAR_LABEL_BYTES,
    MAX_EXTENSION_POPUP_TITLE_BYTES, MAX_EXTENSION_POPUP_BODY_BYTES,
};
use std::os::unix::net::UnixStream;
use wasmtime::{
    Caller, Config, Engine, Extern, Linker, Module, Store, StoreLimits, StoreLimitsBuilder,
};

/// Fuel assigned to one extension invocation. Wasmtime accounts normal Wasm
/// instructions against this budget and traps a guest that runs out.
const MAX_FUEL: u64 = 1_000_000;

/// The maximum linear memory an extension instance may allocate or grow to.
const MAX_LINEAR_MEMORY_BYTES: usize = 1024 * 1024;

/// Representation JSON is bounded before it crosses from the host into guest
/// linear memory. This is an ABI-level resource limit rather than an implicit
/// allocation based on guest-controlled capacity.
const MAX_DOM_READ_BYTES: usize = 64 * 1024;

const RESULT_OK: i32 = 0;
const RESULT_ERROR: i32 = -1;
const RESULT_BUFFER_TOO_SMALL: i32 = -2;
const RESULT_INVALID_ARGUMENT: i32 = -3;
const RESULT_NOT_FOUND: i32 = -4;

/// The core-defined context for one fresh `blueice_start` invocation. The
/// integer values exposed through the ABI are stable: `0` is startup, `1`
/// is a successfully committed navigation, `2` is toolbar activation, and
/// `3` is activation of a native popup's single action button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeInvocation {
    Startup,
    NavigationCommitted { tab_id: u64 },
    ToolbarActivated { tab_id: u64 },
    PopupActionActivated { tab_id: u64 },
}

impl RuntimeInvocation {
    fn kind(self) -> i32 {
        match self {
            Self::Startup => 0,
            Self::NavigationCommitted { .. } => 1,
            Self::ToolbarActivated { .. } => 2,
            Self::PopupActionActivated { .. } => 3,
        }
    }

    fn tab_id(self) -> i64 {
        match self {
            Self::Startup => -1,
            Self::NavigationCommitted { tab_id } => i64::try_from(tab_id).unwrap_or(-1),
            Self::ToolbarActivated { tab_id } => i64::try_from(tab_id).unwrap_or(-1),
            Self::PopupActionActivated { tab_id } => i64::try_from(tab_id).unwrap_or(-1),
        }
    }
}

struct RuntimeState {
    stream: UnixStream,
    limits: StoreLimits,
    invocation: RuntimeInvocation,
}

/// Executes the single required `blueice_start: () -> ()` export from an
/// already validated installed package over an authenticated core connection.
///
/// The module bytes come from [`InstalledExtension`], not a second filesystem
/// read, preserving the exact package bytes used to derive the extension ID.
/// An error terminates the host process rather than permitting a partially
/// initialized extension to continue with unknown authority.
pub fn execute_installed_extension(
    extension: &InstalledExtension,
    stream: UnixStream,
) -> Result<(), String> {
    execute_installed_extension_for_invocation(extension, stream, RuntimeInvocation::Startup)
}

/// Executes the required entrypoint in a fresh resource-bounded instance for
/// one core-defined lifecycle invocation. The socket is a clone of the host's
/// authenticated stream; it is dropped when this invocation finishes before
/// the host waits for another event.
pub fn execute_installed_extension_for_invocation(
    extension: &InstalledExtension,
    stream: UnixStream,
    invocation: RuntimeInvocation,
) -> Result<(), String> {
    let mut config = Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config)
        .map_err(|error| format!("could not create the constrained Wasm engine: {error}"))?;
    let module = Module::new(&engine, extension.wasm_bytes()).map_err(|error| {
        format!(
            "could not compile validated extension module {}: {error}",
            extension.wasm_path().display()
        )
    })?;
    let mut linker = Linker::new(&engine);
    install_blueice_abi(&mut linker)?;

    let limits = StoreLimitsBuilder::new()
        .memory_size(MAX_LINEAR_MEMORY_BYTES)
        .table_elements(1_024)
        .instances(1)
        .tables(1)
        .memories(1)
        .build();
    let mut store = Store::new(
        &engine,
        RuntimeState {
            stream,
            limits,
            invocation,
        },
    );
    store.limiter(|state| &mut state.limits);
    store
        .set_fuel(MAX_FUEL)
        .map_err(|error| format!("could not set the extension fuel limit: {error}"))?;

    let instance = linker.instantiate(&mut store, &module).map_err(|error| {
        format!(
            "could not instantiate extension {} with the restricted BlueIce ABI: {error}",
            extension.extension_id()
        )
    })?;
    let start = instance
        .get_typed_func::<(), ()>(&mut store, "blueice_start")
        .map_err(|error| {
            format!(
                "extension {} must export blueice_start with signature () -> (): {error}",
                extension.extension_id()
            )
        })?;
    start.call(&mut store, ()).map_err(|error| {
        format!(
            "extension {} failed while running blueice_start within its resource limits: {error}",
            extension.extension_id()
        )
    })?;
    Ok(())
}

/// Registers every host call an extension can import. Do not add WASI or an
/// ambient utility import here: each function is intentionally a bounded,
/// protocol-shaped capability request whose final authorization belongs to
/// core.
fn install_blueice_abi(linker: &mut Linker<RuntimeState>) -> Result<(), String> {
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
            "network_response_utf8",
            |mut caller: Caller<'_, RuntimeState>, tab_id: i64, destination: i32, capacity: i32| {
                network_response_utf8(&mut caller, tab_id, destination, capacity)
            },
        )
        .map_err(|error| format!("could not define the network_response_utf8 ABI import: {error}"))?;
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
        .map_err(|error| format!("could not define the set_toolbar_button_utf8 ABI import: {error}"))?;
    linker
        .func_wrap(
            "blueice",
            "clear_toolbar_button",
            |mut caller: Caller<'_, RuntimeState>| clear_toolbar_button(&mut caller),
        )
        .map_err(|error| format!("could not define the clear_toolbar_button ABI import: {error}"))?;
    linker
        .func_wrap(
            "blueice",
            "show_popup_utf8",
            |mut caller: Caller<'_, RuntimeState>, tab_id: i64, title_ptr: i32, title_len: i32, body_ptr: i32, body_len: i32| {
                show_popup_utf8(&mut caller, tab_id, title_ptr, title_len, body_ptr, body_len)
            },
        )
        .map_err(|error| format!("could not define the show_popup_utf8 ABI import: {error}"))?;
    linker
        .func_wrap(
            "blueice",
            "show_popup_action_utf8",
            |mut caller: Caller<'_, RuntimeState>, tab_id: i64, title_ptr: i32, title_len: i32, body_ptr: i32, body_len: i32, action_ptr: i32, action_len: i32| {
                show_popup_action_utf8(&mut caller, tab_id, title_ptr, title_len, body_ptr, body_len, action_ptr, action_len)
            },
        )
        .map_err(|error| format!("could not define the show_popup_action_utf8 ABI import: {error}"))?;
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
            |mut caller: Caller<'_, RuntimeState>, host_ptr: i32, host_len: i32, path_ptr: i32, path_len: i32| {
                register_network_block_path_prefix(&mut caller, host_ptr, host_len, path_ptr, path_len)
            },
        )
        .map_err(|error| {
            format!("could not define the register_network_block_path_prefix ABI import: {error}")
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
            |mut caller: Caller<'_, RuntimeState>, key_ptr: i32, key_len: i32, destination: i32, capacity: i32| {
                storage_get(&mut caller, key_ptr, key_len, destination, capacity, true)
            },
        )
        .map_err(|error| format!("could not define the durable_storage_get_utf8 ABI import: {error}"))?;
    linker
        .func_wrap(
            "blueice",
            "durable_storage_set_utf8",
            |mut caller: Caller<'_, RuntimeState>, key_ptr: i32, key_len: i32, value_ptr: i32, value_len: i32| {
                storage_set(&mut caller, key_ptr, key_len, value_ptr, value_len, true)
            },
        )
        .map_err(|error| format!("could not define the durable_storage_set_utf8 ABI import: {error}"))?;
    linker
        .func_wrap(
            "blueice",
            "durable_storage_remove_utf8",
            |mut caller: Caller<'_, RuntimeState>, key_ptr: i32, key_len: i32| {
                storage_remove(&mut caller, key_ptr, key_len, true)
            },
        )
        .map_err(|error| format!("could not define the durable_storage_remove_utf8 ABI import: {error}"))?;
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

fn runtime_event_kind(caller: &Caller<'_, RuntimeState>) -> i32 {
    caller.data().invocation.kind()
}

fn runtime_event_tab_id(caller: &Caller<'_, RuntimeState>) -> i64 {
    caller.data().invocation.tab_id()
}

fn dom_read_utf8(
    caller: &mut Caller<'_, RuntimeState>,
    tab_id: i64,
    destination: i32,
    capacity: i32,
) -> i32 {
    let Ok(tab_id) = stable_id(tab_id) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok((destination, capacity)) = guest_range(destination, capacity, MAX_DOM_READ_BYTES) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let value = match request_core(caller, ExtensionRequest::DomReadTab { tab_id }) {
        Ok(ExtensionReply::DomReadResult { value }) => value,
        Ok(_) | Err(()) => return RESULT_ERROR,
    };
    let bytes = value.as_bytes();
    if bytes.len() > MAX_DOM_READ_BYTES || bytes.len() > capacity {
        return RESULT_BUFFER_TOO_SMALL;
    }
    if write_guest_bytes(caller, destination, bytes).is_err() {
        return RESULT_INVALID_ARGUMENT;
    }
    i32::try_from(bytes.len()).unwrap_or(RESULT_ERROR)
}

/// Copies JSON response metadata into guest memory. A missing HTTP response
/// returns RESULT_NOT_FOUND, distinct from authorization or transport errors.
fn network_response_utf8(
    caller: &mut Caller<'_, RuntimeState>,
    tab_id: i64,
    destination: i32,
    capacity: i32,
) -> i32 {
    let Ok(tab_id) = stable_id(tab_id) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok((destination, capacity)) =
        guest_range(destination, capacity, MAX_NETWORK_OBSERVATION_BYTES)
    else {
        return RESULT_INVALID_ARGUMENT;
    };
    let response = match request_core(caller, ExtensionRequest::ReadNetworkResponse { tab_id }) {
        Ok(ExtensionReply::NetworkResponseResult { response: Some(response) }) => response,
        Ok(ExtensionReply::NetworkResponseResult { response: None }) => return RESULT_NOT_FOUND,
        Ok(_) | Err(()) => return RESULT_ERROR,
    };
    let Ok(bytes) = serde_json::to_vec(&response) else {
        return RESULT_ERROR;
    };
    if bytes.len() > MAX_NETWORK_OBSERVATION_BYTES || bytes.len() > capacity {
        return RESULT_BUFFER_TOO_SMALL;
    }
    if write_guest_bytes(caller, destination, &bytes).is_err() {
        return RESULT_INVALID_ARGUMENT;
    }
    i32::try_from(bytes.len()).unwrap_or(RESULT_ERROR)
}

/// Copies a committed navigation's bounded request/redirect/response trace.
fn network_trace_utf8(
    caller: &mut Caller<'_, RuntimeState>,
    tab_id: i64,
    destination: i32,
    capacity: i32,
) -> i32 {
    let Ok(tab_id) = stable_id(tab_id) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok((destination, capacity)) = guest_range(destination, capacity, MAX_NETWORK_TRACE_BYTES)
    else {
        return RESULT_INVALID_ARGUMENT;
    };
    let trace = match request_core(caller, ExtensionRequest::ReadNetworkTrace { tab_id }) {
        Ok(ExtensionReply::NetworkTraceResult { trace: Some(trace) }) => trace,
        Ok(ExtensionReply::NetworkTraceResult { trace: None }) => return RESULT_NOT_FOUND,
        Ok(_) | Err(()) => return RESULT_ERROR,
    };
    let Ok(bytes) = serde_json::to_vec(&trace) else {
        return RESULT_ERROR;
    };
    if bytes.len() > MAX_NETWORK_TRACE_BYTES || bytes.len() > capacity {
        return RESULT_BUFFER_TOO_SMALL;
    }
    if write_guest_bytes(caller, destination, &bytes).is_err() {
        return RESULT_INVALID_ARGUMENT;
    }
    i32::try_from(bytes.len()).unwrap_or(RESULT_ERROR)
}

fn set_toolbar_button_utf8(
    caller: &mut Caller<'_, RuntimeState>,
    pointer: i32,
    length: i32,
) -> i32 {
    let Ok((pointer, length)) = guest_range(pointer, length, MAX_EXTENSION_TOOLBAR_LABEL_BYTES)
    else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok(bytes) = read_guest_bytes(caller, pointer, length) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok(label) = String::from_utf8(bytes) else {
        return RESULT_INVALID_ARGUMENT;
    };
    match request_core(caller, ExtensionRequest::SetToolbarButton { label }) {
        Ok(ExtensionReply::UiInjectAck) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

fn clear_toolbar_button(caller: &mut Caller<'_, RuntimeState>) -> i32 {
    match request_core(caller, ExtensionRequest::ClearToolbarButton) {
        Ok(ExtensionReply::UiInjectAck) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

fn show_popup_utf8(
    caller: &mut Caller<'_, RuntimeState>,
    tab_id: i64,
    title_ptr: i32,
    title_len: i32,
    body_ptr: i32,
    body_len: i32,
) -> i32 {
    let Ok(tab_id) = stable_id(tab_id) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let (Ok((title_ptr, title_len)), Ok((body_ptr, body_len))) = (
        guest_range(title_ptr, title_len, MAX_EXTENSION_POPUP_TITLE_BYTES),
        guest_range(body_ptr, body_len, MAX_EXTENSION_POPUP_BODY_BYTES),
    ) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let (Ok(title), Ok(body)) = (
        read_guest_bytes(caller, title_ptr, title_len),
        read_guest_bytes(caller, body_ptr, body_len),
    ) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let (Ok(title), Ok(body)) = (String::from_utf8(title), String::from_utf8(body)) else {
        return RESULT_INVALID_ARGUMENT;
    };
    match request_core(caller, ExtensionRequest::ShowPopup { tab_id, title, body }) {
        Ok(ExtensionReply::UiInjectAck) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

fn show_popup_action_utf8(
    caller: &mut Caller<'_, RuntimeState>,
    tab_id: i64,
    title_ptr: i32,
    title_len: i32,
    body_ptr: i32,
    body_len: i32,
    action_ptr: i32,
    action_len: i32,
) -> i32 {
    let Ok(tab_id) = stable_id(tab_id) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let (Ok((title_ptr, title_len)), Ok((body_ptr, body_len)), Ok((action_ptr, action_len))) = (
        guest_range(title_ptr, title_len, MAX_EXTENSION_POPUP_TITLE_BYTES),
        guest_range(body_ptr, body_len, MAX_EXTENSION_POPUP_BODY_BYTES),
        guest_range(action_ptr, action_len, MAX_EXTENSION_TOOLBAR_LABEL_BYTES),
    ) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let (Ok(title), Ok(body), Ok(action_label)) = (
        read_guest_bytes(caller, title_ptr, title_len),
        read_guest_bytes(caller, body_ptr, body_len),
        read_guest_bytes(caller, action_ptr, action_len),
    ) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let (Ok(title), Ok(body), Ok(action_label)) = (
        String::from_utf8(title),
        String::from_utf8(body),
        String::from_utf8(action_label),
    ) else {
        return RESULT_INVALID_ARGUMENT;
    };
    match request_core(caller, ExtensionRequest::ShowPopupAction {
        tab_id,
        title,
        body,
        action_label,
    }) {
        Ok(ExtensionReply::UiInjectAck) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

fn clear_popup(caller: &mut Caller<'_, RuntimeState>) -> i32 {
    match request_core(caller, ExtensionRequest::ClearPopup) {
        Ok(ExtensionReply::UiInjectAck) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

fn set_text_input_value(
    caller: &mut Caller<'_, RuntimeState>,
    tab_id: i64,
    node_id: i64,
    value_ptr: i32,
    value_len: i32,
) -> i32 {
    let (Ok(tab_id), Ok(node_id)) = (stable_id(tab_id), stable_id(node_id)) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok((value_ptr, value_len)) = guest_range(value_ptr, value_len, MAX_TEXT_WRITE_BYTES) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok(value) = read_guest_bytes(caller, value_ptr, value_len) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok(value) = String::from_utf8(value) else {
        return RESULT_INVALID_ARGUMENT;
    };
    match request_core(
        caller,
        ExtensionRequest::SetTextInputValue {
            tab_id,
            node_id,
            value,
        },
    ) {
        Ok(ExtensionReply::DomWriteAck) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

fn set_checkbox_checked(
    caller: &mut Caller<'_, RuntimeState>,
    tab_id: i64,
    node_id: i64,
    checked: i32,
) -> i32 {
    let (Ok(tab_id), Ok(node_id)) = (stable_id(tab_id), stable_id(node_id)) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let checked = match checked {
        0 => false,
        1 => true,
        _ => return RESULT_INVALID_ARGUMENT,
    };
    match request_core(
        caller,
        ExtensionRequest::SetCheckboxChecked {
            tab_id,
            node_id,
            checked,
        },
    ) {
        Ok(ExtensionReply::DomWriteAck) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

/// Selects one live radio through the version-5 `dom:write` operation. The
/// guest supplies no group name or `checked` Boolean: core derives the local
/// group and applies the atomic mutual-exclusion transition itself.
fn set_radio_checked(caller: &mut Caller<'_, RuntimeState>, tab_id: i64, node_id: i64) -> i32 {
    let (Ok(tab_id), Ok(node_id)) = (stable_id(tab_id), stable_id(node_id)) else {
        return RESULT_INVALID_ARGUMENT;
    };
    match request_core(
        caller,
        ExtensionRequest::SetRadioChecked { tab_id, node_id },
    ) {
        Ok(ExtensionReply::DomWriteAck) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

/// Selects one live option through the version-6 `dom:write` operation. The
/// guest supplies no select owner, peer list, or selected Boolean: core
/// validates the option and derives the single-select transition itself.
fn select_option(caller: &mut Caller<'_, RuntimeState>, tab_id: i64, node_id: i64) -> i32 {
    let (Ok(tab_id), Ok(node_id)) = (stable_id(tab_id), stable_id(node_id)) else {
        return RESULT_INVALID_ARGUMENT;
    };
    match request_core(caller, ExtensionRequest::SelectOption { tab_id, node_id }) {
        Ok(ExtensionReply::DomWriteAck) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

fn set_textarea_value(
    caller: &mut Caller<'_, RuntimeState>,
    tab_id: i64,
    node_id: i64,
    value_ptr: i32,
    value_len: i32,
) -> i32 {
    let (Ok(tab_id), Ok(node_id)) = (stable_id(tab_id), stable_id(node_id)) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok((value_ptr, value_len)) = guest_range(value_ptr, value_len, MAX_TEXT_WRITE_BYTES) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok(value) = read_guest_bytes(caller, value_ptr, value_len) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok(value) = String::from_utf8(value) else {
        return RESULT_INVALID_ARGUMENT;
    };
    match request_core(
        caller,
        ExtensionRequest::SetTextareaValue {
            tab_id,
            node_id,
            value,
        },
    ) {
        Ok(ExtensionReply::DomWriteAck) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

fn set_range_input_value(
    caller: &mut Caller<'_, RuntimeState>,
    tab_id: i64,
    node_id: i64,
    value: i64,
) -> i32 {
    let (Ok(tab_id), Ok(node_id)) = (stable_id(tab_id), stable_id(node_id)) else {
        return RESULT_INVALID_ARGUMENT;
    };
    match request_core(
        caller,
        ExtensionRequest::SetRangeInputValue {
            tab_id,
            node_id,
            value,
        },
    ) {
        Ok(ExtensionReply::DomWriteAck) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

/// Asks core to install one version-2 `network:intercept` declarative rule.
/// The guest receives no interception callback or ambient networking handle;
/// it can only submit one bounded UTF-8 URL for the host's normal capability
/// and gatekeeper review.
fn register_network_block_url(
    caller: &mut Caller<'_, RuntimeState>,
    url_ptr: i32,
    url_len: i32,
) -> i32 {
    let Ok((url_ptr, url_len)) = guest_range(url_ptr, url_len, MAX_NETWORK_BLOCK_URL_BYTES) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok(url) = read_guest_bytes(caller, url_ptr, url_len) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok(url) = String::from_utf8(url) else {
        return RESULT_INVALID_ARGUMENT;
    };
    match request_core(caller, ExtensionRequest::RegisterNetworkBlockUrl { url }) {
        Ok(ExtensionReply::NetworkInterceptAck) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

/// Removes only the caller connection's declarative URL, host, and path block rules. This
/// version-3 operation supplies no URL or other extension-controlled policy
/// input and therefore merely reduces the caller's own active rule set.
fn clear_network_block_urls(caller: &mut Caller<'_, RuntimeState>) -> i32 {
    match request_core(caller, ExtensionRequest::ClearNetworkBlockUrls) {
        Ok(ExtensionReply::NetworkInterceptAck) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

/// Registers one bounded version-4 host/subdomain rule through the ordinary
/// capability, gatekeeper, and core-owned validation path.
fn register_network_block_host(
    caller: &mut Caller<'_, RuntimeState>,
    host_ptr: i32,
    host_len: i32,
) -> i32 {
    let Ok((host_ptr, host_len)) = guest_range(host_ptr, host_len, MAX_NETWORK_BLOCK_HOST_BYTES)
    else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok(host) = read_guest_bytes(caller, host_ptr, host_len) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok(host) = String::from_utf8(host) else {
        return RESULT_INVALID_ARGUMENT;
    };
    match request_core(caller, ExtensionRequest::RegisterNetworkBlockHost { host }) {
        Ok(ExtensionReply::NetworkInterceptAck) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

/// Registers one version-5 literal host/path-prefix rule. Both guest ranges
/// are independently bounded before either payload reaches core.
fn register_network_block_path_prefix(
    caller: &mut Caller<'_, RuntimeState>,
    host_ptr: i32,
    host_len: i32,
    path_ptr: i32,
    path_len: i32,
) -> i32 {
    let Ok((host_ptr, host_len)) = guest_range(host_ptr, host_len, MAX_NETWORK_BLOCK_HOST_BYTES) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok((path_ptr, path_len)) = guest_range(path_ptr, path_len, MAX_NETWORK_BLOCK_PATH_BYTES) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok(host) = read_guest_bytes(caller, host_ptr, host_len).and_then(|bytes| String::from_utf8(bytes).map_err(|_| ())) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok(path_prefix) = read_guest_bytes(caller, path_ptr, path_len).and_then(|bytes| String::from_utf8(bytes).map_err(|_| ())) else {
        return RESULT_INVALID_ARGUMENT;
    };
    match request_core(caller, ExtensionRequest::RegisterNetworkBlockPathPrefix { host, path_prefix }) {
        Ok(ExtensionReply::NetworkInterceptAck) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

/// Copies the caller's bounded storage value into guest memory. A missing key
/// has its own stable result rather than being conflated with an empty value
/// or an authorization/network failure.
fn storage_get_utf8(
    caller: &mut Caller<'_, RuntimeState>,
    key_ptr: i32,
    key_len: i32,
    destination: i32,
    capacity: i32,
) -> i32 {
    storage_get(caller, key_ptr, key_len, destination, capacity, false)
}

fn storage_get(
    caller: &mut Caller<'_, RuntimeState>,
    key_ptr: i32,
    key_len: i32,
    destination: i32,
    capacity: i32,
    durable: bool,
) -> i32 {
    let Ok(key) = read_storage_key(caller, key_ptr, key_len) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok((destination, capacity)) = guest_range(destination, capacity, MAX_STORAGE_VALUE_BYTES)
    else {
        return RESULT_INVALID_ARGUMENT;
    };
    let request = if durable {
        ExtensionRequest::DurableStorageGet { key }
    } else {
        ExtensionRequest::StorageGet { key }
    };
    let value = match request_core(caller, request) {
        Ok(ExtensionReply::StorageGetResult { value: Some(value) }) => value,
        Ok(ExtensionReply::StorageGetResult { value: None }) => return RESULT_NOT_FOUND,
        Ok(_) | Err(()) => return RESULT_ERROR,
    };
    let bytes = value.as_bytes();
    if bytes.len() > MAX_STORAGE_VALUE_BYTES || bytes.len() > capacity {
        return RESULT_BUFFER_TOO_SMALL;
    }
    if write_guest_bytes(caller, destination, bytes).is_err() {
        return RESULT_INVALID_ARGUMENT;
    }
    i32::try_from(bytes.len()).unwrap_or(RESULT_ERROR)
}

/// Writes exactly one bounded key/value pair into the caller's isolated
/// storage bucket. The host still validates the key grammar and aggregate
/// quota after the request crosses the authenticated protocol boundary.
fn storage_set_utf8(
    caller: &mut Caller<'_, RuntimeState>,
    key_ptr: i32,
    key_len: i32,
    value_ptr: i32,
    value_len: i32,
) -> i32 {
    storage_set(caller, key_ptr, key_len, value_ptr, value_len, false)
}

fn storage_set(
    caller: &mut Caller<'_, RuntimeState>,
    key_ptr: i32,
    key_len: i32,
    value_ptr: i32,
    value_len: i32,
    durable: bool,
) -> i32 {
    let Ok(key) = read_storage_key(caller, key_ptr, key_len) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok((value_ptr, value_len)) = guest_range(value_ptr, value_len, MAX_STORAGE_VALUE_BYTES)
    else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok(value) = read_guest_bytes(caller, value_ptr, value_len) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok(value) = String::from_utf8(value) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let request = if durable {
        ExtensionRequest::DurableStorageSet { key, value }
    } else {
        ExtensionRequest::StorageSet { key, value }
    };
    match request_core(caller, request) {
        Ok(ExtensionReply::StorageSetAck) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

/// Removes one key from only the caller's isolated storage bucket. Returns
/// `1` when a value existed, `0` when it was already absent, and a negative
/// ABI error for malformed input or an unavailable core operation.
fn storage_remove_utf8(caller: &mut Caller<'_, RuntimeState>, key_ptr: i32, key_len: i32) -> i32 {
    storage_remove(caller, key_ptr, key_len, false)
}

fn storage_remove(
    caller: &mut Caller<'_, RuntimeState>,
    key_ptr: i32,
    key_len: i32,
    durable: bool,
) -> i32 {
    let Ok(key) = read_storage_key(caller, key_ptr, key_len) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let request = if durable {
        ExtensionRequest::DurableStorageRemove { key }
    } else {
        ExtensionRequest::StorageRemove { key }
    };
    match request_core(caller, request) {
        Ok(ExtensionReply::StorageRemoveAck { removed: true }) => 1,
        Ok(ExtensionReply::StorageRemoveAck { removed: false }) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

fn read_storage_key(
    caller: &mut Caller<'_, RuntimeState>,
    key_ptr: i32,
    key_len: i32,
) -> Result<String, ()> {
    let (key_ptr, key_len) = guest_range(key_ptr, key_len, MAX_STORAGE_KEY_BYTES)?;
    let key = read_guest_bytes(caller, key_ptr, key_len)?;
    String::from_utf8(key).map_err(|_| ())
}

fn stable_id(value: i64) -> Result<u64, ()> {
    u64::try_from(value).map_err(|_| ())
}

fn guest_range(pointer: i32, length: i32, maximum: usize) -> Result<(usize, usize), ()> {
    let pointer = usize::try_from(pointer).map_err(|_| ())?;
    let length = usize::try_from(length).map_err(|_| ())?;
    if length > maximum || pointer.checked_add(length).is_none() {
        return Err(());
    }
    Ok((pointer, length))
}

fn guest_memory(caller: &mut Caller<'_, RuntimeState>) -> Result<wasmtime::Memory, ()> {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or(())
}

fn read_guest_bytes(
    caller: &mut Caller<'_, RuntimeState>,
    pointer: usize,
    length: usize,
) -> Result<Vec<u8>, ()> {
    let memory = guest_memory(caller)?;
    let mut bytes = vec![0; length];
    memory.read(&*caller, pointer, &mut bytes).map_err(|_| ())?;
    Ok(bytes)
}

fn write_guest_bytes(
    caller: &mut Caller<'_, RuntimeState>,
    pointer: usize,
    bytes: &[u8],
) -> Result<(), ()> {
    let memory = guest_memory(caller)?;
    memory.write(&mut *caller, pointer, bytes).map_err(|_| ())
}

fn request_core(
    caller: &mut Caller<'_, RuntimeState>,
    request: ExtensionRequest,
) -> Result<ExtensionReply, ()> {
    let stream = &mut caller.data_mut().stream;
    write_extension_request(stream, &request).map_err(|_| ())?;
    read_extension_reply(stream).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load_installed_extension;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;

    fn installed_extension(label: &str, wasm: &str) -> (std::path::PathBuf, InstalledExtension) {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "blueice-extension-runtime-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("extension.json"),
            r#"{"name":"Runtime test","version":"1","blueice_api_version":1,"entry_point":"extension.wasm","capabilities":{"declared":["dom:read","dom:write","storage"]}}"#,
        )
        .unwrap();
        fs::write(root.join("extension.wasm"), wat::parse_str(wasm).unwrap()).unwrap();
        let installed = load_installed_extension(root.join("extension.json")).unwrap();
        (root, installed)
    }

    #[test]
    fn reactor_reads_bounded_network_response_metadata_without_ambient_network_access() {
        let (root, extension) = installed_extension(
            "network-observe",
            r#"(module
                (import "blueice" "network_response_utf8" (func $observe (param i64 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (func (export "blueice_start")
                    i64.const 7
                    i32.const 0
                    i32.const 512
                    call $observe
                    i32.const 0
                    i32.le_s
                    if unreachable end))"#,
        );
        let (guest, mut core) = UnixStream::pair().unwrap();
        let core_thread = thread::spawn(move || {
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::ReadNetworkResponse { tab_id: 7 }
            );
            blueice_ipc::extension::write_extension_reply(
                &mut core,
                &ExtensionReply::NetworkResponseResult {
                    response: Some(blueice_ipc::extension::NetworkResponseInfo {
                        method: "GET".to_string(),
                        final_url: "https://example.test/final".to_string(),
                        status: 200,
                        content_type: Some("text/html".to_string()),
                    }),
                },
            )
            .unwrap();
        });
        execute_installed_extension(&extension, guest).unwrap();
        core_thread.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reactor_reads_bounded_committed_network_trace_without_ambient_network_access() {
        let (root, extension) = installed_extension(
            "network-trace",
            r#"(module
                (import "blueice" "network_trace_utf8" (func $observe (param i64 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (func (export "blueice_start")
                    i64.const 7
                    i32.const 0
                    i32.const 1024
                    call $observe
                    i32.const 0
                    i32.le_s
                    if unreachable end))"#,
        );
        let (guest, mut core) = UnixStream::pair().unwrap();
        let core_thread = thread::spawn(move || {
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::ReadNetworkTrace { tab_id: 7 }
            );
            blueice_ipc::extension::write_extension_reply(
                &mut core,
                &ExtensionReply::NetworkTraceResult {
                    trace: Some(blueice_ipc::extension::NetworkTraceInfo {
                        request_url: "https://example.test/start".to_string(),
                        redirects: vec![blueice_ipc::extension::NetworkRedirectInfo {
                            request_url: "https://example.test/start".to_string(),
                            status: 302,
                            target_url: "https://example.test/final".to_string(),
                        }],
                        response: blueice_ipc::extension::NetworkResponseInfo {
                            method: "GET".to_string(),
                            final_url: "https://example.test/final".to_string(),
                            status: 200,
                            content_type: Some("text/html".to_string()),
                        },
                    }),
                },
            )
            .unwrap();
        });
        execute_installed_extension(&extension, guest).unwrap();
        core_thread.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn toolbar_activation_runs_a_fresh_guest_with_an_explicit_tab_and_bounded_ui_import() {
        let (root, extension) = installed_extension(
            "toolbar-activation",
            r#"(module
                (import "blueice" "runtime_event_kind" (func $kind (result i32)))
                (import "blueice" "runtime_event_tab_id" (func $tab (result i64)))
                (import "blueice" "set_toolbar_button_utf8" (func $toolbar (param i32 i32) (result i32)))
                (import "blueice" "clear_toolbar_button" (func $clear (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "Notes")
                (func (export "blueice_start")
                    call $kind
                    i32.const 2
                    i32.ne
                    if unreachable end
                    call $tab
                    i64.const 9
                    i64.ne
                    if unreachable end
                    i32.const 0
                    i32.const 5
                    call $toolbar
                    i32.const 0
                    i32.ne
                    if unreachable end
                    call $clear
                    i32.const 0
                    i32.ne
                    if unreachable end))"#,
        );
        let (guest, mut core) = UnixStream::pair().unwrap();
        let core_thread = thread::spawn(move || {
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::SetToolbarButton {
                    label: "Notes".to_string(),
                }
            );
            blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::UiInjectAck)
                .unwrap();
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::ClearToolbarButton
            );
            blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::UiInjectAck)
                .unwrap();
        });
        execute_installed_extension_for_invocation(
            &extension,
            guest,
            RuntimeInvocation::ToolbarActivated { tab_id: 9 },
        )
        .unwrap();
        core_thread.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn popup_import_forwards_bounded_text_and_clear_to_core() {
        let (root, extension) = installed_extension(
            "popup-activation",
            r#"(module
                (import "blueice" "show_popup_utf8" (func $show (param i64 i32 i32 i32 i32) (result i32)))
                (import "blueice" "clear_popup" (func $clear (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "Notes")
                (data (i32.const 16) "Saved locally")
                (func (export "blueice_start")
                    i64.const 9
                    i32.const 0
                    i32.const 5
                    i32.const 16
                    i32.const 13
                    call $show
                    i32.const 0
                    i32.ne
                    if unreachable end
                    call $clear
                    i32.const 0
                    i32.ne
                    if unreachable end))"#,
        );
        let (guest, mut core) = UnixStream::pair().unwrap();
        let core_thread = thread::spawn(move || {
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::ShowPopup {
                    tab_id: 9,
                    title: "Notes".to_string(),
                    body: "Saved locally".to_string(),
                }
            );
            blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::UiInjectAck)
                .unwrap();
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::ClearPopup
            );
            blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::UiInjectAck)
                .unwrap();
        });
        execute_installed_extension_for_invocation(
            &extension,
            guest,
            RuntimeInvocation::ToolbarActivated { tab_id: 9 },
        )
        .unwrap();
        core_thread.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn popup_action_import_forwards_label_and_receives_core_defined_activation() {
        let (root, extension) = installed_extension(
            "popup-action-activation",
            r#"(module
                (import "blueice" "runtime_event_kind" (func $kind (result i32)))
                (import "blueice" "runtime_event_tab_id" (func $tab (result i64)))
                (import "blueice" "show_popup_action_utf8" (func $show (param i64 i32 i32 i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "Notes")
                (data (i32.const 16) "Saved locally")
                (data (i32.const 48) "Open notes")
                (func (export "blueice_start")
                    call $kind
                    i32.const 3
                    i32.ne
                    if unreachable end
                    call $tab
                    i64.const 9
                    i64.ne
                    if unreachable end
                    i64.const 9
                    i32.const 0
                    i32.const 5
                    i32.const 16
                    i32.const 13
                    i32.const 48
                    i32.const 10
                    call $show
                    i32.const 0
                    i32.ne
                    if unreachable end))"#,
        );
        let (guest, mut core) = UnixStream::pair().unwrap();
        let core_thread = thread::spawn(move || {
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::ShowPopupAction {
                    tab_id: 9,
                    title: "Notes".to_string(),
                    body: "Saved locally".to_string(),
                    action_label: "Open notes".to_string(),
                }
            );
            blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::UiInjectAck)
                .unwrap();
        });
        execute_installed_extension_for_invocation(
            &extension,
            guest,
            RuntimeInvocation::PopupActionActivated { tab_id: 9 },
        )
        .unwrap();
        core_thread.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reactor_forwards_bounded_reads_and_form_writes_to_core() {
        let (root, extension) = installed_extension(
            "requests",
            r#"(module
                (import "blueice" "dom_read_utf8" (func $read (param i64 i32 i32) (result i32)))
                (import "blueice" "set_text_input_value" (func $text (param i64 i64 i32 i32) (result i32)))
                (import "blueice" "set_checkbox_checked" (func $checkbox (param i64 i64 i32) (result i32)))
                (import "blueice" "set_radio_checked" (func $radio (param i64 i64) (result i32)))
                (import "blueice" "select_option" (func $select (param i64 i64) (result i32)))
                (import "blueice" "set_textarea_value" (func $textarea (param i64 i64 i32 i32) (result i32)))
                (import "blueice" "set_range_input_value" (func $range (param i64 i64 i64) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "BlueIce")
                (func (export "blueice_start")
                    i64.const 7
                    i32.const 64
                    i32.const 128
                    call $read
                    drop
                    i64.const 7
                    i64.const 12
                    i32.const 0
                    i32.const 7
                    call $text
                    drop
                    i64.const 7
                    i64.const 13
                    i32.const 1
                    call $checkbox
                    drop
                    i64.const 7
                    i64.const 15
                    call $radio
                    drop
                    i64.const 7
                    i64.const 16
                    call $select
                    drop
                    i64.const 7
                    i64.const 14
                    i32.const 0
                    i32.const 7
                    call $textarea
                    drop
                    i64.const 7
                    i64.const 17
                    i64.const -3
                    call $range
                    drop))"#,
        );
        let (guest, mut core) = UnixStream::pair().unwrap();
        let core_thread = thread::spawn(move || {
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::DomReadTab { tab_id: 7 }
            );
            blueice_ipc::extension::write_extension_reply(
                &mut core,
                &ExtensionReply::DomReadResult {
                    value: r#"{"tab_id":7}"#.to_string(),
                },
            )
            .unwrap();
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::SetTextInputValue {
                    tab_id: 7,
                    node_id: 12,
                    value: "BlueIce".to_string(),
                }
            );
            blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::DomWriteAck)
                .unwrap();
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::SetCheckboxChecked {
                    tab_id: 7,
                    node_id: 13,
                    checked: true,
                }
            );
            blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::DomWriteAck)
                .unwrap();
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::SetRadioChecked {
                    tab_id: 7,
                    node_id: 15,
                }
            );
            blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::DomWriteAck)
                .unwrap();
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::SelectOption {
                    tab_id: 7,
                    node_id: 16,
                }
            );
            blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::DomWriteAck)
                .unwrap();
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::SetTextareaValue {
                    tab_id: 7,
                    node_id: 14,
                    value: "BlueIce".to_string(),
                }
            );
            blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::DomWriteAck)
                .unwrap();
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::SetRangeInputValue {
                    tab_id: 7,
                    node_id: 17,
                    value: -3,
                }
            );
            blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::DomWriteAck)
                .unwrap();
        });

        execute_installed_extension(&extension, guest).unwrap();
        core_thread.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reactor_forwards_a_bounded_declarative_navigation_block_url_to_core() {
        let url = "https://example.test/private";
        let (root, extension) = installed_extension(
            "network-block-rule",
            r#"(module
                (import "blueice" "register_network_block_url" (func $block (param i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "https://example.test/private")
                (func (export "blueice_start")
                    i32.const 0
                    i32.const 28
                    call $block
                    i32.const 0
                    i32.ne
                    if unreachable end))"#,
        );
        let (guest, mut core) = UnixStream::pair().unwrap();
        let core_thread = thread::spawn(move || {
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::RegisterNetworkBlockUrl {
                    url: url.to_string(),
                }
            );
            blueice_ipc::extension::write_extension_reply(
                &mut core,
                &ExtensionReply::NetworkInterceptAck,
            )
            .unwrap();
        });

        execute_installed_extension(&extension, guest).unwrap();
        core_thread.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reactor_forwards_a_bounded_host_block_rule_to_core() {
        let (root, extension) = installed_extension(
            "network-block-host",
            r#"(module
                (import "blueice" "register_network_block_host" (func $block (param i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "example.test")
                (func (export "blueice_start")
                    i32.const 0
                    i32.const 12
                    call $block
                    i32.const 0
                    i32.ne
                    if unreachable end))"#,
        );
        let (guest, mut core) = UnixStream::pair().unwrap();
        let core_thread = thread::spawn(move || {
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::RegisterNetworkBlockHost {
                    host: "example.test".to_string(),
                }
            );
            blueice_ipc::extension::write_extension_reply(
                &mut core,
                &ExtensionReply::NetworkInterceptAck,
            )
            .unwrap();
        });

        execute_installed_extension(&extension, guest).unwrap();
        core_thread.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reactor_forwards_a_bounded_path_prefix_rule_to_core() {
        let (root, extension) = installed_extension(
            "network-block-path-prefix",
            r#"(module
                (import "blueice" "register_network_block_path_prefix" (func $block (param i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "example.test")
                (data (i32.const 16) "/private")
                (func (export "blueice_start")
                    i32.const 0
                    i32.const 12
                    i32.const 16
                    i32.const 8
                    call $block
                    i32.const 0
                    i32.ne
                    if unreachable end))"#,
        );
        let (guest, mut core) = UnixStream::pair().unwrap();
        let core_thread = thread::spawn(move || {
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::RegisterNetworkBlockPathPrefix {
                    host: "example.test".into(), path_prefix: "/private".into(),
                }
            );
            blueice_ipc::extension::write_extension_reply(
                &mut core, &ExtensionReply::NetworkInterceptAck,
            ).unwrap();
        });
        execute_installed_extension(&extension, guest).unwrap();
        core_thread.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reactor_forwards_a_network_rule_clear_to_core() {
        let (root, extension) = installed_extension(
            "network-rule-clear",
            r#"(module
                (import "blueice" "clear_network_block_urls" (func $clear (result i32)))
                (func (export "blueice_start")
                    call $clear
                    i32.const 0
                    i32.ne
                    if unreachable end))"#,
        );
        let (guest, mut core) = UnixStream::pair().unwrap();
        let core_thread = thread::spawn(move || {
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::ClearNetworkBlockUrls
            );
            blueice_ipc::extension::write_extension_reply(
                &mut core,
                &ExtensionReply::NetworkInterceptAck,
            )
            .unwrap();
        });

        execute_installed_extension(&extension, guest).unwrap();
        core_thread.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reactor_forwards_bounded_storage_operations_and_copies_a_found_value() {
        let (root, extension) = installed_extension(
            "storage",
            r#"(module
                (import "blueice" "storage_get_utf8" (func $get (param i32 i32 i32 i32) (result i32)))
                (import "blueice" "storage_set_utf8" (func $set (param i32 i32 i32 i32) (result i32)))
                (import "blueice" "storage_remove_utf8" (func $remove (param i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "task-state")
                (data (i32.const 16) "complete")
                (func (export "blueice_start")
                    i32.const 0
                    i32.const 10
                    i32.const 16
                    i32.const 8
                    call $set
                    i32.const 0
                    i32.ne
                    if unreachable end
                    i32.const 0
                    i32.const 10
                    i32.const 64
                    i32.const 16
                    call $get
                    i32.const 8
                    i32.ne
                    if unreachable end
                    i32.const 64
                    i32.load8_u
                    i32.const 99
                    i32.ne
                    if unreachable end
                    i32.const 0
                    i32.const 10
                    call $remove
                    i32.const 1
                    i32.ne
                    if unreachable end
                    i32.const 0
                    i32.const 10
                    i32.const 64
                    i32.const 16
                    call $get
                    i32.const -4
                    i32.ne
                    if unreachable end))"#,
        );
        let (guest, mut core) = UnixStream::pair().unwrap();
        let core_thread = thread::spawn(move || {
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::StorageSet {
                    key: "task-state".to_string(),
                    value: "complete".to_string(),
                }
            );
            blueice_ipc::extension::write_extension_reply(
                &mut core,
                &ExtensionReply::StorageSetAck,
            )
            .unwrap();
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::StorageGet {
                    key: "task-state".to_string(),
                }
            );
            blueice_ipc::extension::write_extension_reply(
                &mut core,
                &ExtensionReply::StorageGetResult {
                    value: Some("complete".to_string()),
                },
            )
            .unwrap();
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::StorageRemove {
                    key: "task-state".to_string(),
                }
            );
            blueice_ipc::extension::write_extension_reply(
                &mut core,
                &ExtensionReply::StorageRemoveAck { removed: true },
            )
            .unwrap();
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::StorageGet {
                    key: "task-state".to_string(),
                }
            );
            blueice_ipc::extension::write_extension_reply(
                &mut core,
                &ExtensionReply::StorageGetResult { value: None },
            )
            .unwrap();
        });

        execute_installed_extension(&extension, guest).unwrap();
        core_thread.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reactor_forwards_separate_durable_storage_v2_imports() {
        let (root, extension) = installed_extension(
            "durable-storage",
            r#"(module
                (import "blueice" "durable_storage_set_utf8" (func $set (param i32 i32 i32 i32) (result i32)))
                (import "blueice" "durable_storage_get_utf8" (func $get (param i32 i32 i32 i32) (result i32)))
                (import "blueice" "durable_storage_remove_utf8" (func $remove (param i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "key")
                (data (i32.const 16) "value")
                (func (export "blueice_start")
                    i32.const 0 i32.const 3 i32.const 16 i32.const 5 call $set
                    i32.const 0 i32.ne if unreachable end
                    i32.const 0 i32.const 3 i32.const 64 i32.const 16 call $get
                    i32.const 5 i32.ne if unreachable end
                    i32.const 64 i32.load8_u i32.const 118 i32.ne if unreachable end
                    i32.const 0 i32.const 3 call $remove
                    i32.const 1 i32.ne if unreachable end))"#,
        );
        let (guest, mut core) = UnixStream::pair().unwrap();
        let core_thread = thread::spawn(move || {
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::DurableStorageSet {
                    key: "key".to_string(),
                    value: "value".to_string(),
                }
            );
            blueice_ipc::extension::write_extension_reply(&mut core, &ExtensionReply::StorageSetAck)
                .unwrap();
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::DurableStorageGet { key: "key".to_string() }
            );
            blueice_ipc::extension::write_extension_reply(
                &mut core,
                &ExtensionReply::StorageGetResult { value: Some("value".to_string()) },
            )
            .unwrap();
            assert_eq!(
                blueice_ipc::extension::read_extension_request(&mut core).unwrap(),
                ExtensionRequest::DurableStorageRemove { key: "key".to_string() }
            );
            blueice_ipc::extension::write_extension_reply(
                &mut core,
                &ExtensionReply::StorageRemoveAck { removed: true },
            )
            .unwrap();
        });

        execute_installed_extension(&extension, guest).unwrap();
        core_thread.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reactor_requires_the_fixed_entrypoint_and_terminates_a_fuel_exhausting_module() {
        let (missing_root, missing) = installed_extension("missing-entry", "(module)");
        let (guest, _) = UnixStream::pair().unwrap();
        assert!(execute_installed_extension(&missing, guest).is_err());
        let _ = fs::remove_dir_all(missing_root);

        let (loop_root, looping) = installed_extension(
            "fuel",
            r#"(module (func (export "blueice_start") (loop br 0)))"#,
        );
        let (guest, _) = UnixStream::pair().unwrap();
        assert!(execute_installed_extension(&looping, guest).is_err());
        let _ = fs::remove_dir_all(loop_root);
    }

    #[test]
    fn reactor_exposes_only_the_core_defined_navigation_event_context() {
        let (root, extension) = installed_extension(
            "event-context",
            r#"(module
                (import "blueice" "runtime_event_kind" (func $kind (result i32)))
                (import "blueice" "runtime_event_tab_id" (func $tab (result i64)))
                (func (export "blueice_start")
                    call $kind
                    i32.const 1
                    i32.ne
                    if unreachable end
                    call $tab
                    i64.const 77
                    i64.ne
                    if unreachable end))"#,
        );
        let (guest, _) = UnixStream::pair().unwrap();
        execute_installed_extension_for_invocation(
            &extension,
            guest,
            RuntimeInvocation::NavigationCommitted { tab_id: 77 },
        )
        .unwrap();
        let _ = fs::remove_dir_all(root);
    }
}
