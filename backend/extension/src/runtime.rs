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
    MAX_NETWORK_BLOCK_URL_BYTES, MAX_STORAGE_KEY_BYTES, MAX_STORAGE_VALUE_BYTES,
    MAX_TEXT_WRITE_BYTES, MAX_NETWORK_OBSERVATION_BYTES,
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
/// integer values exposed through the ABI are stable: `0` is startup and `1`
/// is a successfully committed navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeInvocation {
    Startup,
    NavigationCommitted { tab_id: u64 },
}

impl RuntimeInvocation {
    fn kind(self) -> i32 {
        match self {
            Self::Startup => 0,
            Self::NavigationCommitted { .. } => 1,
        }
    }

    fn tab_id(self) -> i64 {
        match self {
            Self::Startup => -1,
            Self::NavigationCommitted { tab_id } => i64::try_from(tab_id).unwrap_or(-1),
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

/// Removes only the caller connection's version-2 exact block rules. This
/// version-3 operation supplies no URL or other extension-controlled policy
/// input and therefore merely reduces the caller's own active rule set.
fn clear_network_block_urls(caller: &mut Caller<'_, RuntimeState>) -> i32 {
    match request_core(caller, ExtensionRequest::ClearNetworkBlockUrls) {
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
    let Ok(key) = read_storage_key(caller, key_ptr, key_len) else {
        return RESULT_INVALID_ARGUMENT;
    };
    let Ok((destination, capacity)) = guest_range(destination, capacity, MAX_STORAGE_VALUE_BYTES)
    else {
        return RESULT_INVALID_ARGUMENT;
    };
    let value = match request_core(caller, ExtensionRequest::StorageGet { key }) {
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
    match request_core(caller, ExtensionRequest::StorageSet { key, value }) {
        Ok(ExtensionReply::StorageSetAck) => RESULT_OK,
        Ok(_) | Err(()) => RESULT_ERROR,
    }
}

/// Removes one key from only the caller's isolated storage bucket. Returns
/// `1` when a value existed, `0` when it was already absent, and a negative
/// ABI error for malformed input or an unavailable core operation.
fn storage_remove_utf8(caller: &mut Caller<'_, RuntimeState>, key_ptr: i32, key_len: i32) -> i32 {
    let Ok(key) = read_storage_key(caller, key_ptr, key_len) else {
        return RESULT_INVALID_ARGUMENT;
    };
    match request_core(caller, ExtensionRequest::StorageRemove { key }) {
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
