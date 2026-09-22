// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The on-disk record that makes a transfer resumable
//! (`phase-10-download-manager/PLAN.md`'s "Files, sidecar, and resume"):
//! a small JSON file next to the pre-allocated data file, holding the
//! per-segment progress and what is needed to prove the remote file is
//! still the same one.

use crate::download::MAX_TRANSFER_SEGMENTS;
use crate::download::probe::{Probe, Validator, is_weak_etag};
use crate::download::secure_fs;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// Version 2 refuses sidecars created before HTTP `Last-Modified` values
/// were checked for RFC 9110's strong-validator condition.
pub const SIDECAR_VERSION: u32 = 2;
/// A valid, bounded sidecar with 1,024 segments is comfortably below this.
/// Rejecting a larger adjacent file prevents a corrupt local sidecar from
/// consuming arbitrary memory during restart.
const MAX_SIDECAR_BYTES: u64 = 256 * 1024;

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut os = path.as_os_str().to_owned();
    os.push(suffix);
    PathBuf::from(os)
}

/// Where the in-progress data lives until the transfer completes and it is
/// renamed to `dest`, so `dest` itself never holds a partial file.
pub fn part_path(dest: &Path) -> PathBuf {
    append_suffix(dest, ".blueice-part")
}

pub fn sidecar_path(dest: &Path) -> PathBuf {
    append_suffix(dest, ".blueice-part.json")
}

/// One segment's persisted progress: `[start, end)`, of which the bytes up
/// to `pos` are durably on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidecarSegment {
    pub start: u64,
    pub end: u64,
    pub pos: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sidecar {
    pub version: u32,
    pub url: String,
    pub final_url: String,
    pub total: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    pub segments: Vec<SidecarSegment>,
}

/// Why a stored sidecar can't be resumed from -- the transfer restarts
/// from byte 0 instead, and says why in its event log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartReason {
    UnsupportedVersion,
    DifferentUrl,
    DifferentSize,
    /// Either side has nothing to prove the remote file is unchanged.
    NoValidator,
    ValidatorChanged,
    DataFileWrongLength,
    InconsistentSegments,
}

impl fmt::Display for RestartReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RestartReason::UnsupportedVersion => "the saved progress was written by an incompatible version",
            RestartReason::DifferentUrl => "the saved progress is for a different URL",
            RestartReason::DifferentSize => "the remote file's size differs from the saved progress",
            RestartReason::NoValidator => "there is no ETag or Last-Modified to confirm the remote file is unchanged",
            RestartReason::ValidatorChanged => "the remote file changed since the saved progress (its ETag or Last-Modified differs)",
            RestartReason::DataFileWrongLength => "the partial file on disk is not the expected size",
            RestartReason::InconsistentSegments => "the saved segment list is inconsistent",
        })
    }
}

fn stored_validator(etag: &Option<String>, last_modified: &Option<String>) -> Option<Validator> {
    match etag {
        Some(etag) if !is_weak_etag(etag) => Some(Validator::StrongEtag(etag.clone())),
        _ => last_modified.clone().map(Validator::LastModified),
    }
}

impl Sidecar {
    pub fn from_probe(probe: &Probe, segments: Vec<SidecarSegment>) -> Self {
        Sidecar {
            version: SIDECAR_VERSION,
            url: probe.url.clone(),
            final_url: probe.final_url.clone(),
            total: probe.total.unwrap_or(0),
            etag: probe.etag.clone(),
            last_modified: probe.last_modified.clone(),
            content_type: probe.content_type.clone(),
            segments,
        }
    }

    /// Whether this sidecar may be resumed from, given a fresh `probe` of
    /// the same URL and the length of the data file found on disk. Every
    /// way it might not is a distinct [`RestartReason`], because "why did
    /// this restart from zero" is exactly what a user watching a large
    /// download wants to be told.
    pub fn check(&self, probe: &Probe, data_len: u64) -> Result<(), RestartReason> {
        if self.version != SIDECAR_VERSION {
            return Err(RestartReason::UnsupportedVersion);
        }
        if self.url != probe.url {
            return Err(RestartReason::DifferentUrl);
        }
        if probe.total != Some(self.total) {
            return Err(RestartReason::DifferentSize);
        }
        match (
            stored_validator(&self.etag, &self.last_modified),
            probe.validator(),
        ) {
            (Some(stored), Some(fresh)) if stored == fresh => {}
            (Some(_), Some(_)) => return Err(RestartReason::ValidatorChanged),
            _ => return Err(RestartReason::NoValidator),
        }
        if data_len != self.total {
            return Err(RestartReason::DataFileWrongLength);
        }
        self.check_segments()
    }

