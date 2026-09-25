// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Generation-lifetime regressions for direct static debugger metadata.

use super::*;

fn artifact() -> DirectScript {
    compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(
            ENTRY,
            "const answer: number = 42; answer;",
        )]),
        CompilerOptions::default(),
    )
    .unwrap()
}

#[test]
fn debug_metadata_fingerprint_binds_declared_global_call_policy() {
    let loader = MapLoader::from([ModuleSource::new(
        ENTRY,
        "const answer: number = 42; answer;",
    )]);
    let standalone = compile_direct_script(ENTRY, &loader, CompilerOptions::default()).unwrap();
    let direct_page = compile_direct_script(
        ENTRY,
        &loader,
        CompilerOptions {
            require_declared_global_calls: true,
            ..CompilerOptions::default()
        },
    )
    .unwrap();

    assert_ne!(
        standalone.compiler_options_fingerprint,
        direct_page.compiler_options_fingerprint
    );
    assert_ne!(
        standalone.debug_info.compiler_options_hash,
        direct_page.debug_info.compiler_options_hash
    );
}

#[test]
fn retains_only_static_metadata_for_one_live_direct_generation() {
    let artifact = artifact();
    let mut programs = bluejs::BlueJsProgramRegistry::default();
    let mut debug = DirectDebugRegistry::default();
    let attachment = artifact.attach_debug_in(&mut programs, &mut debug).unwrap();
    let retained = debug.get(&programs, attachment.handle).unwrap();

    assert_eq!(retained.handle(), attachment.handle);
    assert_eq!(
        retained.static_info().compiler_options_hash,
        artifact.compiler_options_fingerprint
    );
    assert_eq!(retained.static_info().sources.len(), 1);
    assert!(retained
        .static_info()
        .types
        .iter()
        .any(|static_type| static_type.display == "number"));
    assert_eq!(retained.safe_point_map(), &attachment.safe_point_map);
    for source_byte in [0, 1, 7, 26, usize::MAX] {
        assert_eq!(
            retained.breakpoint_at_or_after(ENTRY, source_byte),
            attachment.breakpoint_at_or_after(ENTRY, source_byte),
        );
    }
    assert_eq!(
        retained.breakpoint_at_or_after("page:///other.ts", 0),
        DirectSafePointBinding::Unbound,
    );

    assert!(programs.invalidate(attachment.handle));
    assert!(matches!(
        debug.get(&programs, attachment.handle),
        Err(DirectDebugAttachmentError::BlueJsProgram(
            bluejs::BlueJsProgramDebugError::UnknownProgram
        ))
    ));
    assert_eq!(debug.prune_invalid(&programs), 1);
    assert!(debug.is_empty());
}

#[test]
fn mismatched_metadata_fails_closed_and_invalidates_the_new_generation() {
    let mut artifact = artifact();
    artifact.debug_info.compiler_options_hash.push_str("-wrong");
    let mut programs = bluejs::BlueJsProgramRegistry::default();
    let mut debug = DirectDebugRegistry::default();
    let error = artifact
        .attach_debug_in(&mut programs, &mut debug)
        .unwrap_err();

    assert!(matches!(
        error,
        BridgeError::DebugAttachment(DirectDebugAttachmentError::CompilerOptionsMismatch)
    ));
    assert!(debug.is_empty());
}

#[test]
fn retention_limits_reject_a_program_before_metadata_is_exposed() {
    let artifact = artifact();
    let mut programs = bluejs::BlueJsProgramRegistry::default();
    let mut debug = DirectDebugRegistry::new(DirectDebugRetentionLimits {
        max_programs: 0,
        ..DirectDebugRetentionLimits::default()
    });
    let error = artifact
        .attach_debug_in(&mut programs, &mut debug)
        .unwrap_err();

    assert!(matches!(
        error,
        BridgeError::DebugAttachment(DirectDebugAttachmentError::RetentionLimit {
            resource: "programs",
            limit: 0,
        })
    ));
    assert!(debug.is_empty());
}

#[test]
fn lowering_span_retention_limit_rejects_before_source_breakpoints_are_available() {
    let artifact = artifact();
    let mut programs = bluejs::BlueJsProgramRegistry::default();
    let mut debug = DirectDebugRegistry::new(DirectDebugRetentionLimits {
        max_lowering_spans_per_program: 0,
        ..DirectDebugRetentionLimits::default()
    });

    assert!(matches!(
        artifact.attach_debug_in(&mut programs, &mut debug),
        Err(BridgeError::DebugAttachment(
            DirectDebugAttachmentError::RetentionLimit {
                resource: "lowering spans",
                limit: 0,
            }
        ))
    ));
    assert!(debug.is_empty());
}

#[test]
fn retained_source_breakpoints_cannot_disagree_with_the_verified_safe_point_map() {
    let artifact = artifact();
    let mut programs = bluejs::BlueJsProgramRegistry::default();
    let mut attachment = artifact.attach_in(&mut programs).unwrap();
    let original_binding = attachment.provenance[0].safe_point;
    assert!(matches!(original_binding, DirectSafePointBinding::Bound(_)));
    attachment.provenance[0].safe_point = DirectSafePointBinding::Unbound;
    let mut debug = DirectDebugRegistry::default();

    assert!(matches!(
        debug.retain(
            &programs,
            &attachment,
            &artifact.language_version,
            &artifact.compiler_options_fingerprint,
            &artifact.sources,
            &artifact.debug_info,
        ),
        Err(DirectDebugAttachmentError::SafePointMap(_))
    ));
    assert!(debug.is_empty());
}

#[test]
fn retained_source_breakpoints_preserve_unbound_spans_before_later_bound_code() {
    let mut artifact = artifact();
    let bluejs::BlueJsProgramV1::Script(program) = &mut artifact.program else {
        panic!("the fixture must compile as a classic script");
    };
    program.body[0] = bluejs::Stmt::Empty;
    artifact.bytecode = artifact.program.compile().unwrap();
    let mut programs = bluejs::BlueJsProgramRegistry::default();
    let mut debug = DirectDebugRegistry::default();
    let attachment = artifact.attach_debug_in(&mut programs, &mut debug).unwrap();
    assert_eq!(
        attachment.provenance[0].safe_point,
        DirectSafePointBinding::Unbound
    );
    let DirectSafePointBinding::Bound(later) = attachment.provenance[1].safe_point else {
        panic!("the later expression must still compile to an instruction");
    };
    let retained = debug.get(&programs, attachment.handle).unwrap();
    assert_eq!(
        retained.breakpoint_at_or_after(ENTRY, attachment.provenance[0].source.start),
        DirectSafePointBinding::Unbound,
        "an unbound span must not be skipped to the later instruction"
    );
    assert_eq!(
        retained.breakpoint_at_or_after(ENTRY, attachment.provenance[0].source.end),
        DirectSafePointBinding::Bound(later)
    );
}
