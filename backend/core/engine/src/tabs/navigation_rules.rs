// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn host_matches_block_rule(host: &str, blocked: &str) -> bool {
    host == blocked
        || (blocked.parse::<std::net::Ipv4Addr>().is_err()
            && host
                .strip_suffix(blocked)
                .is_some_and(|prefix| prefix.ends_with('.')))
}

pub(super) fn canonical_navigation_block_path_prefix(input: &str) -> Result<String, String> {
    if input.is_empty()
        || input.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_PATH_BYTES
        || !input.starts_with('/')
        || !input.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~')
        })
        || input
            .split('/')
            .skip(1)
            .any(|segment| segment == "." || segment == "..")
        || input.contains("//")
    {
        return Err("navigation-block path prefixes must be literal ASCII paths of 1–512 bytes, without empty or dot segments, queries, fragments, or percent escapes".to_string());
    }
    Ok(input.to_string())
}

pub(super) fn canonical_navigation_block_host(input: &str) -> Result<String, String> {
    if input.is_empty() || input.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_HOST_BYTES {
        return Err("navigation-block hosts must be 1–253 ASCII bytes".to_string());
    }
    let host = input.strip_suffix('.').unwrap_or(input);
    if host.split('.').any(|label| {
        label.is_empty()
            || label.len() > 63
            || !label
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            || !label
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric)
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    }) {
        return Err(
            "navigation-block hosts must use ASCII DNS labels without wildcards".to_string(),
        );
    }
    let canonical = host.to_ascii_lowercase();
    let parsed = Url::parse(&format!("http://{canonical}/")).map_err(|_| {
        "navigation-block host cannot be interpreted as a canonical HTTP host".to_string()
    })?;
    if parsed.host_str() != Some(canonical.as_str()) {
        return Err(
            "navigation-block host must already use canonical IPv4 or DNS spelling".to_string(),
        );
    }
    Ok(canonical)
}

pub(super) fn canonical_http_navigation_url(input: &str) -> Result<String, String> {
    if input.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_URL_BYTES {
        return Err("navigation-rule URL exceeds the protocol limit".to_string());
    }
    let mut url =
        Url::parse(input).map_err(|error| format!("navigation-block URL is invalid: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return Err("navigation-block URLs must be absolute HTTP(S) URLs".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("navigation-block URLs must not contain credentials".to_string());
    }
    url.set_fragment(None);
    let canonical = url.to_string();
    if canonical.len() > blueice_ipc::extension::MAX_NETWORK_BLOCK_URL_BYTES {
        return Err("canonical navigation-rule URL exceeds the protocol limit".to_string());
    }
    Ok(canonical)
}
