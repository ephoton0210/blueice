// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `blueice-downloads` command line and its `run` loop, kept in the
//! library so the binary is a few lines and this logic is unit- and
//! subprocess-tested like any other.

use crate::manager::{ManagerConfig, TransferManager};
use crate::policy::{default_data_dir, default_download_dir};
use crate::server::serve;
use blueice_ipc::downloads::{DownloadsClient, default_downloads_socket_path};
use blueice_ipc::gatekeeper::default_gatekeeper_socket_path;
use blueice_ipc::local_socket::{bind_private_listener, ensure_private_socket_dir};
use std::io::{self, Read};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub const USAGE: &str = "usage: blueice-downloads [--socket PATH] [--download-dir DIR] [--data-dir DIR] [--gatekeeper-socket PATH] [--max-concurrent N] [--sftp-known-hosts PATH] [--sftp-private-key PATH]";
pub const CREDENTIAL_USAGE: &str = "usage: blueice-downloads credential set <sftp-password|sftp-key-passphrase|ftps-password> --host HOST --username USER [--port PORT] [--socket PATH] --secret-stdin";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub socket: PathBuf,
    pub download_dir: PathBuf,
    pub data_dir: PathBuf,
    pub gatekeeper_socket: PathBuf,
    pub max_concurrent: usize,
    pub sftp_known_hosts: Option<PathBuf>,
    pub sftp_private_key: Option<PathBuf>,
}

/// A credential write deliberately kept outside the MCP tool surface. The
/// secret is accepted only on this process's standard input, never as a
/// command-line argument or JSON-RPC parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    SftpPassword,
    SftpKeyPassphrase,
    FtpsPassword,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialArgs {
    pub socket: PathBuf,
    pub kind: CredentialKind,
    pub host: String,
    pub port: u16,
    pub username: String,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            socket: default_downloads_socket_path(),
            download_dir: default_download_dir(),
            data_dir: default_data_dir(),
            gatekeeper_socket: default_gatekeeper_socket_path(),
            max_concurrent: 3,
            sftp_known_hosts: None,
            sftp_private_key: None,
        }
    }
}

/// Parses the arguments after the program name; every flag is optional and
/// takes the platform default (`$XDG_RUNTIME_DIR/blueice/downloads.sock`,
/// `~/Downloads/BlueIce`, ...) when absent.
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .ok_or_else(|| format!("{name} needs a value\n{USAGE}"))
        };
        match flag.as_str() {
            "--socket" => parsed.socket = PathBuf::from(value("--socket")?),
            "--download-dir" => parsed.download_dir = PathBuf::from(value("--download-dir")?),
            "--data-dir" => parsed.data_dir = PathBuf::from(value("--data-dir")?),
            "--gatekeeper-socket" => {
                parsed.gatekeeper_socket = PathBuf::from(value("--gatekeeper-socket")?)
            }
            "--max-concurrent" => {
                let raw = value("--max-concurrent")?;
                parsed.max_concurrent = raw.parse().ok().filter(|&n| n > 0).ok_or_else(|| {
                    format!("--max-concurrent must be a positive number, not {raw:?}")
                })?;
            }
            "--sftp-known-hosts" => {
                parsed.sftp_known_hosts = Some(PathBuf::from(value("--sftp-known-hosts")?))
            }
            "--sftp-private-key" => {
                parsed.sftp_private_key = Some(PathBuf::from(value("--sftp-private-key")?))
            }
            "--help" | "-h" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
        }
    }
    Ok(parsed)
}

