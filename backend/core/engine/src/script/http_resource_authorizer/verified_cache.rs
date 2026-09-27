// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Exact-document verified HTTP source cache with a shared source-byte budget.

use crate::TabId;
use std::collections::BTreeMap;

/// Bounds key/map overhead even when verified source bodies are empty.
pub(super) const MAX_VERIFIED_SOURCE_CACHE_ENTRIES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ResourceCacheKey {
    pub(super) tab_id: TabId,
    pub(super) document_generation: u64,
    pub(super) canonical_url: String,
    pub(super) expected_integrity: String,
    pub(super) mime_lane: &'static str,
}

#[derive(Debug, Clone)]
struct CachedResource {
    source: String,
    last_used: u64,
}

/// Eviction forces a fresh response and SHA-256 check before readmission.
/// Only retained source payload is charged; key and allocator overhead are
/// separate from this bounded cache budget.
pub(super) struct VerifiedResourceCache {
    entries: BTreeMap<ResourceCacheKey, CachedResource>,
    pub(super) source_bytes: usize,
    max_source_bytes: usize,
    clock: u64,
}

impl VerifiedResourceCache {
    pub(super) fn new(max_source_bytes: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            source_bytes: 0,
            max_source_bytes,
            clock: 0,
        }
    }

    pub(super) fn release_document(&mut self, tab_id: TabId, document_generation: u64) {
        self.entries.retain(|key, resource| {
            if key.tab_id == tab_id && key.document_generation == document_generation {
                self.source_bytes -= resource.source.len();
                false
            } else {
                true
            }
        });
    }

    pub(super) fn retained_source_payload_bytes(
        &self,
        tab_id: TabId,
        document_generation: u64,
    ) -> Option<usize> {
        self.entries
            .iter()
            .try_fold(0usize, |total, (key, resource)| {
                if key.tab_id == tab_id && key.document_generation == document_generation {
                    total.checked_add(resource.source.len())
                } else {
                    Some(total)
                }
            })
    }

    pub(super) fn get(&mut self, key: &ResourceCacheKey) -> Option<String> {
        let entry = self.entries.get_mut(key)?;
        self.clock = self.clock.saturating_add(1);
        entry.last_used = self.clock;
        Some(entry.source.clone())
    }

    pub(super) fn insert_verified(&mut self, key: ResourceCacheKey, source: String) {
        let bytes = source.len();
        if bytes > self.max_source_bytes {
            return;
        }
        if let Some(old) = self.entries.remove(&key) {
            self.source_bytes -= old.source.len();
        }
        while self.source_bytes.saturating_add(bytes) > self.max_source_bytes
            || self.entries.len() >= MAX_VERIFIED_SOURCE_CACHE_ENTRIES
        {
            let victim = self
                .entries
                .iter()
                .min_by_key(|(key, resource)| (resource.last_used, *key))
                .map(|(key, _)| key.clone())
                .expect("a nonempty cache must have a victim above its source budget");
            let evicted = self
                .entries
                .remove(&victim)
                .expect("selected cache victim exists");
            self.source_bytes -= evicted.source.len();
        }
        self.clock = self.clock.saturating_add(1);
        self.source_bytes += bytes;
        self.entries.insert(
            key,
            CachedResource {
                source,
                last_used: self.clock,
            },
        );
    }
}
