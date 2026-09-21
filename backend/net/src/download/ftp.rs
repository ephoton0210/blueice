// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! FTP and explicit FTPS support for the download backend.
//!
//! Plain FTP is deliberately limited to anonymous access: credentials would
//! cross its clear-text control channel. `ftps://` performs RFC 4217's
//! explicit `AUTH TLS` upgrade, verifies the certificate and hostname using
//! the platform trust store, and gets its password from the OS keychain.
//!
//! FTP's `REST` command has a start offset but no end offset, so it cannot
//! meet [`TransferBackend`]'s exact-range contract. The shared engine still
//! supplies lifecycle, retries, atomic output, and gatekeeper review, but FTP
//! and FTPS are intentionally single-stream and never retain partial bytes
//! across a restart.

use crate::download::backend::{ByteRange, ByteStream, FinishableRead, TransferBackend};
use crate::download::credentials::{load_ftps_password, FtpsCredentialRef};
use crate::download::probe::Probe;
use crate::download::{DownloadError, DownloadOptions};
use percent_encoding::percent_decode_str;
use std::collections::HashMap;
use std::io::{self, Read};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::time::Duration;
use suppaftp::native_tls::TlsConnector;
use suppaftp::types::FileType;
use suppaftp::{FtpError, FtpStream, ImplFtpStream, NativeTlsConnector, NativeTlsFtpStream, TlsStream, TransferStream};
use url::Url;
use zeroize::Zeroizing;

pub(crate) struct FtpBackend {
    connect_timeout: Duration,
    response_timeout: Duration,
    /// As with SFTP, serialize each keyring lookup then retain the zeroizing
    /// secret only for this backend instance's transfer lifetime.
    passwords: Mutex<HashMap<FtpsCredentialRef, Option<Zeroizing<String>>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Protocol {
    Ftp,
    Ftps,
}

struct Endpoint {
    protocol: Protocol,
    host: String,
    port: u16,
    username: String,
    path: String,
    url: String,
}

impl FtpBackend {
    pub(crate) fn new(options: &DownloadOptions) -> Self {
        FtpBackend {
            connect_timeout: options.connect_timeout,
            response_timeout: options.response_timeout,
            passwords: Mutex::new(HashMap::new()),
        }
    }

    fn password(&self, endpoint: &Endpoint) -> Result<Zeroizing<String>, DownloadError> {
        let reference = FtpsCredentialRef::new(&endpoint.host, endpoint.port, &endpoint.username)
            .map_err(|error| DownloadError::Credentials(error.to_string()))?;
        let mut passwords = self.passwords.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(password) = passwords.get(&reference) {
            return password.clone().ok_or_else(|| {
                DownloadError::Authentication(format!(
                    "no saved FTPS password for {}@{}:{}",
                    endpoint.username, endpoint.host, endpoint.port
                ))
            });
        }
        let password = load_ftps_password(&reference).map_err(|error| DownloadError::Credentials(error.to_string()))?;
        passwords.insert(reference, password.clone());
        password.ok_or_else(|| {
            DownloadError::Authentication(format!(
                "no saved FTPS password for {}@{}:{}",
                endpoint.username, endpoint.host, endpoint.port
            ))
        })
    }

    fn tcp(&self, endpoint: &Endpoint) -> Result<TcpStream, DownloadError> {
        let addresses = (endpoint.host.as_str(), endpoint.port)
            .to_socket_addrs()
            .map_err(|_| DownloadError::Network(format!("could not resolve {}", endpoint.host)))?;
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
            .ok_or_else(|| {
                DownloadError::Network(format!(
                    "could not connect to {}:{}: {}",
                    endpoint.host,
                    endpoint.port,
                    last_error
                        .map(|error| error.to_string())
                        .unwrap_or_else(|| "no address resolved".to_string())
                ))
            })?;
        tcp.set_read_timeout(Some(self.response_timeout)).map_err(DownloadError::from)?;
        tcp.set_write_timeout(Some(self.response_timeout)).map_err(DownloadError::from)?;
        Ok(tcp)
    }

    fn configure<T: TlsStream>(&self, stream: ImplFtpStream<T>) -> Result<ImplFtpStream<T>, DownloadError> {
        let connect_timeout = self.connect_timeout;
        let response_timeout = self.response_timeout;
        let stream = stream.passive_stream_builder(move |address| {
            let socket = TcpStream::connect_timeout(&address, connect_timeout).map_err(FtpError::ConnectionError)?;
            socket.set_read_timeout(Some(response_timeout)).map_err(FtpError::ConnectionError)?;
            socket.set_write_timeout(Some(response_timeout)).map_err(FtpError::ConnectionError)?;
            Ok(socket)
        });
        stream.get_ref().set_read_timeout(Some(self.response_timeout)).map_err(DownloadError::from)?;
        stream.get_ref().set_write_timeout(Some(self.response_timeout)).map_err(DownloadError::from)?;
        Ok(stream)
    }

