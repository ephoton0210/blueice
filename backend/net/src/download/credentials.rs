// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The small boundary between transfer code and the platform credential
//! store.  Secrets never enter URLs, sidecars, transfer records, or event
//! messages: they are looked up only after an SFTP server's host key has been
//! verified, or after an FTPS control channel has completed certificate and
//! hostname verification.

use keyring::Entry;
use zeroize::Zeroizing;

const SFTP_SERVICE_PREFIX: &str = "org.blueice.downloads.sftp";
const FTPS_SERVICE_PREFIX: &str = "org.blueice.downloads.ftps";

/// Identifies an SFTP password without containing the password itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SftpCredentialRef {
    host: String,
    port: u16,
    username: String,
}

impl SftpCredentialRef {
    pub fn new(host: impl Into<String>, port: u16, username: impl Into<String>) -> Result<Self, CredentialError> {
        let (host, username) = (host.into(), username.into());
        if host.is_empty() || host.chars().any(char::is_control) {
            return Err(CredentialError::InvalidReference("SFTP host must be non-empty and contain no control characters".to_string()));
        }
        if port == 0 {
            return Err(CredentialError::InvalidReference("SFTP port must be between 1 and 65535".to_string()));
        }
        if username.is_empty() || username.chars().any(char::is_control) {
            return Err(CredentialError::InvalidReference("SFTP username must be non-empty and contain no control characters".to_string()));
        }
        Ok(SftpCredentialRef { host, port, username })
    }

    fn entry(&self) -> Result<Entry, CredentialError> {
        // macOS's keychain backend uses service + account and deliberately
        // ignores keyring's `target`, so the endpoint belongs in the service
        // name rather than relying on a platform-specific target behavior.
        Entry::new(&format!("{SFTP_SERVICE_PREFIX}.{}:{}", self.host, self.port), &self.username).map_err(|_| CredentialError::Unavailable)
    }
}

/// Identifies an explicit-FTPS password without containing the password
/// itself. Plain FTP has no password reference because it is anonymous-only.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FtpsCredentialRef {
    host: String,
    port: u16,
    username: String,
}

impl FtpsCredentialRef {
    pub fn new(host: impl Into<String>, port: u16, username: impl Into<String>) -> Result<Self, CredentialError> {
        let (host, username) = (host.into(), username.into());
        if host.is_empty() || host.chars().any(char::is_control) {
            return Err(CredentialError::InvalidReference("FTPS host must be non-empty and contain no control characters".to_string()));
        }
        if port == 0 {
            return Err(CredentialError::InvalidReference("FTPS port must be between 1 and 65535".to_string()));
        }
        if username.is_empty() || username.chars().any(char::is_control) {
            return Err(CredentialError::InvalidReference("FTPS username must be non-empty and contain no control characters".to_string()));
        }
        Ok(FtpsCredentialRef { host, port, username })
    }

    fn entry(&self) -> Result<Entry, CredentialError> {
        Entry::new(&format!("{FTPS_SERVICE_PREFIX}.{}:{}", self.host, self.port), &self.username).map_err(|_| CredentialError::Unavailable)
    }
}

/// An error safe to show to callers: it never includes a password or
/// passphrase, even when the underlying platform reports one verbosely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialError {
    InvalidReference(String),
    Unavailable,
}

impl std::fmt::Display for CredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CredentialError::InvalidReference(message) => f.write_str(message),
            CredentialError::Unavailable => f.write_str("the operating system credential store is unavailable"),
        }
    }
}

impl std::error::Error for CredentialError {}

/// Saves a password in the operating system's credential store. The caller
/// owns the input password and should discard it after this returns; no copy
/// is retained in BlueIce configuration or transfer state.
pub fn save_sftp_password(reference: &SftpCredentialRef, password: &str) -> Result<(), CredentialError> {
    if password.is_empty() {
        return Err(CredentialError::InvalidReference("SFTP password must not be empty".to_string()));
    }
    reference.entry()?.set_password(password).map_err(|_| CredentialError::Unavailable)
}

/// Deletes an SFTP password. A missing password is already the desired
/// state, so this is idempotent.
pub fn delete_sftp_password(reference: &SftpCredentialRef) -> Result<(), CredentialError> {
    match reference.entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(CredentialError::Unavailable),
    }
}

/// Reads the password only when an authenticated transfer needs it. The
/// returned string is zeroized on drop; callers must not format or log it.
pub(crate) fn load_sftp_password(reference: &SftpCredentialRef) -> Result<Option<Zeroizing<String>>, CredentialError> {
    match reference.entry()?.get_password() {
        Ok(password) => Ok(Some(Zeroizing::new(password))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(CredentialError::Unavailable),
    }
}

/// Saves an explicit-FTPS password in the operating system credential store.
pub fn save_ftps_password(reference: &FtpsCredentialRef, password: &str) -> Result<(), CredentialError> {
    if password.is_empty() {
        return Err(CredentialError::InvalidReference("FTPS password must not be empty".to_string()));
    }
    reference.entry()?.set_password(password).map_err(|_| CredentialError::Unavailable)
}

/// Deletes an explicit-FTPS password. A missing password is already the
/// desired state, so this is idempotent.
pub fn delete_ftps_password(reference: &FtpsCredentialRef) -> Result<(), CredentialError> {
    match reference.entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(CredentialError::Unavailable),
    }
}

/// Reads an explicit-FTPS password only after the TLS connection has been
/// established and verified. The returned value zeroizes itself on drop.
pub(crate) fn load_ftps_password(reference: &FtpsCredentialRef) -> Result<Option<Zeroizing<String>>, CredentialError> {
    match reference.entry()?.get_password() {
        Ok(password) => Ok(Some(Zeroizing::new(password))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(CredentialError::Unavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_reference_contains_only_endpoint_identity() {
        let reference = SftpCredentialRef::new("files.example.test", 2222, "alice").unwrap();
        assert_eq!(reference.host, "files.example.test");
        assert_eq!(reference.port, 2222);
        assert_eq!(reference.username, "alice");
    }

    #[test]
    fn ftp_and_sftp_references_use_distinct_protocol_namespaces() {
        let ftps = FtpsCredentialRef::new("files.example.test", 21, "alice").unwrap();
        assert_eq!(ftps.host, "files.example.test");
        assert_eq!(ftps.port, 21);
        assert_eq!(ftps.username, "alice");
        assert!(matches!(FtpsCredentialRef::new("host", 0, "alice"), Err(CredentialError::InvalidReference(_))));
    }

    #[test]
    fn an_empty_or_control_character_identity_is_rejected() {
        assert!(matches!(SftpCredentialRef::new("", 22, "alice"), Err(CredentialError::InvalidReference(_))));
        assert!(matches!(SftpCredentialRef::new("host", 0, "alice"), Err(CredentialError::InvalidReference(_))));
        assert!(matches!(SftpCredentialRef::new("host", 22, "alice\nroot"), Err(CredentialError::InvalidReference(_))));
    }
}
