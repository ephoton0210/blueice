// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_ai_gatekeeper::review;
use blueice_ipc::gatekeeper::{GatekeeperReply, GatekeeperRequest};

#[test]
fn protected_fields_require_same_origin_https_post_and_canonical_origins() {
    for (source, action, method, allowed) in [
        (
            "https://example.test/login",
            "https://example.test/submit",
            "POST",
            true,
        ),
        (
            "https://EXAMPLE.test:443/login",
            "https://example.test/submit",
            "POST",
            true,
        ),
        (
            "https://example.test/login",
            "https://example.test/submit",
            "GET",
            false,
        ),
        (
            "https://example.test/login",
            "http://example.test/submit",
            "POST",
            false,
        ),
        (
            "http://example.test/login",
            "https://example.test/submit",
            "POST",
            false,
        ),
        (
            "https://example.test/login",
            "https://other.test/submit",
            "POST",
            false,
        ),
        (
            "https://example.test/login",
            "https://example.test:444/submit",
            "POST",
            false,
        ),
        (
            "https://example.test/login",
            "https://malware.test/submit",
            "POST",
            false,
        ),
    ] {
        let request = GatekeeperRequest::CheckFormSubmission {
            document_url: source.into(),
            action_url: action.into(),
            method: method.into(),
            has_protected_fields: true,
        };
        assert_eq!(
            matches!(review(&request), GatekeeperReply::Cleared),
            allowed,
            "{source} -> {action} via {method}"
        );
    }
}

#[test]
fn ordinary_http_forms_are_allowed_but_methods_and_malicious_actions_are_not() {
    for (action, method, allowed) in [
        ("http://example.test/submit", "POST", true),
        ("http://example.test/submit", "GET", true),
        ("http://example.test/submit", "DELETE", false),
        ("https://phishing.test/submit", "POST", false),
    ] {
        let request = GatekeeperRequest::CheckFormSubmission {
            document_url: "http://example.test/form".into(),
            action_url: action.into(),
            method: method.into(),
            has_protected_fields: false,
        };
        assert_eq!(
            matches!(review(&request), GatekeeperReply::Cleared),
            allowed
        );
        let mut bytes = Vec::new();
        blueice_ipc::gatekeeper::write_gatekeeper_request(&mut bytes, &request).unwrap();
        assert_eq!(
            blueice_ipc::gatekeeper::read_gatekeeper_request(&mut &bytes[..]).unwrap(),
            request
        );
    }
}
