// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `blueice-downloads` command line and its `run` loop, kept in the
//! library so the binary is a few lines and this logic is unit- and
//! subprocess-tested like any other.

use crate::manager::{ManagerConfig, TransferManager};
use crate::policy::{default_data_dir, default_download_dir};
use crate::server::serve;
use blueice_ipc::downloads::default_downloads_socket_path;
use blueice_ipc::gatekeeper::default_gatekeeper_socket_path;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub const USAGE: &str = "usage: blueice-downloads [--socket PATH] [--download-dir DIR] [--data-dir DIR] [--gatekeeper-socket PATH] [--max-concurrent N] [--sftp-known-hosts PATH] [--sftp-private-key PATH]";

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
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value\n{USAGE}"));
        match flag.as_str() {
            "--socket" => parsed.socket = PathBuf::from(value("--socket")?),
            "--download-dir" => parsed.download_dir = PathBuf::from(value("--download-dir")?),
            "--data-dir" => parsed.data_dir = PathBuf::from(value("--data-dir")?),
            "--gatekeeper-socket" => parsed.gatekeeper_socket = PathBuf::from(value("--gatekeeper-socket")?),
            "--max-concurrent" => {
                let raw = value("--max-concurrent")?;
                parsed.max_concurrent = raw.parse().ok().filter(|&n| n > 0).ok_or_else(|| format!("--max-concurrent must be a positive number, not {raw:?}"))?;
            }
            "--sftp-known-hosts" => parsed.sftp_known_hosts = Some(PathBuf::from(value("--sftp-known-hosts")?)),
            "--sftp-private-key" => parsed.sftp_private_key = Some(PathBuf::from(value("--sftp-private-key")?)),
            "--help" | "-h" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
        }
    }
    Ok(parsed)
}

/// Runs the downloads process: binds the socket (refusing to start beside a
/// live one, replacing a stale one), opens the manager, serves until a
/// client sends `Shutdown`, pauses whatever is running, and removes the
/// socket.
pub fn run(args: Args) -> io::Result<()> {
    if let Some(parent) = args.socket.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if args.socket.exists() {
        if UnixStream::connect(&args.socket).is_ok() {
            return Err(io::Error::new(io::ErrorKind::AddrInUse, format!("another downloads process is already listening on {}", args.socket.display())));
        }
        std::fs::remove_file(&args.socket)?;
    }
    let listener = UnixListener::bind(&args.socket)?;
    // Credential-setting requests can carry a password, so do not rely on
    // the caller's umask: only this user may connect to the socket.
    std::fs::set_permissions(&args.socket, std::fs::Permissions::from_mode(0o600))?;
    let mut config = ManagerConfig::new(&args.download_dir, &args.data_dir, &args.gatekeeper_socket);
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
        assert_eq!(Args::default().socket.file_name().unwrap(), "downloads.sock");
    }

    #[test]
    fn every_flag_overrides_its_default() {
        let args = parse(&["--socket", "/s", "--download-dir", "/d", "--data-dir", "/data", "--gatekeeper-socket", "/g", "--max-concurrent", "5"]).unwrap();
        assert_eq!(
            args,
            Args { socket: "/s".into(), download_dir: "/d".into(), data_dir: "/data".into(), gatekeeper_socket: "/g".into(), max_concurrent: 5, sftp_known_hosts: None, sftp_private_key: None }
        );
    }

    #[test]
    fn an_sftp_known_hosts_file_can_be_selected_explicitly() {
        assert_eq!(parse(&["--sftp-known-hosts", "/keys/known_hosts"]).unwrap().sftp_known_hosts, Some(PathBuf::from("/keys/known_hosts")));
    }

    #[test]
    fn an_sftp_private_key_can_be_selected_without_putting_it_in_a_url() {
        assert_eq!(parse(&["--sftp-private-key", "/keys/id_ed25519"]).unwrap().sftp_private_key, Some(PathBuf::from("/keys/id_ed25519")));
    }

    #[test]
    fn bad_arguments_are_reported_with_the_usage() {
        for bad in [&["--nope"][..], &["--socket"], &["--max-concurrent", "0"], &["--max-concurrent", "many"], &["--max-concurrent"]] {
            let err = parse(bad).unwrap_err();
            assert!(err.contains("usage:") || err.contains("positive"), "{bad:?}: {err}");
        }
        assert_eq!(parse(&["--help"]).unwrap_err(), USAGE);
        assert_eq!(parse(&["-h"]).unwrap_err(), USAGE);
    }
}
