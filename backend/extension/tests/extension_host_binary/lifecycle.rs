// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn a_manifest_derived_identity_is_required_over_a_real_process_boundary() {
    let (root, manifest, extension_id) = manifest_package("derived-id");
    {
        let host = ExtensionHost::spawn_with_manifest("derived-id", Some(&manifest));

        let mut installed_extension = host.connect();
        write_extension_request(&mut installed_extension, &hello(&extension_id)).unwrap();
        assert_eq!(
            read_extension_reply(&mut installed_extension).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(&mut installed_extension, &ExtensionRequest::DomRead).unwrap();
        assert!(matches!(
            read_extension_reply(&mut installed_extension).unwrap(),
            ExtensionReply::DomReadResult { .. }
        ));
        drop(installed_extension);

        let mut friendly_name_impersonator = host.connect();
        write_extension_request(&mut friendly_name_impersonator, &hello("Binary test")).unwrap();
        assert_eq!(
            read_extension_reply(&mut friendly_name_impersonator).unwrap(),
            empty_hello_ack()
        );
        write_extension_request(&mut friendly_name_impersonator, &ExtensionRequest::DomRead)
            .unwrap();
        assert!(matches!(
            read_extension_reply(&mut friendly_name_impersonator).unwrap(),
            ExtensionReply::CapabilityDenied { capability, .. } if capability == "dom:read"
        ));
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn an_unsupported_capability_version_is_reported_without_breaking_a_compatible_capability() {
    let host = ExtensionHost::spawn("partial-version");
    let mut stream = host.connect();

    write_extension_request(
        &mut stream,
        &ExtensionRequest::Hello {
            extension_id: "minimal-slice-extension".to_string(),
            capability_versions: BTreeMap::from([
                ("dom:read".to_string(), 1),
                ("future:capability".to_string(), 99),
            ]),
        },
    )
    .unwrap();
    assert_eq!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::HelloAck {
            unsupported_capabilities: BTreeMap::from([(
                "future:capability".to_string(),
                blueice_ipc::extension::UnsupportedCapabilityVersion::UnknownCapability,
            )]),
        }
    );

    // The host must keep the connection alive and honor the separately
    // compatible, granted capability.
    write_extension_request(&mut stream, &ExtensionRequest::DomRead).unwrap();
    assert!(matches!(
        read_extension_reply(&mut stream).unwrap(),
        ExtensionReply::DomReadResult { .. }
    ));
}
