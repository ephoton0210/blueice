// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn without_a_supervised_assistant_every_assistant_request_is_rejected() {
    let broker = broker_with_settings(None);
    for request in [
        trusted_window::TrustedWindowRequest::InspectAssistantSettings,
        trusted_window::TrustedWindowRequest::ApproveAssistantProposal {
            id: 1,
            digest: "x".into(),
        },
        trusted_window::TrustedWindowRequest::DenyAssistantProposal { id: 1 },
        trusted_window::TrustedWindowRequest::EditAssistantSettings {
            settings: blueice_assistant_settings::AssistantSettings::default(),
        },
    ] {
        let reply = handle_trusted_window_session_request(request, &broker, &mut None);
        assert!(
            matches!(&reply, trusted_window::TrustedWindowReply::Rejected { reason } if reason.contains("no assistant")),
            "{reply:?}"
        );
    }
}
