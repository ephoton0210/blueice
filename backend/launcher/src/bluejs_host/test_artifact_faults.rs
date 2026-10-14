// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Debug-build-only state substitutions for real child-process regressions.
//! No page-host or public debugger request can call these methods.

use super::*;

impl BlueJsChildHost {
    fn test_live_map(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> &mut BoundEmittedMap {
        let document = self.documents.get_mut(&tab_id).expect("live test realm");
        assert_eq!(document.generation, document_generation);
        document
            .emitted_maps
            .get_mut(&program)
            .expect("live test emitted map")
    }

    /// Removes the actual map admitted for a first-generation program and
    /// retains it across a later document replacement in this test child.
    #[doc(hidden)]
    pub fn capture_retained_emitted_map_for_test(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) {
        let document = self.documents.get_mut(&tab_id).expect("live test realm");
        assert_eq!(document.generation, document_generation);
        let old = document
            .emitted_maps
            .remove(&program)
            .expect("live first-generation map");
        assert_eq!(old.admitted_for, Some((document_generation, program)));
        assert!(self.saved_emitted_map_for_test.replace(old).is_none());
    }

    /// Substitutes the exact saved map, even when a new compiler emission has
    /// identical generated JavaScript bytes and therefore the same hash.
    #[doc(hidden)]
    pub fn substitute_prior_generation_emitted_map_for_test(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) {
        let old = self
            .saved_emitted_map_for_test
            .take()
            .expect("saved prior-generation map");
        let document = self.documents.get_mut(&tab_id).expect("new test realm");
        assert_eq!(document.generation, document_generation);
        let program = *document
            .emitted_maps
            .keys()
            .next()
            .expect("new emitted map");
        let new = document.emitted_maps.get(&program).unwrap();
        assert_eq!(old.generated.source_hash, new.generated.source_hash);
        assert_ne!(old.admitted_for, Some((document_generation, program)));
        document.emitted_maps.insert(program, old);
    }

    /// Replaces the retained artifact with an independently compiled map and
    /// JavaScript whose hash differs from the installed program.
    #[doc(hidden)]
    pub fn substitute_other_javascript_emitted_map_for_test(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        javascript: String,
        map: SourceMap,
    ) {
        assert!(map
            .original_position_at(
                &javascript,
                0,
                0,
                BoundEmittedMap::MAX_SEGMENTS,
                BoundEmittedMap::MAX_RETAINED_BYTES,
            )
            .is_ok());
        let bound = self.test_live_map(tab_id, document_generation, program);
        let other_hash = page_host::source_hash(&javascript);
        assert_ne!(bound.generated.source_hash, other_hash);
        bound.generated.source = javascript;
        bound.generated.source_hash = other_hash;
        bound.map = map;
    }

    /// Models a caller-asserted map that bypassed the compiler admission
    /// step. Its content may be valid, but it has no live-program seal.
    #[doc(hidden)]
    pub fn substitute_unadmitted_emitted_map_for_test(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        map: SourceMap,
    ) {
        let bound = self.test_live_map(tab_id, document_generation, program);
        assert!(map
            .original_position_at(
                &bound.generated.source,
                0,
                0,
                BoundEmittedMap::MAX_SEGMENTS,
                BoundEmittedMap::MAX_RETAINED_BYTES,
            )
            .is_ok());
        bound.map = map;
        bound.admitted_for = None;
    }
}