/// Parses the local-only credential command. It requires `--secret-stdin` so
/// a future flag cannot accidentally turn a password into argv/history data.
pub fn parse_credential_args(
    args: impl IntoIterator<Item = String>,
) -> Result<CredentialArgs, String> {
    let mut args = args.into_iter();
    let operation = args.next().ok_or_else(|| CREDENTIAL_USAGE.to_string())?;
    let raw_kind = args.next().ok_or_else(|| CREDENTIAL_USAGE.to_string())?;
    if operation != "set" {
        return Err(CREDENTIAL_USAGE.to_string());
    }
    let kind = match raw_kind.as_str() {
        "sftp-password" => CredentialKind::SftpPassword,
        "sftp-key-passphrase" => CredentialKind::SftpKeyPassphrase,
        "ftps-password" => CredentialKind::FtpsPassword,
        _ => return Err(CREDENTIAL_USAGE.to_string()),
    };
    let mut socket = default_downloads_socket_path();
    let mut host = None;
    let mut username = None;
    let mut port = None;
    let mut secret_stdin = false;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--socket" => socket = PathBuf::from(credential_value(&mut args, "--socket")?),
            "--host" => host = Some(credential_value(&mut args, "--host")?),
            "--username" => username = Some(credential_value(&mut args, "--username")?),
            "--port" => {
                let raw = credential_value(&mut args, "--port")?;
                port = Some(raw.parse::<u16>().ok().filter(|&p| p != 0).ok_or_else(|| {
                    format!(
                        "--port must be a non-zero port number, not {raw:?}\n{CREDENTIAL_USAGE}"
                    )
                })?);
            }
            "--secret-stdin" => secret_stdin = true,
            "--help" | "-h" => return Err(CREDENTIAL_USAGE.to_string()),
            _ => {
                return Err(format!(
                    "unknown credential option {flag:?}\n{CREDENTIAL_USAGE}"
                ));
            }
        }
    }
    if !secret_stdin {
        return Err(format!("--secret-stdin is required\n{CREDENTIAL_USAGE}"));
    }
    let host = host.ok_or_else(|| format!("--host is required\n{CREDENTIAL_USAGE}"))?;
    let username = username.ok_or_else(|| format!("--username is required\n{CREDENTIAL_USAGE}"))?;
    if host.is_empty() || username.is_empty() {
        return Err(format!(
            "--host and --username must not be empty\n{CREDENTIAL_USAGE}"
        ));
    }
    let default_port = match kind {
        CredentialKind::SftpPassword | CredentialKind::SftpKeyPassphrase => 22,
        CredentialKind::FtpsPassword => 21,
    };
    Ok(CredentialArgs {
        socket,
        kind,
        host,
        port: port.unwrap_or(default_port),
        username,
    })
}

fn credential_value(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{name} needs a value\n{CREDENTIAL_USAGE}"))
}

/// Reads a credential from standard input and forwards it only over the
/// private downloads socket. The command has no secret-bearing argv or MCP
/// request; the secret is not printed on success or failure.
pub fn set_credential_from_stdin(args: CredentialArgs) -> io::Result<()> {
    const MAX_SECRET_BYTES: u64 = 16 * 1024;
    let mut secret = String::new();
    io::stdin()
        .take(MAX_SECRET_BYTES + 1)
        .read_to_string(&mut secret)?;
    if secret.len() as u64 > MAX_SECRET_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "credential input exceeds 16 KiB",
        ));
    }
    let secret = secret.trim_end_matches(['\r', '\n']);
    if secret.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "credential input is empty",
        ));
    }
    let stream = UnixStream::connect(&args.socket)?;
    let mut client = DownloadsClient::connect(stream).map_err(client_error)?;
    let result = match args.kind {
        CredentialKind::SftpPassword => {
            client.set_sftp_password(&args.host, args.port, &args.username, secret)
        }
        CredentialKind::SftpKeyPassphrase => {
            client.set_sftp_private_key_passphrase(&args.host, args.port, &args.username, secret)
        }
        CredentialKind::FtpsPassword => {
            client.set_ftps_password(&args.host, args.port, &args.username, secret)
        }
    };
    result.map_err(client_error)
}

fn client_error(error: blueice_ipc::downloads::ClientError) -> io::Error {
    io::Error::other(error.to_string())
}

