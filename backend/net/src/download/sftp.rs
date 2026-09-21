// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! SFTP's implementation of the download backend.  Each segment owns an SSH
//! connection: that maps naturally to the coordinator's independently
//! retryable workers, and avoids sharing a stateful SFTP channel between
//! workers.  Host verification is strict -- no trust-on-first-use fallback.

use crate::download::backend::{ByteRange, ByteStream, TransferBackend};
use crate::download::credentials::{load_sftp_password, SftpCredentialRef};
use crate::download::probe::Probe;
use crate::download::{DownloadError, DownloadOptions};
use percent_encoding::percent_decode_str;
use ssh2::{CheckResult, KnownHostFileKind, Session, Sftp};
use std::collections::HashMap;
use std::io::{Seek, SeekFrom};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use url::Url;
use zeroize::Zeroizing;

pub(crate) struct SftpBackend {
    connect_timeout: Duration,
    response_timeout: Duration,
    known_hosts: Option<PathBuf>,
    /// Keyring backends are not reliably reentrant on every supported OS.
    /// Serialize each credential's first lookup, then keep the zeroizing
    /// secret only for this backend instance's short transfer lifetime.
    passwords: Mutex<HashMap<SftpCredentialRef, Option<Zeroizing<String>>>>,
}

struct Endpoint {
    host: String,
    port: u16,
    username: String,
    path: String,
    url: String,
}

impl SftpBackend {
    pub(crate) fn new(options: &DownloadOptions) -> Self {
        SftpBackend { connect_timeout: options.connect_timeout, response_timeout: options.response_timeout, known_hosts: options.sftp_known_hosts.clone(), passwords: Mutex::new(HashMap::new()) }
    }

    fn known_hosts_path(&self) -> Result<PathBuf, DownloadError> {
        if let Some(path) = &self.known_hosts {
            return Ok(path.clone());
        }
        let home = std::env::var_os("HOME").ok_or_else(|| DownloadError::HostVerification("no known-hosts path was configured and HOME is unset".to_string()))?;
        Ok(PathBuf::from(home).join(".ssh").join("known_hosts"))
    }

    fn password(&self, endpoint: &Endpoint) -> Result<Option<Zeroizing<String>>, DownloadError> {
        let reference = SftpCredentialRef::new(&endpoint.host, endpoint.port, &endpoint.username).map_err(|error| DownloadError::Credentials(error.to_string()))?;
        let mut passwords = self.passwords.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(password) = passwords.get(&reference) {
            return Ok(password.clone());
        }
        let password = load_sftp_password(&reference).map_err(|error| DownloadError::Credentials(error.to_string()))?;
        passwords.insert(reference, password.clone());
        Ok(password)
    }

    fn connect(&self, endpoint: &Endpoint) -> Result<Sftp, DownloadError> {
        let addresses = (endpoint.host.as_str(), endpoint.port).to_socket_addrs().map_err(|error| DownloadError::Network(format!("could not resolve {}: {error}", endpoint.host)))?;
        let mut last_error = None;
        let tcp = addresses
            .filter_map(|address| match TcpStream::connect_timeout(&address, self.connect_timeout) {
                Ok(stream) => Some(stream),
                Err(error) => {
                    last_error = Some(error);
                    None
                }
            })
            .next()
            .ok_or_else(|| DownloadError::Network(format!("could not connect to {}:{}: {}", endpoint.host, endpoint.port, last_error.map(|error| error.to_string()).unwrap_or_else(|| "no address resolved".to_string()))))?;
        tcp.set_read_timeout(Some(self.response_timeout)).map_err(DownloadError::from)?;
        tcp.set_write_timeout(Some(self.response_timeout)).map_err(DownloadError::from)?;

        let mut session = Session::new().map_err(|error| DownloadError::Network(format!("could not create an SSH session: {error}")))?;
        session.set_timeout(self.response_timeout.as_millis().clamp(1, u32::MAX as u128) as u32);
        session.set_tcp_stream(tcp);
        session.handshake().map_err(|error| DownloadError::Network(format!("SSH handshake with {}:{} failed: {error}", endpoint.host, endpoint.port)))?;

        let key = session.host_key().map(|(key, _)| key).ok_or_else(|| DownloadError::HostVerification(format!("{}:{} sent no host key", endpoint.host, endpoint.port)))?;
        let known_hosts_path = self.known_hosts_path()?;
        let mut known_hosts = session.known_hosts().map_err(|error| DownloadError::HostVerification(format!("could not open known-hosts data: {error}")))?;
        known_hosts.read_file(&known_hosts_path, KnownHostFileKind::OpenSSH).map_err(|error| DownloadError::HostVerification(format!("could not read {}: {error}", known_hosts_path.display())))?;
        match known_hosts.check_port(&endpoint.host, endpoint.port, key) {
            CheckResult::Match => {}
            CheckResult::NotFound => return Err(DownloadError::HostVerification(format!("{}:{} is not present in {}", endpoint.host, endpoint.port, known_hosts_path.display()))),
            CheckResult::Mismatch => return Err(DownloadError::HostVerification(format!("{}:{} does not match its key in {}", endpoint.host, endpoint.port, known_hosts_path.display()))),
            CheckResult::Failure => return Err(DownloadError::HostVerification(format!("could not check {}:{} against {}", endpoint.host, endpoint.port, known_hosts_path.display()))),
        }

        if session.userauth_agent(&endpoint.username).is_err() {
            match self.password(endpoint)? {
                Some(password) => session.userauth_password(&endpoint.username, &password).map_err(|_| DownloadError::Authentication(format!("the saved SFTP password could not authenticate {}", endpoint.username)))?,
                None => return Err(DownloadError::Authentication(format!("no matching SSH-agent identity or saved SFTP password for {}", endpoint.username))),
            }
        }
        if !session.authenticated() {
            return Err(DownloadError::Authentication(format!("the SSH agent did not authenticate {}", endpoint.username)));
        }
        session.sftp().map_err(|error| DownloadError::Network(format!("could not start SFTP for {}@{}: {error}", endpoint.username, endpoint.host)))
    }

