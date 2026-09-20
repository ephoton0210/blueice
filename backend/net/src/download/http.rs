// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The one place the engine talks HTTP. Everything else (probing,
//! segment workers) goes through [`get`], so lifting the ranged fetch
//! behind a `TransferBackend` trait when Phase 11 brings a second
//! protocol (`phase-10-download-manager/PLAN.md`'s "Relationship to
//! Phase 11") means replacing this file's callers' one call, not
//! threading a new abstraction through the engine.

use crate::download::{DownloadError, DownloadOptions};
use ureq::http::Response;
use ureq::{Agent, Body};

/// An agent that reports HTTP error statuses as responses (the engine
/// classifies them itself) and bounds how long a connection or a first
/// response may take. There is deliberately **no body timeout** here:
/// `ureq`'s is a total budget, not an idle timeout, so stall detection
/// is the coordinator's watchdog instead.
pub(crate) fn agent(options: &DownloadOptions) -> Agent {
    Agent::config_builder().http_status_as_error(false).timeout_connect(Some(options.connect_timeout)).timeout_recv_response(Some(options.response_timeout)).build().into()
}

fn map_error(error: ureq::Error) -> DownloadError {
    match error {
        // A URL that doesn't parse can never succeed on retry, so it must not
        // be classed as a (retryable) network failure. `ureq` reports it as
        // `BadUri` or, for an empty host, through the `http` crate's error.
        ureq::Error::BadUri(what) => DownloadError::InvalidUrl(what),
        ureq::Error::Http(e) => DownloadError::InvalidUrl(e.to_string()),
        other => DownloadError::Network(other.to_string()),
    }
}

/// `GET url`, optionally for the inclusive range `bytes=start-end`, with
/// `If-Range` when a validator is given. Content encoding
/// is refused (`Accept-Encoding: identity`): with a compressed body, byte
/// offsets would refer to the encoded stream, not the file.
pub(crate) fn get(agent: &Agent, url: &str, range: Option<(u64, u64)>, if_range: Option<&str>) -> Result<Response<Body>, DownloadError> {
    let mut request = agent.get(url).header("Accept-Encoding", "identity");
    if let Some((start, end)) = range {
        request = request.header("Range", format!("bytes={start}-{end}"));
        if let Some(validator) = if_range {
            request = request.header("If-Range", validator);
        }
    }
    request.call().map_err(map_error)
}

/// A response header as text, if present and valid UTF-8.
pub(crate) fn header(response: &Response<Body>, name: &str) -> Option<String> {
    response.headers().get(name).and_then(|value| value.to_str().ok()).map(str::to_string)
}