/// Runs the downloads process: binds the socket (refusing to start beside a
/// live one, replacing a stale one), opens the manager, serves until a
/// client sends `Shutdown`, pauses whatever is running, and removes the
/// socket.
pub fn run(args: Args) -> io::Result<()> {
    if let Some(parent) = args.socket.parent() {
        ensure_private_socket_dir(parent)?;
    }
    if args.socket.exists() {
        if UnixStream::connect(&args.socket).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!(
                    "another downloads process is already listening on {}",
                    args.socket.display()
                ),
            ));
        }
        std::fs::remove_file(&args.socket)?;
    }
    // Credential-setting requests can carry a password.  The bind helper
    // applies a restrictive umask before creating this file, not merely a
    // best-effort chmod afterwards.
    let listener = bind_private_listener(&args.socket)?;
    let mut config =
        ManagerConfig::new(&args.download_dir, &args.data_dir, &args.gatekeeper_socket);
    config.max_concurrent = args.max_concurrent;
    config.options.sftp_known_hosts = args.sftp_known_hosts;
    config.options.sftp_private_key = args.sftp_private_key;
    let manager = TransferManager::open(config)?;

    serve(listener, manager.clone(), Arc::new(AtomicBool::new(false)));

    manager.shutdown();
    let _ = std::fs::remove_file(&args.socket);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_arguments_means_every_default() {
        assert_eq!(parse(&[]).unwrap(), Args::default());
        assert_eq!(Args::default().max_concurrent, 3);
        assert_eq!(Args::default().sftp_known_hosts, None);
        assert_eq!(Args::default().sftp_private_key, None);
        assert_eq!(
            Args::default().socket.file_name().unwrap(),
            "downloads.sock"
        );
    }

    #[test]
    fn every_flag_overrides_its_default() {
        let args = parse(&[
            "--socket",
            "/s",
            "--download-dir",
            "/d",
            "--data-dir",
            "/data",
            "--gatekeeper-socket",
            "/g",
            "--max-concurrent",
            "5",
        ])
        .unwrap();
        assert_eq!(
            args,
            Args {
                socket: "/s".into(),
                download_dir: "/d".into(),
                data_dir: "/data".into(),
                gatekeeper_socket: "/g".into(),
                max_concurrent: 5,
                sftp_known_hosts: None,
                sftp_private_key: None
            }
        );
    }

    #[test]
    fn an_sftp_known_hosts_file_can_be_selected_explicitly() {
        assert_eq!(
            parse(&["--sftp-known-hosts", "/keys/known_hosts"])
                .unwrap()
                .sftp_known_hosts,
            Some(PathBuf::from("/keys/known_hosts"))
        );
    }

    #[test]
    fn an_sftp_private_key_can_be_selected_without_putting_it_in_a_url() {
        assert_eq!(
            parse(&["--sftp-private-key", "/keys/id_ed25519"])
                .unwrap()
                .sftp_private_key,
            Some(PathBuf::from("/keys/id_ed25519"))
        );
    }

    #[test]
    fn bad_arguments_are_reported_with_the_usage() {
        for bad in [
            &["--nope"][..],
            &["--socket"],
            &["--max-concurrent", "0"],
            &["--max-concurrent", "many"],
            &["--max-concurrent"],
        ] {
            let err = parse(bad).unwrap_err();
            assert!(
                err.contains("usage:") || err.contains("positive"),
                "{bad:?}: {err}"
            );
        }
        assert_eq!(parse(&["--help"]).unwrap_err(), USAGE);
        assert_eq!(parse(&["-h"]).unwrap_err(), USAGE);
    }

    #[test]
    fn credentials_require_stdin_and_choose_the_protocol_default_port() {
        let credential = parse_credential_args(
            [
                "set",
                "sftp-password",
                "--host",
                "files.example.test",
                "--username",
                "alice",
                "--secret-stdin",
            ]
            .map(str::to_string),
        )
        .unwrap();
        assert_eq!(credential.kind, CredentialKind::SftpPassword);
        assert_eq!(credential.port, 22);
        assert_eq!(credential.socket, default_downloads_socket_path());
        assert!(
            parse_credential_args(
                ["set", "ftps-password", "--host", "h", "--username", "u"].map(str::to_string)
            )
            .unwrap_err()
            .contains("--secret-stdin")
        );
    }

    #[test]
    fn credentials_reject_a_bad_kind_port_or_unknown_option() {
        for args in [
            ["set", "unknown", "--secret-stdin"].as_slice(),
            [
                "set",
                "sftp-password",
                "--host",
                "h",
                "--username",
                "u",
                "--port",
                "0",
                "--secret-stdin",
            ]
            .as_slice(),
            [
                "set",
                "sftp-password",
                "--host",
                "h",
                "--username",
                "u",
                "--password",
                "secret",
                "--secret-stdin",
            ]
            .as_slice(),
        ] {
            assert!(parse_credential_args(args.iter().map(|arg| (*arg).to_string())).is_err());
        }
    }
}
