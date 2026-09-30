// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Live iteration over `Map` and `Set` entries, and `clear`.
//!
//! A collection's entries are an insertion-ordered list in which a deletion
//! leaves a tombstone (`None`) instead of shifting later entries, and `clear`
//! empties every slot without shortening the list. An iterator (or a
//! `forEach` loop) therefore holds a plain list position that stays valid
//! while the collection changes: entries appended later are reached, entries
//! deleted before they are reached are skipped, and a deleted-then-re-added
//! key is visited again at its new, later position -- exactly the observable
//! behaviour ECMA-262 specifies for the `[[MapData]]`/`[[SetData]]` lists.

use super::*;

/// One position of a collection's entry list.
pub(crate) enum CollectionEntry {
    /// A live entry: its key and value (a Set stores `undefined` as the value).
    Present(Value, Value),
    /// A tombstone left by a deletion or `clear`: skipped, but still a valid
    /// position to step past.
    Deleted,
    /// Past the last position: nothing more will be visited, *unless* the
    /// collection later grows (which a `forEach` loop, unlike a finished
    /// iterator, will then observe).
    End,
}

impl Heap {
    /// Allocates a Map/Set iterator over `collection` (`map` says which kind),
    /// producing keys, values or `[key, value]` pairs per `kind`.
    pub(crate) fn alloc_collection_iterator(
        &mut self,
        collection: ObjectId,
        map: bool,
        kind: ArrayIteratorKind,
        prototype: ObjectId,
    ) -> Result<ObjectId, HeapError> {
        self.alloc(
            ObjectKind::CollectionIterator {
                collection: Some(collection),
                index: 0,
                kind,
                map,
            },
            Some(prototype),
        )
    }

    /// The entry at `index` of a Map's or Set's list.
    pub(crate) fn collection_entry_at(
        &self,
        collection: ObjectId,
        index: usize,
    ) -> Result<CollectionEntry, HeapError> {
        let entries = match &self.object(collection)?.kind {
            ObjectKind::Map { entries } | ObjectKind::Set { entries } => entries,
            _ => return Err(HeapError::InvalidInternalSlot(collection)),
        };
        Ok(match entries.entries.get(index) {
            None => CollectionEntry::End,
            Some(None) => CollectionEntry::Deleted,
            Some(Some((key, value))) => CollectionEntry::Present(key.clone(), value.clone()),
        })
    }

    /// Steps the Map (`map`) or Set iterator `id` to its next live entry.
    ///
    /// The outer `None` means `id` is not an iterator of that kind (a brand
    /// failure for the caller to report); `Some(None)` means it is exhausted
    /// -- and it stays exhausted from then on, even if entries arrive later;
    /// `Some(Some((key, value, kind)))` is the next entry.
    #[allow(clippy::type_complexity)]
    pub(crate) fn collection_iterator_next(
        &mut self,
        id: ObjectId,
        map: bool,
    ) -> Result<Option<Option<(Value, Value, ArrayIteratorKind)>>, HeapError> {
        let (collection, mut index, kind) = match &self.object(id)?.kind {
            ObjectKind::CollectionIterator {
                collection,
                index,
                kind,
                map: iterator_map,
            } if *iterator_map == map => (*collection, *index, *kind),
            _ => return Ok(None),
        };
        let Some(collection) = collection else {
            return Ok(Some(None));
        };
        let step = loop {
            match self.collection_entry_at(collection, index)? {
                CollectionEntry::Present(key, value) => {
                    index += 1;
                    break Some((key, value, kind));
                }
                CollectionEntry::Deleted => index += 1,
                CollectionEntry::End => break None,
            }
        };
        // Entry lookup above established the iterator brand. Reading the
        // collection cannot change that object before this update.
        self.set_collection_iterator_progress(id, index, step.is_none())
            .expect("validated collection iterator remains live");
        Ok(Some(step))
    }

    pub(super) fn set_collection_iterator_progress(
        &mut self,
        id: ObjectId,
        index: usize,
        finished: bool,
    ) -> Result<(), HeapError> {
        let entry = self
            .objects
            .get_mut(&id)
            .ok_or(HeapError::InvalidObject(id))?;
        let ObjectKind::CollectionIterator {
            collection: iterated,
            index: position,
            ..
        } = &mut entry.kind
        else {
            return Err(HeapError::InvalidInternalSlot(id));
        };
        *position = index;
        if finished {
            *iterated = None;
        }
        Ok(())
    }

    /// `Map.prototype.clear` / `Set.prototype.clear`: every entry becomes a
    /// tombstone, and the byte accounting drops what the entries held. The
    /// list keeps its length so live iterators keep valid positions; entries
    /// added afterwards land past them and are visited.
    pub(crate) fn collection_clear(&mut self, object: ObjectId) -> Result<(), HeapError> {
        let entry = self
            .objects
            .get_mut(&object)
            .ok_or(HeapError::InvalidObject(object))?;
        let entries = match &mut entry.kind {
            ObjectKind::Map { entries } | ObjectKind::Set { entries } => entries,
            _ => return Err(HeapError::InvalidInternalSlot(object)),
        };
        let mut released = 0;
        for slot in entries.entries.iter_mut() {
            if let Some((key, value)) = slot.take() {
                released += map_entry_bytes(&key, &value);
            }
        }
        entries.indexes.clear();
        entries.len = 0;
        entry.bytes -= released;
        self.managed_bytes -= released;
        Ok(())
    }
}
