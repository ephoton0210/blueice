// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Inline Source Map v3 data URLs for CLI artifact publication.

use base64::Engine;
use blueice_bluets::SourceMap;

pub(super) fn url(map: &SourceMap) -> String {
    format!(
        "data:application/json;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(map.to_json())
    )
}
