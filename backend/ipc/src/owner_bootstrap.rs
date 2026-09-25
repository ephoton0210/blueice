// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One-shot launcher-to-core startup policy, never a public IPC operation.
//!
//! The trusted owner may distribute a sealed compiler catalog and an HTTP(S)
//! page-script integrity manifest together. The core validates both before
//! binding listeners; the isolated page host receives only closed source
//! graphs, not this policy, a URL resolver, a fetcher, or a file capability.

use crate::compiler_catalog::{CompilerCatalogBootstrap, MAX_COMPILER_CATALOG_FRAME_BYTES};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use std::io::{self, Read, Write};

pub const CORE_OWNER_BOOTSTRAP_VERSION: u32 = 1;
pub const MAX_OWNER_HTTP_POLICY_BYTES: usize = 32 * 1_024;
pub const MAX_OWNER_BOOTSTRAP_FRAME_BYTES: usize =
    MAX_COMPILER_CATALOG_FRAME_BYTES + MAX_OWNER_HTTP_POLICY_BYTES + 4_096;
pub const MAX_OWNER_HTTP_RESOURCES: usize = 128;
pub const MAX_OWNER_HTTP_URL_BYTES: usize = 2_048;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreOwnerBootstrap {
    pub version: u32,
    #[serde(default)]
    pub compiler_catalog: Option<CompilerCatalogBootstrap>,
    #[serde(default)]
    pub page_http_policy: Option<OwnerHttpPolicyBootstrap>,
}

impl fmt::Debug for CoreOwnerBootstrap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CoreOwnerBootstrap")
            .field("version", &self.version)
            .field("compiler_catalog", &self.compiler_catalog)
            .field("page_http_policy", &self.page_http_policy)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerHttpPolicyBootstrap {
    pub origin_rule: OwnerHttpOriginRule,
    pub resources: Vec<OwnerHttpResource>,
}

