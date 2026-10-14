// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core-owned Blob URLs. A request retains immutable data independently of
//! later revocation, while document lifetime and storage keys fence admission.

use crate::{BrowserContextId, TabId, TabManager};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use url::{Origin, Url};

mod uploads;

static NEXT_ENVIRONMENT: AtomicU64 = AtomicU64::new(1);
const MAX_URLS: usize = 4096;
const MAX_BYTES: usize = 256 * 1024 * 1024;

/// Fetches require the creator's storage key. Top-level navigation follows
/// the File API's navigation exception; subresource loads must use `Fetch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectUrlPurpose {
    Fetch,
    Navigation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DocumentOrigin {
    Tuple(String),
    Opaque(u64),
}
impl DocumentOrigin {
    fn from_url(url: Option<&str>, environment: u64) -> Self {
        match url
            .and_then(|url| Url::parse(url).ok())
            .map(|url| url.origin())
        {
            Some(origin @ Origin::Tuple(..)) => Self::Tuple(origin.ascii_serialization()),
            _ => Self::Opaque(environment),
        }
    }
    fn serialize(&self) -> &str {
        match self {
            Self::Tuple(origin) => origin,
            Self::Opaque(_) => "null",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StorageKey {
    context: BrowserContextId,
    origin: DocumentOrigin,
}

#[derive(Debug)]
struct BlobResource {
    bytes: Vec<u8>,
    mime_type: String,
    creator_origin: DocumentOrigin,
}

/// An already admitted request. Its immutable bytes survive revocation or
/// destruction of the creator document; later requests must resolve again.
#[derive(Clone, Debug)]
pub struct ObjectUrlLease(Arc<BlobResource>);
impl ObjectUrlLease {
    pub fn bytes(&self) -> &[u8] {
        &self.0.bytes
    }
    pub fn mime_type(&self) -> &str {
        &self.0.mime_type
    }
    pub fn creator_origin(&self) -> &str {
        self.0.creator_origin.serialize()
    }
    pub(crate) fn origin_identity(&self) -> DocumentOrigin {
        self.0.creator_origin.clone()
    }
}

struct Entry {
    owner: u64,
    storage_key: StorageKey,
    resource: Arc<BlobResource>,
}
#[derive(Default)]
pub(crate) struct ObjectUrlStore {
    entries: Mutex<HashMap<String, Entry>>,
    uploads: Mutex<HashMap<u64, uploads::Upload>>,
    next_upload: AtomicU64,
}
impl ObjectUrlStore {
    fn entries(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Owned by one live Page, never by a history snapshot or a JS wrapper.
/// Dropping it retires every URL minted by that exact document.
pub(crate) struct DocumentEnvironment {
    id: u64,
    storage_key: StorageKey,
    store: Weak<ObjectUrlStore>,
}
impl Drop for DocumentEnvironment {
    fn drop(&mut self) {
        if let Some(store) = self.store.upgrade() {
            store.entries().retain(|_, entry| entry.owner != self.id);
            store
                .uploads
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .retain(|_, upload| upload.owner != self.id);
        }
    }
}

fn lookup_key(value: &str) -> Option<String> {
    let mut url = Url::parse(value).ok()?;
    if url.scheme() != "blob" {
        return None;
    }
    url.set_fragment(None);
    Some(url.to_string())
}

impl TabManager {
    fn object_url_storage_key(&self, tab: TabId) -> Option<StorageKey> {
        let page = self.get(tab)?;
        let context = self.tab_context(tab)?;
        match &page.object_url_environment {
            Some(environment) => Some(environment.storage_key.clone()),
            None => match page
                .object_url_origin
                .clone()
                .unwrap_or_else(|| DocumentOrigin::from_url(page.url(), 0))
            {
                origin @ DocumentOrigin::Tuple(_) => Some(StorageKey { context, origin }),
                DocumentOrigin::Opaque(_) => None,
            },
        }
    }

    /// Retains a Blob's bytes under an unguessable UUID URL. The caller must
    /// supply the live tab identity; script IPC additionally checks its realm.
    pub fn create_object_url(
        &mut self,
        tab: TabId,
        bytes: Vec<u8>,
        mime_type: &str,
    ) -> Result<String, String> {
        if mime_type.len() > blueice_ipc::script::SCRIPT_MAX_NAME_BYTES {
            return Err("Object URL MIME type too large".into());
        }
        let (owner, storage_key) = self.ensure_object_url_environment(tab)?;
        let store = self.object_urls.clone();
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|error| format!("Object URL randomness: {error}"))?;
        random[6] = (random[6] & 0x0f) | 0x40;
        random[8] = (random[8] & 0x3f) | 0x80;
        let hex: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let uuid = format!(
            "{}-{}-{}-{}-{}",
            &hex[..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..]
        );
        let url = format!("blob:{}/{}", storage_key.origin.serialize(), uuid);
        let mime_type = if mime_type.bytes().all(|byte| (0x20..=0x7e).contains(&byte)) {
            mime_type.to_ascii_lowercase()
        } else {
            String::new()
        };
        let mut entries = store.entries();
        let retained: usize = entries
            .values()
            .map(|entry| entry.resource.bytes.len())
            .sum();
        let uploads = store
            .uploads
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let reserved: usize = uploads.values().map(|upload| upload.expected).sum();
        if entries.len() + uploads.len() >= MAX_URLS
            || bytes.len() > MAX_BYTES.saturating_sub(retained + reserved)
        {
            return Err("Object URL storage limit reached".into());
        }
        if entries.contains_key(&url) {
            return Err("Object URL identity collision".into());
        }
        entries.insert(
            url.clone(),
            Entry {
                owner,
                storage_key: storage_key.clone(),
                resource: Arc::new(BlobResource {
                    bytes,
                    mime_type,
                    creator_origin: storage_key.origin.clone(),
                }),
            },
        );
        Ok(url)
    }

    pub fn resolve_object_url(
        &self,
        requester: TabId,
        value: &str,
        purpose: ObjectUrlPurpose,
    ) -> Option<ObjectUrlLease> {
        self.get(requester)?;
        let key = lookup_key(value)?;
        let storage_key = self.object_url_storage_key(requester);
        let entries = self.object_urls.entries();
        let entry = entries.get(&key)?;
        if purpose == ObjectUrlPurpose::Fetch && storage_key.as_ref() != Some(&entry.storage_key) {
            return None;
        }
        Some(ObjectUrlLease(entry.resource.clone()))
    }

    /// Unknown, non-Blob and foreign-storage URLs are silent no-ops.
    pub fn revoke_object_url(&mut self, requester: TabId, value: &str) {
        let Some(storage_key) = self.object_url_storage_key(requester) else {
            return;
        };
        let Some(key) = lookup_key(value) else { return };
        let mut entries = self.object_urls.entries();
        if entries
            .get(&key)
            .is_some_and(|entry| entry.storage_key == storage_key)
        {
            entries.remove(&key);
        }
    }
}
