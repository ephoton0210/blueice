// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) struct Upload {
    pub(super) owner: u64,
    pub(super) expected: usize,
    mime_type: String,
    bytes: Vec<u8>,
}

impl TabManager {
    pub(super) fn ensure_object_url_environment(
        &mut self,
        tab: TabId,
    ) -> Result<(u64, StorageKey), String> {
        let context = self.tab_context(tab).ok_or("Unknown object URL creator")?;
        let store = self.object_urls.clone();
        let page = self.get_mut(tab).ok_or("Unknown object URL creator")?;
        if page.object_url_environment.is_none() {
            let id = NEXT_ENVIRONMENT
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .map_err(|_| "Object URL environment identities exhausted")?;
            page.object_url_environment = Some(DocumentEnvironment {
                id,
                storage_key: StorageKey {
                    context,
                    origin: page
                        .object_url_origin
                        .clone()
                        .unwrap_or_else(|| DocumentOrigin::from_url(page.url(), id)),
                },
                store: Arc::downgrade(&store),
            });
        }
        let environment = page
            .object_url_environment
            .as_ref()
            .expect("installed above");
        Ok((environment.id, environment.storage_key.clone()))
    }

    pub fn begin_object_url_upload(
        &mut self,
        tab: TabId,
        expected: usize,
        mime_type: &str,
    ) -> Result<u64, String> {
        if mime_type.len() > blueice_ipc::script::SCRIPT_MAX_NAME_BYTES {
            return Err("Object URL MIME type too large".into());
        }
        let (owner, _) = self.ensure_object_url_environment(tab)?;
        let entries = self.object_urls.entries();
        let mut uploads = self
            .object_urls
            .uploads
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let retained: usize = entries
            .values()
            .map(|entry| entry.resource.bytes.len())
            .sum();
        let reserved: usize = uploads.values().map(|upload| upload.expected).sum();
        if entries.len() + uploads.len() >= MAX_URLS
            || expected > MAX_BYTES.saturating_sub(retained + reserved)
        {
            return Err("Object URL storage limit reached".into());
        }
        let id = self
            .object_urls
            .next_upload
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| "Object URL upload identities exhausted")?;
        uploads.insert(
            id,
            Upload {
                owner,
                expected,
                mime_type: mime_type.into(),
                bytes: Vec::new(),
            },
        );
        Ok(id)
    }

    pub fn append_object_url_upload(
        &mut self,
        tab: TabId,
        id: u64,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), String> {
        let (owner, _) = self.ensure_object_url_environment(tab)?;
        if bytes.len() > blueice_ipc::script::SCRIPT_MAX_FILE_CHUNK_BYTES {
            return Err("Object URL upload chunk too large".into());
        }
        let mut uploads = self
            .object_urls
            .uploads
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let upload = uploads
            .get_mut(&id)
            .filter(|upload| upload.owner == owner)
            .ok_or("Object URL upload unavailable")?;
        if offset != upload.bytes.len() || bytes.len() > upload.expected.saturating_sub(offset) {
            return Err("Invalid object URL upload range".into());
        }
        upload
            .bytes
            .try_reserve(bytes.len())
            .map_err(|_| "Object URL upload allocation failed")?;
        upload.bytes.extend_from_slice(bytes);
        Ok(())
    }

    pub fn commit_object_url_upload(&mut self, tab: TabId, id: u64) -> Result<String, String> {
        let (owner, _) = self.ensure_object_url_environment(tab)?;
        let upload = {
            let mut uploads = self
                .object_urls
                .uploads
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let upload = uploads
                .get(&id)
                .filter(|upload| upload.owner == owner)
                .ok_or("Object URL upload unavailable")?;
            if upload.bytes.len() != upload.expected {
                return Err("Object URL upload incomplete".into());
            }
            uploads.remove(&id).expect("checked upload")
        };
        self.create_object_url(tab, upload.bytes, &upload.mime_type)
    }

    pub fn abort_object_url_upload(&mut self, tab: TabId, id: u64) {
        let Some(environment) = self
            .get(tab)
            .and_then(|page| page.object_url_environment.as_ref())
        else {
            return;
        };
        let mut uploads = self
            .object_urls
            .uploads
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if uploads
            .get(&id)
            .is_some_and(|upload| upload.owner == environment.id)
        {
            uploads.remove(&id);
        }
    }
}
