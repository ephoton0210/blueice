// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explicitly authorized remote declaration sources (J.4.4).
//!
//! The owner lists each source as `{ specifier, url, sha256 }`: the import
//! specifier it stands for, where its bytes may be fetched from, and the SHA-256
//! of exactly the bytes that are allowed. Nothing else makes BlueTS fetch
//! anything: compilation reads only the verified cache, and no text in a source or
//! in a declaration (an `import`, a `/// <reference>`, an `import("https://..")`)
//! is ever a fetch trigger. The fetch is a separate, explicit step, bounded in
//! size, count and total cache size; fetched bytes are checked against the pin
//! *before* they are written, and are checked again every time they are read, so a
//! corrupted or substituted cache entry is rejected rather than trusted.
//!
//! A remote declaration is type-only: it has no emitted code, no run-time
//! binding and no ambient scope, and cannot itself import anything.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use ring::digest::{digest, SHA256};

use crate::package_resolution::split_package_specifier;

/// One owner-authorized remote declaration source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteDeclarationSource {
    /// The bare specifier imports use to name it.
    pub specifier: String,
    /// `https://` location the bytes may be fetched from.
    pub url: String,
    /// Lower-case hex SHA-256 of the exact declaration bytes.
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteLimits {
    pub max_source_bytes: usize,
    pub max_sources: usize,
    pub max_cache_bytes: usize,
}

impl Default for RemoteLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 1024 * 1024,
            max_sources: 16,
            max_cache_bytes: 8 * 1024 * 1024,
        }
    }
}

/// Fetches bytes from a URL. The host supplies it, so the fetch goes through its
/// own authorization; implementations must not follow redirects to another
/// origin and must stop reading at `max_bytes`.
pub trait RemoteFetcher {
    fn fetch(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteError {
    TooManySources(usize),
    InvalidSpecifier(String),
    DuplicateSpecifier(String),
    InvalidUrl {
        url: String,
        reason: &'static str,
    },
    InvalidPin(String),
    FetchFailed {
        url: String,
        message: String,
    },
    TooLarge {
        url: String,
        limit: usize,
    },
    /// The bytes do not hash to the pin.
    PinMismatch {
        url: String,
        expected: String,
        actual: String,
    },
    NotUtf8(String),
    CacheFull {
        limit: usize,
    },
    NotCached {
        specifier: String,
        sha256: String,
    },
    /// A cached file no longer hashes to its name.
    CorruptCache {
        sha256: String,
    },
    Io(String),
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManySources(limit) => {
                write!(formatter, "more than {limit} remote declaration sources")
            }
            Self::InvalidSpecifier(specifier) => write!(
                formatter,
                "remote declaration specifier `{specifier}` is not a bare package specifier"
            ),
            Self::DuplicateSpecifier(specifier) => {
                write!(
                    formatter,
                    "duplicate remote declaration specifier `{specifier}`"
                )
            }
            Self::InvalidUrl { url, reason } => {
                write!(
                    formatter,
                    "remote declaration URL `{url}` is refused: {reason}"
                )
            }
            Self::InvalidPin(pin) => {
                write!(formatter, "`{pin}` is not a lower-case hex SHA-256 pin")
            }
            Self::FetchFailed { url, message } => {
                write!(formatter, "fetching `{url}` failed: {message}")
            }
            Self::TooLarge { url, limit } => {
                write!(formatter, "`{url}` is larger than the {limit}-byte limit")
            }
            Self::PinMismatch {
                url,
                expected,
                actual,
            } => write!(
                formatter,
                "`{url}` does not match its pin (expected sha256 {expected}, got {actual})"
            ),
            Self::NotUtf8(url) => write!(formatter, "`{url}` is not UTF-8 text"),
            Self::CacheFull { limit } => {
                write!(
                    formatter,
                    "the declaration cache would exceed {limit} bytes"
                )
            }
            Self::NotCached { specifier, sha256 } => write!(
                formatter,
                "remote declaration `{specifier}` ({sha256}) is not cached; run \
                 `bluetsc fetch-declarations` to fetch it (compilation never fetches)"
            ),
            Self::CorruptCache { sha256 } => {
                write!(
                    formatter,
                    "cached declaration {sha256} does not match its hash"
                )
            }
            Self::Io(message) => write!(formatter, "declaration cache I/O error: {message}"),
        }
    }
}

impl std::error::Error for RemoteError {}

pub fn sha256_hex(bytes: &[u8]) -> String {
    digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Checks the owner's list on its own, before any fetch or cache access.
pub fn validate_sources(
    sources: &[RemoteDeclarationSource],
    limits: &RemoteLimits,
) -> Result<(), RemoteError> {
    if sources.len() > limits.max_sources {
        return Err(RemoteError::TooManySources(limits.max_sources));
    }
    let mut seen = BTreeSet::new();
    for source in sources {
        match split_package_specifier(&source.specifier) {
            Some((name, rest)) if rest.is_empty() || !name.is_empty() => {}
            _ => return Err(RemoteError::InvalidSpecifier(source.specifier.clone())),
        }
        if !seen.insert(source.specifier.as_str()) {
            return Err(RemoteError::DuplicateSpecifier(source.specifier.clone()));
        }
        validate_url(&source.url)?;
        if source.sha256.len() != 64
            || !source
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(RemoteError::InvalidPin(source.sha256.clone()));
        }
    }
    Ok(())
}

