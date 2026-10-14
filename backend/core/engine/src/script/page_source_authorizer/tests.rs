// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_bluets::{AuthorizedModule, AuthorizedModuleLoader};

fn loader() -> AuthorizedModuleLoader {
    AuthorizedModuleLoader::new([AuthorizedModule::new("entry", "1")], []).unwrap()
}

#[test]
fn authorized_page_script_graph_rejects_an_empty_entry_or_resolver_fingerprint() {
    assert_eq!(
        AuthorizedPageScriptGraph::new("", loader(), "fingerprint"),
        Err(AuthorizedPageScriptGraphError::EmptyEntry)
    );
    assert_eq!(
        AuthorizedPageScriptGraph::new("entry", loader(), "   "),
        Err(AuthorizedPageScriptGraphError::EmptyResolverFingerprint)
    );
    assert!(AuthorizedPageScriptGraph::new("entry", loader(), "fingerprint").is_ok());
}

#[test]
fn authorized_page_script_graph_error_display_messages_are_fixed() {
    assert_eq!(
        AuthorizedPageScriptGraphError::EmptyEntry.to_string(),
        "authorized page script entry is empty"
    );
    assert_eq!(
        AuthorizedPageScriptGraphError::EmptyResolverFingerprint.to_string(),
        "authorized page script resolver fingerprint is empty"
    );
}

struct MinimalPageScriptSourceAuthorizer;

impl PageScriptSourceAuthorizer for MinimalPageScriptSourceAuthorizer {
    fn authorize(
        &mut self,
        _request: &PageScriptSourceRequest,
    ) -> Result<AuthorizedPageScriptGraph, PageScriptSourceAuthorizationError> {
        Err(PageScriptSourceAuthorizationError::new("denied"))
    }
}

#[test]
fn page_script_source_authorizer_default_reports_no_retained_payload() {
    let authorizer = MinimalPageScriptSourceAuthorizer;
    assert_eq!(
        authorizer.retained_source_payload_bytes(TabId::from_u64(1), 1),
        None
    );
}

struct MinimalOutOfProcessPageScriptSourceAuthorizer;

impl OutOfProcessPageScriptSourceAuthorizer for MinimalOutOfProcessPageScriptSourceAuthorizer {
    fn authorize(
        &self,
        _request: &OutOfProcessPageScriptSourceRequest,
    ) -> Result<AuthorizedOutOfProcessPageScriptGraph, OutOfProcessPageScriptSourceAuthorizationError>
    {
        Err(OutOfProcessPageScriptSourceAuthorizationError::new(
            "denied",
        ))
    }
}

#[test]
fn out_of_process_page_script_source_authorizer_default_reports_no_retained_payload() {
    let authorizer = MinimalOutOfProcessPageScriptSourceAuthorizer;
    assert_eq!(
        authorizer.retained_source_payload_bytes(TabId::from_u64(1), 1),
        None
    );
}