    fn plain_client(&self, endpoint: &Endpoint) -> Result<FtpStream, DownloadError> {
        let stream = FtpStream::connect_with_stream(self.tcp(endpoint)?).map_err(|error| ftp_error("opening the FTP control connection", error))?;
        let mut stream = self.configure(stream)?;
        stream
            .login("anonymous", "anonymous@blueice.local")
            .map_err(|_| DownloadError::Authentication("the FTP server rejected anonymous access".to_string()))?;
        stream.transfer_type(FileType::Binary).map_err(|error| ftp_error("selecting binary FTP transfer mode", error))?;
        Ok(stream)
    }

    fn secure_client(&self, endpoint: &Endpoint) -> Result<NativeTlsFtpStream, DownloadError> {
        let stream = NativeTlsFtpStream::connect_with_stream(self.tcp(endpoint)?)
            .map_err(|error| ftp_error("opening the FTPS control connection", error))?;
        let stream = self.configure(stream)?;
        let connector = TlsConnector::new().map_err(|_| DownloadError::Network("could not create the FTPS TLS verifier".to_string()))?;
        let mut stream = stream
            .into_secure(NativeTlsConnector::from(connector), &endpoint.host)
            .map_err(|error| ftp_error("verifying the FTPS server certificate", error))?;
        let password = self.password(endpoint)?;
        stream
            .login(endpoint.username.as_str(), password.as_str())
            .map_err(|_| DownloadError::Authentication("the saved FTPS password was rejected".to_string()))?;
        stream.transfer_type(FileType::Binary).map_err(|error| ftp_error("selecting binary FTPS transfer mode", error))?;
        Ok(stream)
    }

    fn probe_file<T: TlsStream>(&self, client: &mut ImplFtpStream<T>, endpoint: &Endpoint) -> Result<Probe, DownloadError> {
        let total = client
            .size(&endpoint.path)
            .map_err(|error| ftp_error("reading the FTP file size", error))?
            .try_into()
            .map_err(|_| DownloadError::Protocol("the FTP file size does not fit in u64".to_string()))?;
        let last_modified = client.mdtm(&endpoint.path).ok().map(|time| format!("ftp-mdtm:{time}"));
        Ok(Probe {
            url: endpoint.url.clone(),
            final_url: endpoint.url.clone(),
            total: Some(total),
            accepts_ranges: false,
            etag: None,
            last_modified,
            content_type: None,
            content_disposition: None,
            restart_resume_safe: false,
        })
    }

    fn check_unchanged<T: TlsStream>(&self, client: &mut ImplFtpStream<T>, endpoint: &Endpoint, probe: &Probe) -> Result<(), DownloadError> {
        let current_size: u64 = client
            .size(&endpoint.path)
            .map_err(|error| ftp_error("rechecking the FTP file size", error))?
            .try_into()
            .map_err(|_| DownloadError::Protocol("the FTP file size does not fit in u64".to_string()))?;
        if Some(current_size) != probe.total {
            return Err(DownloadError::ResourceChanged(format!(
                "the FTP file's size changed from {:?} to {current_size} bytes",
                probe.total
            )));
        }
        if let Some(expected) = &probe.last_modified {
            let current = client
                .mdtm(&endpoint.path)
                .map(|time| format!("ftp-mdtm:{time}"))
                .map_err(|_| DownloadError::ResourceChanged("the FTP server stopped reporting the file modification time".to_string()))?;
            if current != *expected {
                return Err(DownloadError::ResourceChanged("the FTP file modification time changed during the download".to_string()));
            }
        }
        Ok(())
    }

    fn get_file<T: TlsStream + Send + 'static>(
        &self,
        mut client: ImplFtpStream<T>,
        endpoint: &Endpoint,
        probe: &Probe,
        range: Option<ByteRange>,
    ) -> Result<ByteStream, DownloadError> {
        if range.is_some() {
            return Err(DownloadError::Protocol("FTP REST cannot prove an exact end offset, so FTP downloads are single-stream".to_string()));
        }
        self.check_unchanged(&mut client, endpoint, probe)?;
        let stream = client.retr_as_stream(&endpoint.path).map_err(|error| ftp_error("opening the FTP data connection", error))?;
        Ok(Box::new(FtpBody { stream: Some(stream) }))
    }
}