    /// The segments, in any stored order, must tile `[0, total)` exactly
    /// with no empty span, and every `pos` must lie inside its own span.
    fn check_segments(&self) -> Result<(), RestartReason> {
        if self.segments.len() > MAX_TRANSFER_SEGMENTS {
            return Err(RestartReason::InconsistentSegments);
        }
        let mut segments: Vec<&SidecarSegment> = self.segments.iter().collect();
        segments.sort_by_key(|s| s.start);
        let (Some(first), Some(last)) = (segments.first(), segments.last()) else {
            return Err(RestartReason::InconsistentSegments);
        };
        if first.start != 0 || last.end != self.total {
            return Err(RestartReason::InconsistentSegments);
        }
        for segment in &segments {
            if segment.start >= segment.end
                || segment.pos < segment.start
                || segment.pos > segment.end
            {
                return Err(RestartReason::InconsistentSegments);
            }
        }
        if segments.windows(2).any(|pair| pair[0].end != pair[1].start) {
            return Err(RestartReason::InconsistentSegments);
        }
        Ok(())
    }

    /// Writes the sidecar next to `dest` atomically (write a temporary
    /// file, `fsync` it, rename over the real one), so a crash mid-write
    /// leaves the previous sidecar intact instead of a truncated one.
    pub fn save(&self, dest: &Path) -> io::Result<()> {
        let path = sidecar_path(dest);
        let tmp = append_suffix(&path, ".tmp");
        let bytes = serde_json::to_vec(self).map_err(io::Error::other)?;
        let written = (|| -> io::Result<()> {
            let mut file = secure_fs::open_replace(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            secure_fs::replace(&tmp, &path)
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        written
    }

    /// The sidecar next to `dest`, or `None` if it is missing or not a
    /// sidecar at all (corrupt progress just means "start over").
    pub fn load(dest: &Path) -> Option<Sidecar> {
        let mut bytes = Vec::new();
        let mut file = secure_fs::open_existing(&sidecar_path(dest), false).ok()?;
        if file.metadata().ok()?.len() > MAX_SIDECAR_BYTES {
            return None;
        }
        file.read_to_end(&mut bytes).ok()?;
        serde_json::from_slice(&bytes).ok()
    }
}

/// Deletes the data file and sidecar for `dest`; whichever is already
/// gone is fine.
pub fn remove_partials(dest: &Path) {
    let sidecar = sidecar_path(dest);
    let _ = secure_fs::remove(&part_path(dest));
    let _ = secure_fs::remove(&append_suffix(&sidecar, ".tmp"));
    let _ = secure_fs::remove(&sidecar);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::testutil::TempDir;
    use std::path::Path;

    fn probe() -> Probe {
        Probe {
            url: "http://h/f".to_string(),
            final_url: "http://cdn/f".to_string(),
            total: Some(1_000),
            accepts_ranges: true,
            etag: Some("\"abc\"".to_string()),
            last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".to_string()),
            content_type: Some("application/octet-stream".to_string()),
            content_disposition: None,
            restart_resume_safe: true,
        }
    }

    fn segments() -> Vec<SidecarSegment> {
        vec![
            SidecarSegment {
                start: 0,
                end: 500,
                pos: 200,
            },
            SidecarSegment {
                start: 500,
                end: 1_000,
                pos: 500,
            },
        ]
    }

    fn valid() -> Sidecar {
        Sidecar::from_probe(&probe(), segments())
    }

    #[test]
    fn the_part_file_and_sidecar_sit_next_to_the_destination() {
        assert_eq!(
            part_path(Path::new("/d/file.iso")),
            Path::new("/d/file.iso.blueice-part")
        );
        assert_eq!(
            sidecar_path(Path::new("/d/file.iso")),
            Path::new("/d/file.iso.blueice-part.json")
        );
        assert_eq!(
            part_path(Path::new("/d/noext")),
            Path::new("/d/noext.blueice-part")
        );
        assert_eq!(
            sidecar_path(Path::new("rel/a.tar.gz")),
            Path::new("rel/a.tar.gz.blueice-part.json")
        );
    }

    #[test]
    fn a_consistent_sidecar_passes_the_check() {
        assert_eq!(valid().check(&probe(), 1_000), Ok(()));
    }

    #[test]
    fn segments_need_not_be_stored_in_order() {
        let mut segs = segments();
        segs.reverse();
        assert_eq!(
            Sidecar::from_probe(&probe(), segs).check(&probe(), 1_000),
            Ok(())
        );
    }

    #[test]
    fn an_unsupported_version_is_rejected() {
        let mut s = valid();
        s.version = SIDECAR_VERSION + 1;
        assert_eq!(
            s.check(&probe(), 1_000),
            Err(RestartReason::UnsupportedVersion)
        );
    }

    #[test]
    fn a_different_url_is_rejected() {
        let other = Probe {
            url: "http://h/other".to_string(),
            ..probe()
        };
        assert_eq!(
            valid().check(&other, 1_000),
            Err(RestartReason::DifferentUrl)
        );
    }

    #[test]
    fn a_different_total_size_is_rejected() {
        let other = Probe {
            total: Some(2_000),
            ..probe()
        };
        assert_eq!(
            valid().check(&other, 1_000),
            Err(RestartReason::DifferentSize)
        );
        let unknown = Probe {
            total: None,
            ..probe()
        };
        assert_eq!(
            valid().check(&unknown, 1_000),
            Err(RestartReason::DifferentSize)
        );
    }

    #[test]
    fn too_many_saved_segments_are_not_resumable() {
        let mut sidecar = valid();
        sidecar.segments = vec![
            SidecarSegment {
                start: 0,
                end: 1,
                pos: 0,
            };
            MAX_TRANSFER_SEGMENTS + 1
        ];
        assert_eq!(
            sidecar.check(&probe(), 1_000),
            Err(RestartReason::InconsistentSegments)
        );
    }

    #[test]
    fn resuming_without_anything_to_validate_against_is_refused() {
        // The fresh probe carries no validator...
        let bare = Probe {
            etag: None,
            last_modified: None,
            ..probe()
        };
        assert_eq!(valid().check(&bare, 1_000), Err(RestartReason::NoValidator));
        // ...or the stored sidecar never had one.
        let stored = Sidecar::from_probe(&bare, segments());
        assert_eq!(
            stored.check(&probe(), 1_000),
            Err(RestartReason::NoValidator)
        );
    }

    #[test]
    fn a_changed_etag_or_last_modified_is_rejected() {
        let changed_etag = Probe {
            etag: Some("\"different\"".to_string()),
            ..probe()
        };
        assert_eq!(
            valid().check(&changed_etag, 1_000),
            Err(RestartReason::ValidatorChanged)
        );

        let only_lm = Probe {
            etag: None,
            ..probe()
        };
        let stored = Sidecar::from_probe(&only_lm, segments());
        let changed_lm = Probe {
            etag: None,
            last_modified: Some("Thu, 22 Oct 2015 07:28:00 GMT".to_string()),
            ..probe()
        };
        assert_eq!(
            stored.check(&changed_lm, 1_000),
            Err(RestartReason::ValidatorChanged)
        );
    }

    #[test]
    fn a_validator_of_a_different_kind_is_conservatively_a_change() {
        // Stored with only Last-Modified; the server now offers a strong ETag.
        let only_lm = Probe {
            etag: None,
            ..probe()
        };
        let stored = Sidecar::from_probe(&only_lm, segments());
        assert_eq!(
            stored.check(&probe(), 1_000),
            Err(RestartReason::ValidatorChanged)
        );
    }

    #[test]
    fn a_data_file_of_the_wrong_length_is_rejected() {
        assert_eq!(
            valid().check(&probe(), 999),
            Err(RestartReason::DataFileWrongLength)
        );
        assert_eq!(
            valid().check(&probe(), 0),
            Err(RestartReason::DataFileWrongLength)
        );
    }

    #[test]
    fn inconsistent_segment_lists_are_rejected() {
        let cases: Vec<(&str, Vec<SidecarSegment>)> = vec![
            ("empty", vec![]),
            (
                "does not start at 0",
                vec![SidecarSegment {
                    start: 1,
                    end: 1_000,
                    pos: 1,
                }],
            ),
            (
                "does not reach the total",
                vec![SidecarSegment {
                    start: 0,
                    end: 999,
                    pos: 0,
                }],
            ),
            (
                "gap",
                vec![
                    SidecarSegment {
                        start: 0,
                        end: 400,
                        pos: 0,
                    },
                    SidecarSegment {
                        start: 500,
                        end: 1_000,
                        pos: 500,
                    },
                ],
            ),
            (
                "overlap",
                vec![
                    SidecarSegment {
                        start: 0,
                        end: 600,
                        pos: 0,
                    },
                    SidecarSegment {
                        start: 500,
                        end: 1_000,
                        pos: 500,
                    },
                ],
            ),
            (
                "pos before start",
                vec![
                    SidecarSegment {
                        start: 0,
                        end: 500,
                        pos: 0,
                    },
                    SidecarSegment {
                        start: 500,
                        end: 1_000,
                        pos: 499,
                    },
                ],
            ),
            (
                "pos after end",
                vec![
                    SidecarSegment {
                        start: 0,
                        end: 500,
                        pos: 501,
                    },
                    SidecarSegment {
                        start: 500,
                        end: 1_000,
                        pos: 500,
                    },
                ],
            ),
            (
                "empty span",
                vec![
                    SidecarSegment {
                        start: 0,
                        end: 0,
                        pos: 0,
                    },
                    SidecarSegment {
                        start: 0,
                        end: 1_000,
                        pos: 0,
                    },
                ],
            ),
        ];
        for (what, segs) in cases {
            assert_eq!(
                Sidecar::from_probe(&probe(), segs).check(&probe(), 1_000),
                Err(RestartReason::InconsistentSegments),
                "{what}"
            );
        }
    }

    #[test]
    fn a_sidecar_round_trips_through_the_file_system() {
        let dir = TempDir::new("sidecar-roundtrip");
        let dest = dir.path().join("file.iso");
        let sidecar = valid();
        sidecar.save(&dest).unwrap();
        assert_eq!(Sidecar::load(&dest), Some(sidecar));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(sidecar_path(&dest))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600,
            "a sidecar records URLs and validators"
        );
    }

    #[test]
    fn saving_is_atomic_and_leaves_no_temporary_file() {
        let dir = TempDir::new("sidecar-atomic");
        let dest = dir.path().join("file.iso");
        valid().save(&dest).unwrap();
        let mut newer = valid();
        newer.segments[0].pos = 300;
        newer.save(&dest).unwrap();
        assert_eq!(
            Sidecar::load(&dest).unwrap().segments[0].pos,
            300,
            "a second save replaces the first"
        );
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["file.iso.blueice-part.json".to_string()]);
    }

    #[test]
    fn a_missing_or_corrupt_sidecar_loads_as_none() {
        let dir = TempDir::new("sidecar-corrupt");
        let dest = dir.path().join("file.iso");
        assert_eq!(Sidecar::load(&dest), None);
        std::fs::write(sidecar_path(&dest), b"{ not json").unwrap();
        assert_eq!(Sidecar::load(&dest), None);
        std::fs::write(sidecar_path(&dest), b"{}").unwrap();
        assert_eq!(
            Sidecar::load(&dest),
            None,
            "a JSON object without the required fields is corrupt too"
        );
    }

    #[test]
    fn loading_a_sidecar_never_follows_an_external_symlink() {
        let dir = TempDir::new("sidecar-link");
        let dest = dir.path().join("file.iso");
        let outside = dir.path().join("outside.json");
        std::fs::write(&outside, serde_json::to_vec(&valid()).unwrap()).unwrap();
        std::os::unix::fs::symlink(&outside, sidecar_path(&dest)).unwrap();
        assert_eq!(Sidecar::load(&dest), None);
        assert!(outside.exists(), "a refused read must not touch the target");
    }

    #[test]
    fn a_sidecar_from_another_version_still_loads_so_check_can_name_the_reason() {
        let dir = TempDir::new("sidecar-version");
        let dest = dir.path().join("file.iso");
        let mut s = valid();
        s.version = 99;
        s.save(&dest).unwrap();
        assert_eq!(
            Sidecar::load(&dest).unwrap().check(&probe(), 1_000),
            Err(RestartReason::UnsupportedVersion)
        );
    }

    #[test]
    fn saving_into_a_missing_directory_is_an_error() {
        let dir = TempDir::new("sidecar-missing-dir");
        assert!(
            valid()
                .save(&dir.path().join("no-such-dir").join("file.iso"))
                .is_err()
        );
    }

    #[test]
    fn remove_partials_deletes_both_files_and_tolerates_their_absence() {
        let dir = TempDir::new("sidecar-remove");
        let dest = dir.path().join("file.iso");
        std::fs::write(part_path(&dest), b"data").unwrap();
        valid().save(&dest).unwrap();
        remove_partials(&dest);
        assert!(!part_path(&dest).exists() && !sidecar_path(&dest).exists());
        remove_partials(&dest); // nothing left: must not panic
    }

    #[test]
    fn every_restart_reason_explains_itself() {
        for reason in [
            RestartReason::UnsupportedVersion,
            RestartReason::DifferentUrl,
            RestartReason::DifferentSize,
            RestartReason::NoValidator,
            RestartReason::ValidatorChanged,
            RestartReason::DataFileWrongLength,
            RestartReason::InconsistentSegments,
        ] {
            assert!(reason.to_string().len() > 10, "{reason:?}");
        }
    }
}