fn validate_url(url: &str) -> Result<(), RemoteError> {
    let refuse = |reason| RemoteError::InvalidUrl {
        url: url.to_string(),
        reason,
    };
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| refuse("only https is allowed"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err(refuse("no host"));
    }
    if authority.contains('@') {
        return Err(refuse("credentials in a URL are not allowed"));
    }
    if url.contains('#') {
        return Err(refuse("a fragment is not part of what is fetched"));
    }
    if url
        .chars()
        .any(|character| character.is_control() || character == ' ')
    {
        return Err(refuse("control characters or spaces"));
    }
    Ok(())
}

/// A content-addressed directory of verified declaration files.
pub struct DeclarationCache {
    directory: PathBuf,
    limits: RemoteLimits,
}

/// What a fetch step did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FetchReport {
    pub fetched: Vec<String>,
    pub already_cached: Vec<String>,
}

impl DeclarationCache {
    pub fn new(directory: impl Into<PathBuf>, limits: RemoteLimits) -> Self {
        Self {
            directory: directory.into(),
            limits,
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    fn path_of(&self, sha256: &str) -> PathBuf {
        self.directory.join(format!("{sha256}.d.ts"))
    }

    /// The verified text for a pin, if cached. A cached file whose bytes do not
    /// hash to its name is an error, never trusted and never silently replaced.
    pub fn read(&self, sha256: &str) -> Result<Option<String>, RemoteError> {
        let path = self.path_of(sha256);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(RemoteError::Io(error.to_string())),
        };
        if bytes.len() > self.limits.max_source_bytes || sha256_hex(&bytes) != sha256 {
            return Err(RemoteError::CorruptCache {
                sha256: sha256.to_string(),
            });
        }
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| RemoteError::NotUtf8(sha256.to_string()))
    }

    /// The verified text of each source, from the cache only. A source that is
    /// not cached is an error that names the explicit fetch step.
    pub fn load_all(
        &self,
        sources: &[RemoteDeclarationSource],
    ) -> Result<Vec<(RemoteDeclarationSource, String)>, RemoteError> {
        validate_sources(sources, &self.limits)?;
        sources
            .iter()
            .map(|source| {
                self.read(&source.sha256)?
                    .map(|text| (source.clone(), text))
                    .ok_or_else(|| RemoteError::NotCached {
                        specifier: source.specifier.clone(),
                        sha256: source.sha256.clone(),
                    })
            })
            .collect()
    }

    fn cached_bytes(&self) -> usize {
        fs::read_dir(&self.directory)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| entry.metadata().ok())
            .map(|metadata| usize::try_from(metadata.len()).unwrap_or(usize::MAX))
            .fold(0usize, usize::saturating_add)
    }

    /// Fetches every listed source that is not already cached, in the order
    /// listed: only the owner's URLs, each bounded, each checked against its pin
    /// before it is written (atomically: a partial or mismatched download never
    /// appears in the cache).
    pub fn fetch_missing(
        &self,
        sources: &[RemoteDeclarationSource],
        fetcher: &dyn RemoteFetcher,
    ) -> Result<FetchReport, RemoteError> {
        validate_sources(sources, &self.limits)?;
        fs::create_dir_all(&self.directory).map_err(|error| RemoteError::Io(error.to_string()))?;
        let mut report = FetchReport::default();
        for source in sources {
            if self.read(&source.sha256)?.is_some() {
                report.already_cached.push(source.specifier.clone());
                continue;
            }
            let bytes = fetcher
                .fetch(&source.url, self.limits.max_source_bytes)
                .map_err(|message| RemoteError::FetchFailed {
                    url: source.url.clone(),
                    message,
                })?;
            if bytes.len() > self.limits.max_source_bytes {
                return Err(RemoteError::TooLarge {
                    url: source.url.clone(),
                    limit: self.limits.max_source_bytes,
                });
            }
            let actual = sha256_hex(&bytes);
            if actual != source.sha256 {
                return Err(RemoteError::PinMismatch {
                    url: source.url.clone(),
                    expected: source.sha256.clone(),
                    actual,
                });
            }
            if std::str::from_utf8(&bytes).is_err() {
                return Err(RemoteError::NotUtf8(source.url.clone()));
            }
            if self.cached_bytes().saturating_add(bytes.len()) > self.limits.max_cache_bytes {
                return Err(RemoteError::CacheFull {
                    limit: self.limits.max_cache_bytes,
                });
            }
            self.store(&source.sha256, &bytes)?;
            report.fetched.push(source.specifier.clone());
        }
        Ok(report)
    }

    fn store(&self, sha256: &str, bytes: &[u8]) -> Result<(), RemoteError> {
        let temporary = self
            .directory
            .join(format!(".{sha256}.{}.partial", std::process::id()));
        let io = |error: std::io::Error| RemoteError::Io(error.to_string());
        let mut file = fs::File::create(&temporary).map_err(io)?;
        file.write_all(bytes).map_err(io)?;
        file.sync_all().map_err(io)?;
        fs::rename(&temporary, self.path_of(sha256)).map_err(io)
    }
}

#[cfg(test)]
mod tests;