impl fmt::Debug for OwnerHttpPolicyBootstrap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OwnerHttpPolicyBootstrap")
            .field("resource_count", &self.resources.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OwnerHttpOriginRule {
    SameDocumentOrigin,
    ExactOrigin(String),
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerHttpResource {
    pub canonical_url: String,
    pub integrity: String,
}

impl CoreOwnerBootstrap {
    pub fn validate(&self) -> io::Result<()> {
        if self.version != CORE_OWNER_BOOTSTRAP_VERSION {
            return Err(invalid("unsupported core owner bootstrap version"));
        }
        if self.compiler_catalog.is_none() && self.page_http_policy.is_none() {
            return Err(invalid("empty core owner bootstrap"));
        }
        if let Some(catalog) = &self.compiler_catalog {
            catalog.validate()?;
        }
        if let Some(policy) = &self.page_http_policy {
            policy.validate()?;
        }
        Ok(())
    }

    pub fn from_json_slice(bytes: &[u8]) -> io::Result<Self> {
        if bytes.is_empty() || bytes.len() > MAX_OWNER_BOOTSTRAP_FRAME_BYTES {
            return Err(invalid("core owner bootstrap exceeds its fixed byte bound"));
        }
        let bootstrap: Self = serde_json::from_slice(bytes)
            .map_err(|_| invalid("invalid core owner bootstrap JSON"))?;
        bootstrap.validate()?;
        Ok(bootstrap)
    }
}

impl OwnerHttpPolicyBootstrap {
    /// Shape-checks owner input without deciding URL canonicalization. Core's
    /// existing HTTP source authorizer is the sole semantic URL policy.
    pub fn validate(&self) -> io::Result<()> {
        let encoded_bytes = serde_json::to_vec(self)
            .map_err(|_| invalid("owner HTTP policy could not be serialized"))?;
        if encoded_bytes.len() > MAX_OWNER_HTTP_POLICY_BYTES {
            return Err(invalid("owner HTTP policy exceeds its fixed byte bound"));
        }
        if self.resources.is_empty() || self.resources.len() > MAX_OWNER_HTTP_RESOURCES {
            return Err(invalid(
                "owner HTTP manifest resource count is outside its bound",
            ));
        }
        if let OwnerHttpOriginRule::ExactOrigin(origin) = &self.origin_rule {
            if origin.is_empty() || origin.len() > MAX_OWNER_HTTP_URL_BYTES || origin.contains('\0')
            {
                return Err(invalid("owner HTTP exact origin is malformed"));
            }
        }
        let mut urls = BTreeSet::new();
        for resource in &self.resources {
            if resource.canonical_url.is_empty()
                || resource.canonical_url.len() > MAX_OWNER_HTTP_URL_BYTES
                || resource.canonical_url.contains('\0')
                || !urls.insert(resource.canonical_url.as_str())
            {
                return Err(invalid("owner HTTP manifest URL is malformed or repeated"));
            }
            let Some(digest) = resource.integrity.strip_prefix("sha256:") else {
                return Err(invalid("owner HTTP manifest integrity must be SHA-256"));
            };
            if digest.len() != 64
                || !digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(invalid("owner HTTP manifest integrity is malformed"));
            }
        }
        Ok(())
    }

    pub fn from_json_slice(bytes: &[u8]) -> io::Result<Self> {
        if bytes.is_empty() || bytes.len() > MAX_OWNER_HTTP_POLICY_BYTES {
            return Err(invalid("owner HTTP policy exceeds its fixed byte bound"));
        }
        let policy: Self =
            serde_json::from_slice(bytes).map_err(|_| invalid("invalid owner HTTP policy JSON"))?;
        policy.validate()?;
        Ok(policy)
    }
}

/// The entire startup envelope is buffered and bounded before one byte is
/// written to the child pipe, so a serialization failure cannot register a
/// prefix of the owner's projects or page resources.
pub fn write_core_owner_bootstrap<W: Write>(
    writer: &mut W,
    bootstrap: &CoreOwnerBootstrap,
) -> io::Result<()> {
    bootstrap.validate()?;
    let mut payload = CappedPayload::default();
    serde_json::to_writer(&mut payload, bootstrap)
        .map_err(|_| invalid("core owner bootstrap JSON exceeds its bound"))?;
    let length = u32::try_from(payload.0.len())
        .map_err(|_| invalid("core owner bootstrap frame exceeds its bound"))?;
    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(&payload.0)
}

pub fn read_core_owner_bootstrap<R: Read>(reader: &mut R) -> io::Result<CoreOwnerBootstrap> {
    let mut length = [0u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_OWNER_BOOTSTRAP_FRAME_BYTES {
        return Err(invalid("core owner bootstrap frame exceeds its bound"));
    }
    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload)?;
    let mut trailing = [0u8; 1];
    if reader.read(&mut trailing)? != 0 {
        return Err(invalid("core owner bootstrap contains trailing data"));
    }
    CoreOwnerBootstrap::from_json_slice(&payload)
}

#[derive(Default)]
struct CappedPayload(Vec<u8>);

impl Write for CappedPayload {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_OWNER_BOOTSTRAP_FRAME_BYTES {
            return Err(invalid("core owner bootstrap frame exceeds its bound"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_policy() -> OwnerHttpPolicyBootstrap {
        OwnerHttpPolicyBootstrap {
            origin_rule: OwnerHttpOriginRule::SameDocumentOrigin,
            resources: vec![OwnerHttpResource {
                canonical_url: "https://example.test/app.js".to_string(),
                integrity: format!("sha256:{}", "a".repeat(64)),
            }],
        }
    }

    #[test]
    fn policy_and_owner_envelope_round_trip_without_debug_url_disclosure() {
        let bootstrap = CoreOwnerBootstrap {
            version: CORE_OWNER_BOOTSTRAP_VERSION,
            compiler_catalog: None,
            page_http_policy: Some(fixture_policy()),
        };
        let mut bytes = Vec::new();
        write_core_owner_bootstrap(&mut bytes, &bootstrap).unwrap();
        assert_eq!(
            read_core_owner_bootstrap(&mut bytes.as_slice()).unwrap(),
            bootstrap
        );
        assert!(!format!("{bootstrap:?}").contains("example.test"));
    }

    #[test]
    fn malformed_page_policy_and_envelope_fail_closed() {
        let mut policy = fixture_policy();
        let duplicate = policy.resources[0].clone();
        policy.resources.push(duplicate);
        assert!(policy.validate().is_err());
        policy = fixture_policy();
        policy.resources[0].integrity = "sha256:wrong".to_string();
        assert!(policy.validate().is_err());
        let bootstrap = CoreOwnerBootstrap {
            version: CORE_OWNER_BOOTSTRAP_VERSION,
            compiler_catalog: None,
            page_http_policy: None,
        };
        assert!(bootstrap.validate().is_err());
        let oversized = u32::try_from(MAX_OWNER_BOOTSTRAP_FRAME_BYTES + 1).unwrap();
        assert!(read_core_owner_bootstrap(&mut oversized.to_le_bytes().as_slice()).is_err());
    }
}