impl TransferBackend for FtpBackend {
    fn probe(&self, url: &str) -> Result<Probe, DownloadError> {
        let endpoint = parse_endpoint(url)?;
        match endpoint.protocol {
            Protocol::Ftp => {
                let mut client = self.plain_client(&endpoint)?;
                self.probe_file(&mut client, &endpoint)
            }
            Protocol::Ftps => {
                let mut client = self.secure_client(&endpoint)?;
                self.probe_file(&mut client, &endpoint)
            }
        }
    }

    fn get(&self, probe: &Probe, range: Option<ByteRange>) -> Result<ByteStream, DownloadError> {
        let endpoint = parse_endpoint(&probe.url)?;
        match endpoint.protocol {
            Protocol::Ftp => self.get_file(self.plain_client(&endpoint)?, &endpoint, probe, range),
            Protocol::Ftps => self.get_file(self.secure_client(&endpoint)?, &endpoint, probe, range),
        }
    }
}

/// Reads an FTP data channel and, crucially, turns its final `226`/`250`
/// control response into a stream error. The coordinator calls `finish` as
/// soon as it has written the expected byte count.
struct FtpBody<T: TlsStream + Send> {
    stream: Option<TransferStream<T>>,
}

impl<T: TlsStream + Send> Read for FtpBody<T> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.stream
            .as_mut()
            .ok_or_else(|| io::Error::other("FTP transfer was already finalized"))?
            .read(buffer)
    }
}

impl<T: TlsStream + Send> FinishableRead for FtpBody<T> {
    fn finish(&mut self) -> io::Result<()> {
        match self.stream.take() {
            Some(stream) => stream.finish().map_err(|_| io::Error::other("the FTP server did not confirm the completed transfer")),
            None => Ok(()),
        }
    }
}

