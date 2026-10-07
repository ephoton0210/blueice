// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A download transaction for original navigation bytes. There is deliberately
//! no URL fetch, probe, retry or request replay in this module.

use super::clearance::DownloadClearance;
use super::sidecar::part_path;
use super::transfer::{available_bytes, check_capacity, publish_file};
use super::{ensure_transfer_text_limit, secure_fs, DownloadError, DownloadOptions};
use blueice_ipc::downloads::ResponseDownload;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const MAX_RESPONSE_CHUNK_BYTES: usize = 64 * 1024;

pub fn validate_metadata(response: &ResponseDownload) -> Result<(), DownloadError> {
    crate::validate_url_scheme(&response.url)
        .map_err(|error| DownloadError::InvalidUrl(error.to_string()))?;
    ensure_transfer_text_limit("response URL", &response.url)?;
    if !(200..=599).contains(&response.status) {
        return Err(DownloadError::Protocol("invalid response status".into()));
    }
    for (field, value) in [
        ("content type", response.content_type.as_deref()),
        (
            "content disposition",
            response.content_disposition.as_deref(),
        ),
    ] {
        if let Some(value) = value {
            ensure_transfer_text_limit(field, value)?;
            if value.bytes().any(|byte| byte.is_ascii_control()) {
                return Err(DownloadError::Protocol(format!("invalid {field}")));
            }
        }
    }
    Ok(())
}

/// Owns a newly created partial file. Only explicit successful finish publishes
/// it; abandonment, cancellation, short input and write failure remove it.
pub struct ResponseFile {
    file: File,
    dest: PathBuf,
    response: ResponseDownload,
    options: DownloadOptions,
    completed: u64,
}

impl ResponseFile {
    pub fn begin(
        dest: &Path,
        response: ResponseDownload,
        clearance: DownloadClearance,
        options: DownloadOptions,
    ) -> Result<Self, DownloadError> {
        validate_metadata(&response)?;
        let file_name = dest
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if clearance.requested_url() != response.url
            || clearance.final_url() != response.url
            || clearance.file_name() != file_name
            || clearance.content_type() != response.content_type.as_deref()
            || clearance.total_bytes() != response.total_bytes
        {
            return Err(DownloadError::ClearanceMismatch(
                "original response metadata differs from clearance".into(),
            ));
        }
        if !options.overwrite && std::fs::symlink_metadata(dest).is_ok() {
            return Err(DownloadError::DestinationExists(dest.to_owned()));
        }
        let parent = dest
            .parent()
            .ok_or_else(|| DownloadError::Io("missing destination folder".into()))?;
        check_capacity(response.total_bytes, available_bytes(parent)?, &options)?;
        let file = secure_fs::open_new(&part_path(dest))?;
        Ok(Self {
            file,
            dest: dest.to_owned(),
            response,
            options,
            completed: 0,
        })
    }

    pub fn append(&mut self, bytes: &[u8]) -> Result<u64, DownloadError> {
        if bytes.len() > MAX_RESPONSE_CHUNK_BYTES {
            return Err(DownloadError::Protocol(
                "response chunk exceeds the bounded buffer".into(),
            ));
        }
        let next = self
            .completed
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| DownloadError::Protocol("response length overflow".into()))?;
        if let Some(total) = self.response.total_bytes {
            if next > total {
                return Err(DownloadError::Protocol(
                    "original response exceeds its declared length".into(),
                ));
            }
        }
        if let Some(limit) = self.options.max_total_bytes {
            if next > limit {
                return Err(DownloadError::SizeLimit {
                    requested: next,
                    limit,
                });
            }
        }
        check_capacity(
            Some(bytes.len() as u64),
            available_bytes(self.dest.parent().expect("validated parent"))?,
            &self.options,
        )?;
        self.file.write_all(bytes)?;
        self.completed = next;
        Ok(next)
    }

    pub fn finish(self) -> Result<u64, DownloadError> {
        if let Some(total) = self.response.total_bytes {
            if total != self.completed {
                return Err(DownloadError::Truncated {
                    got: self.completed,
                    expected: total,
                });
            }
        }
        publish_file(
            &self.file,
            &self.dest,
            self.options.overwrite,
            &self.response.url,
        )?;
        Ok(self.completed)
    }
}

impl Drop for ResponseFile {
    fn drop(&mut self) {
        let _ = secure_fs::remove(&part_path(&self.dest));
    }
}
