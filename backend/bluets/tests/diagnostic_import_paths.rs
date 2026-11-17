// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Import type presentation normalizes only equivalent forms of its owned root.

use std::path::Path;

#[path = "oracle_support/diagnostic_import_paths.rs"]
mod paths;

#[test]
fn a_verbatim_drive_message_uses_the_same_known_root() {
    assert_eq!(
        paths::relative_message(
            "Property 'missing' does not exist on type 'typeof import(\"//?/C:/Temp/owned/base\")'.",
            Path::new(r"\\?\C:\Temp\owned")
        ),
        "Property 'missing' does not exist on type 'typeof import(\"base\")'."
    );
}

#[test]
fn an_ordinary_drive_message_matches_a_verbatim_root() {
    assert_eq!(
        paths::relative_message(
            "typeof import(\"C:/Temp/owned/base\")",
            Path::new(r"\\?\C:\Temp\owned")
        ),
        "typeof import(\"base\")"
    );
}

#[test]
fn a_verbatim_drive_message_matches_an_ordinary_root() {
    assert_eq!(
        paths::relative_message(
            "typeof import(\"//?/C:/Temp/owned/base\")",
            Path::new(r"C:\Temp\owned")
        ),
        "typeof import(\"base\")"
    );
}

#[test]
fn unc_messages_preserve_their_owned_share_identity() {
    for message in [
        "typeof import(\"//server/share/owned/base\")",
        "typeof import(\"//?/UNC/server/share/owned/base\")",
    ] {
        assert_eq!(
            paths::relative_message(message, Path::new(r"\\?\UNC\server\share\owned")),
            "typeof import(\"base\")"
        );
    }
}

#[test]
fn foreign_roots_and_sibling_prefixes_remain_byte_exact() {
    for message in [
        "typeof import(\"//?/C:/Temp/owned-other/base\")",
        "typeof import(\"//?/C:/Elsewhere/owned/base\")",
        "typeof import(\"//?/UNC/elsewhere/share/owned/base\")",
        "Source '//?/C:/Temp/owned/base' names a missing member",
    ] {
        assert_eq!(
            paths::relative_message(message, Path::new(r"\\?\C:\Temp\owned")),
            message
        );
    }
}

#[test]
fn a_posix_import_keeps_every_member_and_related_message_suffix() {
    let message = "Property 'missing' does not exist on type 'typeof import(\"/tmp/owned/base\")'. See 'base'.";
    assert_eq!(
        paths::relative_message(message, Path::new("/tmp/owned")),
        "Property 'missing' does not exist on type 'typeof import(\"base\")'. See 'base'."
    );
}