impl<T: TlsStream + Send> Drop for FtpBody<T> {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

fn ftp_error(action: &str, error: FtpError) -> DownloadError {
    match error {
        FtpError::ConnectionError(_) | FtpError::SecureError(_) => DownloadError::Network(format!("{action} failed")),
        _ => DownloadError::Protocol(format!("the FTP server rejected {action}")),
    }
}

fn decode(value: &str, part: &str) -> Result<String, DownloadError> {
    let value = percent_decode_str(value)
        .decode_utf8()
        .map(|value| value.into_owned())
        .map_err(|_| DownloadError::InvalidUrl(format!("the FTP URL has invalid UTF-8 in its {part}")))?;
    if value.chars().any(char::is_control) {
        return Err(DownloadError::InvalidUrl(format!("the FTP URL's {part} must not contain control characters")));
    }
    Ok(value)
}

fn parse_endpoint(input: &str) -> Result<Endpoint, DownloadError> {
    let url = Url::parse(input).map_err(|error| DownloadError::InvalidUrl(format!("invalid FTP URL {input:?}: {error}")))?;
    let protocol = match url.scheme() {
        "ftp" => Protocol::Ftp,
        "ftps" => Protocol::Ftps,
        _ => return Err(DownloadError::InvalidUrl(format!("unsupported scheme in {input:?} (expected ftp or ftps)"))),
    };
    if url.password().is_some() {
        return Err(DownloadError::InvalidUrl("an FTP URL must not contain a password; configure an FTPS keychain credential separately".to_string()));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(DownloadError::InvalidUrl("an FTP URL cannot contain a query or fragment".to_string()));
    }
    let host = url.host_str().ok_or_else(|| DownloadError::InvalidUrl("an FTP URL needs a host".to_string()))?.to_string();
    if host.chars().any(char::is_control) {
        return Err(DownloadError::InvalidUrl("an FTP URL host must not contain control characters".to_string()));
    }
    let supplied_username = decode(url.username(), "username")?;
    let username = match protocol {
        Protocol::Ftp if supplied_username.is_empty() || supplied_username == "anonymous" => "anonymous".to_string(),
        Protocol::Ftp => {
            return Err(DownloadError::InvalidUrl(
                "plain FTP only permits anonymous access; use ftps://user@host/path for password authentication".to_string(),
            ));
        }
        Protocol::Ftps if supplied_username.is_empty() => {
            return Err(DownloadError::InvalidUrl("an FTPS URL needs a username (for example ftps://alice@example.test/file)".to_string()));
        }
        Protocol::Ftps => supplied_username,
    };
    let path = decode(url.path(), "path")?;
    if path == "/" || path.is_empty() {
        return Err(DownloadError::InvalidUrl("an FTP URL must name a file, not the server root".to_string()));
    }
    Ok(Endpoint {
        protocol,
        host,
        port: url.port().unwrap_or(21),
        username,
        path,
        url: input.to_string(),
    })
}

pub(crate) fn validate_url(input: &str) -> Result<(), DownloadError> {
    parse_endpoint(input).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn ftp_is_anonymous_and_ftps_names_a_user_host_port_and_file() {
        let ftp = parse_endpoint("ftp://example.test/releases/a%20b.iso").unwrap();
        assert_eq!(ftp.protocol, Protocol::Ftp);
        assert_eq!(ftp.username, "anonymous");
        assert_eq!(ftp.port, 21);

        let ftps = parse_endpoint("ftps://alice@example.test:2121/releases/a%20b.iso").unwrap();
        assert_eq!(ftps.protocol, Protocol::Ftps);
        assert_eq!(ftps.username, "alice");
        assert_eq!(ftps.port, 2121);
        assert_eq!(ftps.path, "/releases/a b.iso");
    }

    #[test]
    fn ftp_urls_reject_secrets_unauthenticated_users_and_control_characters() {
        for url in [
            "ftp://alice:secret@example.test/file",
            "ftp://alice@example.test/file",
            "ftps://example.test/file",
            "ftps://alice@example.test/",
            "ftps://alice@example.test/file?version=1",
            "ftps://alice@example.test/file%0d%0aDELE%20everything",
        ] {
            assert!(matches!(parse_endpoint(url), Err(DownloadError::InvalidUrl(_))), "{url}");
        }
    }

    fn command(reader: &mut BufReader<TcpStream>, expected: &str) {
        let mut got = String::new();
        reader.read_line(&mut got).unwrap();
        assert_eq!(got, format!("{expected}\r\n"));
    }

    fn reply(reader: &mut BufReader<TcpStream>, message: &str) {
        reader.get_mut().write_all(message.as_bytes()).unwrap();
        reader.get_mut().flush().unwrap();
    }

    fn authenticate_anonymous(reader: &mut BufReader<TcpStream>) {
        reply(reader, "220 mock FTP ready\r\n");
        command(reader, "USER anonymous");
        reply(reader, "331 password required\r\n");
        command(reader, "PASS anonymous@blueice.local");
        reply(reader, "230 logged in\r\n");
        command(reader, "TYPE I");
        reply(reader, "200 binary mode\r\n");
    }

    /// This is deliberately a wire-level test rather than a mock of
    /// suppaftp: it proves our backend uses anonymous login, binary transfer,
    /// `SIZE`/`MDTM` change checks, passive data, and waits for the final 226.
    #[test]
    fn anonymous_ftp_is_downloaded_as_a_verified_single_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (first, _) = listener.accept().unwrap();
            let mut first = BufReader::new(first);
            authenticate_anonymous(&mut first);
            command(&mut first, "SIZE /releases/file.bin");
            reply(&mut first, "213 6\r\n");
            command(&mut first, "MDTM /releases/file.bin");
            reply(&mut first, "213 20260102030405\r\n");

            let (second, _) = listener.accept().unwrap();
            let mut second = BufReader::new(second);
            authenticate_anonymous(&mut second);
            command(&mut second, "SIZE /releases/file.bin");
            reply(&mut second, "213 6\r\n");
            command(&mut second, "MDTM /releases/file.bin");
            reply(&mut second, "213 20260102030405\r\n");
            command(&mut second, "PASV");
            let data_listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = data_listener.local_addr().unwrap().port();
            reply(
                &mut second,
                &format!("227 entering passive mode (127,0,0,1,{},{})\r\n", port / 256, port % 256),
            );
            command(&mut second, "RETR /releases/file.bin");
            reply(&mut second, "150 opening data connection\r\n");
            let (mut data, _) = data_listener.accept().unwrap();
            data.write_all(b"abc123").unwrap();
            drop(data);
            reply(&mut second, "226 transfer complete\r\n");
        });

        let backend = FtpBackend::new(&DownloadOptions::default());
        let url = format!("ftp://{address}/releases/file.bin");
        let probe = backend.probe(&url).unwrap();
        assert_eq!(probe.total, Some(6));
        assert!(!probe.accepts_ranges);
        assert!(!probe.restart_resume_safe);
        let mut body = backend.get(&probe, None).unwrap();
        let mut bytes = Vec::new();
        body.read_to_end(&mut bytes).unwrap();
        body.finish().unwrap();
        assert_eq!(bytes, b"abc123");
        server.join().unwrap();
    }
}
