// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn handle<S, R, W, N, B, C>(
    context: RequestContext<'_>,
    stream: &mut S,
    request: ExtensionRequest,
    delegates: &mut ExtensionActionDelegates<R, W, N, B, C>,
    storage_generation: Option<u64>,
) -> io::Result<()>
where
    S: Read + Write,
    R: FnMut(Option<u64>) -> Result<String, String>,
    W: FnMut(
        Option<(u64, u64)>,
        String,
        &blueice_ipc::extension::DomWriteTarget,
        u64,
    ) -> Result<(), String>,
    N: FnMut() -> Result<(), String>,
    B: FnMut(String, u64) -> Result<(), String>,
    C: FnMut() -> Result<(), String>,
{
    let RequestContext {
        registry, identity, ..
    } = context;
    let ExtensionActionDelegates { storage, .. } = delegates;
    match request {
        ExtensionRequest::StorageGet { key } => {
            if let Some(reason) =
                capability_denial_reason(registry, identity, CAPABILITY_STORAGE, 1)
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_STORAGE.to_string(),
                        reason,
                    },
                )?;
                return Ok(());
            }
            let reply =
                match with_stable_storage_grant(registry, identity, storage_generation, || {
                    storage.get(&identity.extension_id, &key)
                }) {
                    Ok(value) => ExtensionReply::StorageGetResult { value },
                    Err(reply) => reply,
                };
            write_extension_reply(stream, &reply)?;
        }
        ExtensionRequest::StorageSet { key, value } => {
            if let Some(reason) =
                capability_denial_reason(registry, identity, CAPABILITY_STORAGE, 1)
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_STORAGE.to_string(),
                        reason,
                    },
                )?;
                return Ok(());
            }
            let reply =
                match with_stable_storage_grant(registry, identity, storage_generation, || {
                    storage.set(&identity.extension_id, key, value)
                }) {
                    Ok(()) => ExtensionReply::StorageSetAck,
                    Err(reply) => reply,
                };
            write_extension_reply(stream, &reply)?;
        }
        ExtensionRequest::StorageRemove { key } => {
            if let Some(reason) =
                capability_denial_reason(registry, identity, CAPABILITY_STORAGE, 1)
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_STORAGE.to_string(),
                        reason,
                    },
                )?;
                return Ok(());
            }
            let reply =
                match with_stable_storage_grant(registry, identity, storage_generation, || {
                    storage.remove(&identity.extension_id, &key)
                }) {
                    Ok(removed) => ExtensionReply::StorageRemoveAck { removed },
                    Err(reply) => reply,
                };
            write_extension_reply(stream, &reply)?;
        }
        ExtensionRequest::DurableStorageGet { key } => {
            if let Some(reason) =
                capability_denial_reason(registry, identity, CAPABILITY_STORAGE, 2)
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_STORAGE.to_string(),
                        reason,
                    },
                )?;
                return Ok(());
            }
            let reply =
                match with_stable_storage_grant(registry, identity, storage_generation, || {
                    storage.durable_get(&identity.extension_id, &key)
                }) {
                    Ok(value) => ExtensionReply::StorageGetResult { value },
                    Err(reply) => reply,
                };
            write_extension_reply(stream, &reply)?;
        }
        ExtensionRequest::DurableStorageSet { key, value } => {
            if let Some(reason) =
                capability_denial_reason(registry, identity, CAPABILITY_STORAGE, 2)
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_STORAGE.to_string(),
                        reason,
                    },
                )?;
                return Ok(());
            }
            let reply =
                match with_stable_storage_grant(registry, identity, storage_generation, || {
                    storage.durable_set(&identity.extension_id, key, value)
                }) {
                    Ok(()) => ExtensionReply::StorageSetAck,
                    Err(reply) => reply,
                };
            write_extension_reply(stream, &reply)?;
        }
        ExtensionRequest::DurableStorageRemove { key } => {
            if let Some(reason) =
                capability_denial_reason(registry, identity, CAPABILITY_STORAGE, 2)
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_STORAGE.to_string(),
                        reason,
                    },
                )?;
                return Ok(());
            }
            let reply =
                match with_stable_storage_grant(registry, identity, storage_generation, || {
                    storage.durable_remove(&identity.extension_id, &key)
                }) {
                    Ok(removed) => ExtensionReply::StorageRemoveAck { removed },
                    Err(reply) => reply,
                };
            write_extension_reply(stream, &reply)?;
        }
        ExtensionRequest::DurableStorageListKeys => {
            if let Some(reason) =
                capability_denial_reason(registry, identity, CAPABILITY_STORAGE, 3)
            {
                write_extension_reply(
                    stream,
                    &ExtensionReply::CapabilityDenied {
                        capability: CAPABILITY_STORAGE.to_string(),
                        reason,
                    },
                )?;
                return Ok(());
            }
            let reply =
                match with_stable_storage_grant(registry, identity, storage_generation, || {
                    storage.durable_list_keys(&identity.extension_id)
                }) {
                    Ok(keys) => ExtensionReply::StorageKeysResult { keys },
                    Err(reply) => reply,
                };
            write_extension_reply(stream, &reply)?;
        }
        _ => unreachable!("request belongs to another extension domain"),
    }
    Ok(())
}
