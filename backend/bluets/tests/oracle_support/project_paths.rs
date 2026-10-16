// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Project paths reported by the pinned TypeScript build, made relative to the
//! scratch root the test created.
//!
//! `tsc` prints absolute paths with forward slashes and the filesystem's long
//! names (`C:/Users/runneradmin/...`) while the test's root can be a Windows
//! 8.3 short name (`C:\Users\RUNNER~1\...`) or a macOS `/var` symlink, so a
//! `Path::strip_prefix` against the raw root misses. Compare normalized
//! strings against both the given and the canonical root.

use std::path::Path;

fn normalize(path: &str) -> String {
    let path = path.replace('\\', "/");
    let path = if let Some(unc) = path.strip_prefix("//?/UNC/") {
        format!("//{unc}")
    } else if let Some(verbatim) = path.strip_prefix("//?/") {
        verbatim.to_string()
    } else {
        path
    };
    path.trim_end_matches('/').to_string()
}

fn strip_root<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    if root.is_empty() || path.len() < root.len() || !path.is_char_boundary(root.len()) {
        return None;
    }
    let (head, rest) = path.split_at(root.len());
    // Windows paths are case-insensitive; the pinned compiler preserves the
    // disk's casing but the test's temporary root may not.
    let same = if cfg!(windows) {
        head.eq_ignore_ascii_case(root)
    } else {
        head == root
    };
    (same && (rest.is_empty() || rest.starts_with('/'))).then(|| rest.trim_start_matches('/'))
}

pub(super) fn relative_to(root: &Path, path: &str) -> String {
    let path = normalize(path);
    let canonical = root.canonicalize().ok();
    for candidate in std::iter::once(root).chain(canonical.as_deref()) {
        if let Some(rest) = strip_root(&path, &normalize(&candidate.to_string_lossy())) {
            return rest.to_string();
        }
    }
    path.trim_start_matches("./").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_forward_slash_paths_under_a_backslash_root() {
        let root = Path::new(r"C:\Users\runneradmin\Temp\r-1");
        assert_eq!(
            relative_to(root, "C:/Users/runneradmin/Temp/r-1/p0/tsconfig.json"),
            "p0/tsconfig.json"
        );
    }

    #[test]
    fn strips_a_verbatim_root_and_leaves_foreign_paths_normalized() {
        let root = Path::new(r"\\?\C:\Temp\r-2");
        assert_eq!(
            relative_to(root, "C:/Temp/r-2/tsconfig.json"),
            "tsconfig.json"
        );
        assert_eq!(
            relative_to(root, "C:/Elsewhere/x.json"),
            "C:/Elsewhere/x.json"
        );
    }

    #[test]
    fn requires_a_segment_boundary_and_trims_dot_slash() {
        let root = Path::new("/tmp/r-3");
        assert_eq!(relative_to(root, "/tmp/r-30/a.json"), "/tmp/r-30/a.json");
        assert_eq!(relative_to(root, "./a/b.json"), "a/b.json");
        assert_eq!(relative_to(root, "/tmp/r-3"), "");
    }
}
