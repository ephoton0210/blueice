// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private navigation intent; POST data is never part of review metadata.

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowserNavigation {
    pub(crate) request: blueice_net::NavigationRequest,
    pub(crate) form: Option<FormSource>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FormSource {
    pub(crate) document_url: String,
    pub(crate) has_protected_fields: bool,
}

impl From<String> for BrowserNavigation {
    fn from(url: String) -> Self {
        Self {
            request: blueice_net::NavigationRequest::get(url),
            form: None,
        }
    }
}