    fn revision(stat: &ssh2::FileStat) -> Option<String> {
        stat.mtime.map(|time| format!("sftp-mtime:{time}"))
    }

    fn check_unchanged(probe: &Probe, stat: &ssh2::FileStat) -> Result<(), DownloadError> {
        if stat.size != probe.total {
            return Err(DownloadError::ResourceChanged(format!("the file's size changed from {:?} to {:?} bytes", probe.total, stat.size)));
        }
        if Self::revision(stat) != probe.last_modified {
            return Err(DownloadError::ResourceChanged("the file's SFTP modification time changed during the download".to_string()));
        }
        Ok(())
    }
}

impl TransferBackend for SftpBackend {
    fn probe(&self, url: &str) -> Result<Probe, DownloadError> {
        let endpoint = parse_endpoint(url)?;
        let sftp = self.connect(&endpoint)?;
        let stat = sftp.stat(Path::new(&endpoint.path)).map_err(|error| DownloadError::Protocol(format!("could not stat {}: {error}", endpoint.path)))?;
        Ok(Probe {
            url: endpoint.url.clone(),
            final_url: endpoint.url,
            total: stat.size,
            accepts_ranges: true,
            etag: None,
            last_modified: Self::revision(&stat),
            content_type: None,
            content_disposition: None,
            // SFTP exposes mtime only to seconds, so it is useful for
            // detecting a same-run mutation but not strong enough to retain
            // a partial file across a restart.
            restart_resume_safe: false,
        })
    }

    fn get(&self, probe: &Probe, range: Option<ByteRange>) -> Result<ByteStream, DownloadError> {
        let endpoint = parse_endpoint(&probe.url)?;
        let sftp = self.connect(&endpoint)?;
        let path = Path::new(&endpoint.path);
        let stat = sftp.stat(path).map_err(|error| DownloadError::Network(format!("could not restat {}: {error}", endpoint.path)))?;
        Self::check_unchanged(probe, &stat)?;
        let mut file = sftp.open(path).map_err(|error| DownloadError::Network(format!("could not open {}: {error}", endpoint.path)))?;
        if let Some(range) = range {
            file.seek(SeekFrom::Start(range.start)).map_err(|error| DownloadError::Network(format!("could not seek {} to byte {}: {error}", endpoint.path, range.start)))?;
        }
        Ok(Box::new(file))
    }
}

fn decode(value: &str, part: &str) -> Result<String, DownloadError> {
    percent_decode_str(value).decode_utf8().map(|value| value.into_owned()).map_err(|_| DownloadError::InvalidUrl(format!("the SFTP URL has invalid UTF-8 in its {part}")))
}

fn parse_endpoint(input: &str) -> Result<Endpoint, DownloadError> {
    let url = Url::parse(input).map_err(|error| DownloadError::InvalidUrl(format!("invalid SFTP URL {input:?}: {error}")))?;
    if url.scheme() != "sftp" {
        return Err(DownloadError::InvalidUrl(format!("unsupported scheme in {input:?} (expected sftp)")));
    }
    if url.password().is_some() {
        return Err(DownloadError::InvalidUrl("an SFTP URL must not contain a password; configure SSH-agent or keychain credentials separately".to_string()));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(DownloadError::InvalidUrl("an SFTP URL cannot contain a query or fragment".to_string()));
    }
    let host = url.host_str().ok_or_else(|| DownloadError::InvalidUrl("an SFTP URL needs a host".to_string()))?.to_string();
    let username = decode(url.username(), "username")?;
    if username.is_empty() {
        return Err(DownloadError::InvalidUrl("an SFTP URL needs a username (for example sftp://alice@example.test/file)".to_string()));
    }
    let path = decode(url.path(), "path")?;
    if path == "/" || path.is_empty() {
        return Err(DownloadError::InvalidUrl("an SFTP URL must name a file, not the server root".to_string()));
    }
    Ok(Endpoint { host, port: url.port().unwrap_or(22), username, path, url: input.to_string() })
}

pub(crate) fn validate_url(input: &str) -> Result<(), DownloadError> {
    parse_endpoint(input).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_sftp_url_names_an_explicit_user_host_port_and_file() {
        let endpoint = parse_endpoint("sftp://alice@example.test:2222/releases/a%20b.iso").unwrap();
        assert_eq!(endpoint.host, "example.test");
        assert_eq!(endpoint.port, 2222);
        assert_eq!(endpoint.username, "alice");
        assert_eq!(endpoint.path, "/releases/a b.iso");
    }

    #[test]
    fn sftp_urls_reject_embedded_secrets_and_ambiguous_targets() {
        for url in ["sftp://alice:secret@example.test/file", "sftp://example.test/file", "sftp://alice@example.test/", "sftp://alice@example.test/file?version=1"] {
            assert!(matches!(parse_endpoint(url), Err(DownloadError::InvalidUrl(_))), "{url}");
        }
    }
}
