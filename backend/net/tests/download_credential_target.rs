// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

use blueice_net::download::credentials::resolve_download_credential_target;

#[test]
fn credential_targets_use_the_transfer_parsers_without_requiring_a_file() {
    for (url, scheme, host, port, username) in [
        (
            "sftp://alice@files.example.test",
            "sftp",
            "files.example.test",
            22,
            "alice",
        ),
        (
            "sftp://a%2Bb%40c@[::1]:2222/file",
            "sftp",
            "[::1]",
            2222,
            "a+b@c",
        ),
        (
            "ftps://%E7%94%A8%E6%88%B6@files.example.test/",
            "ftps",
            "files.example.test",
            21,
            "用戶",
        ),
        (
            "ftps://alice@files.example.test:2121/a%20b",
            "ftps",
            "files.example.test",
            2121,
            "alice",
        ),
    ] {
        let target = resolve_download_credential_target(url).unwrap();
        assert_eq!(
            (
                target.scheme.as_str(),
                target.host.as_str(),
                target.port,
                target.username.as_str()
            ),
            (scheme, host, port, username)
        );
    }
}

#[test]
fn invalid_credential_targets_never_echo_supplied_secrets() {
    for url in [
        "sftp://alice:credential-marker@host/file",
        "ftps://alice:credential-marker@host/file",
        "ftp://alice@host/file",
        "https://alice@host/file",
        "sftp://host/file",
        "sftp://alice@host:0/file",
        "ftps://alice@host:0/file",
        "sftp://alice%0Aroot@host/file",
        "ftps://%FF@host/file",
        "sftp://alice@host/file?credential-marker",
        "ftps://alice@host/#credential-marker",
        "sftp://alice@host\n/file",
        "credential-marker",
    ] {
        let error = resolve_download_credential_target(url)
            .unwrap_err()
            .to_string();
        assert!(
            !error.contains("credential-marker"),
            "diagnostics must not copy the input URL"
        );
    }
    assert!(
        resolve_download_credential_target(&format!("sftp://alice@host/{}", "x".repeat(8192)))
            .is_err()
    );
}
