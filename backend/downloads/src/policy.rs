// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Where downloads may land (`phase-10-download-manager/PLAN.md`'s
//! "Destination policy"). An AI agent chooses `dest` over MCP, and
//! `phase-7-local-ai/PLAN.md`'s risk taxonomy names "file-system access
//! beyond a downloads directory" as something to prevent -- so a
//! requested destination is only ever a *relative path inside* one
//! directory, and this module is the single place that decides that.

use blueice_net::download::file_name::sanitize;
use blueice_net::download::sidecar::{part_path, sidecar_path};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// `$BLUEICE_DOWNLOAD_DIR`, else `~/Downloads/BlueIce`, else a directory
/// under the system temp dir. An empty override is no override.
pub fn download_dir_from(override_dir: Option<OsString>, home: Option<OsString>) -> PathBuf {
    match (override_dir.filter(|d| !d.is_empty()), home.filter(|h| !h.is_empty())) {
        (Some(dir), _) => PathBuf::from(dir),
        (None, Some(home)) => PathBuf::from(home).join("Downloads").join("BlueIce"),
        (None, None) => std::env::temp_dir().join("blueice-downloads"),
    }
}

pub fn default_download_dir() -> PathBuf {
    download_dir_from(std::env::var_os("BLUEICE_DOWNLOAD_DIR"), std::env::var_os("HOME"))
}

/// Where `transfers.json` lives: `$XDG_DATA_HOME/blueice/downloads`, else
/// `~/.local/share/blueice/downloads`, else a directory under the temp dir.
pub fn data_dir_from(xdg_data_home: Option<OsString>, home: Option<OsString>) -> PathBuf {
    match (xdg_data_home.filter(|d| !d.is_empty()), home.filter(|h| !h.is_empty())) {
        (Some(xdg), _) => PathBuf::from(xdg).join("blueice").join("downloads"),
        (None, Some(home)) => PathBuf::from(home).join(".local").join("share").join("blueice").join("downloads"),
        (None, None) => std::env::temp_dir().join("blueice-downloads-data"),
    }
}

pub fn default_data_dir() -> PathBuf {
    data_dir_from(std::env::var_os("XDG_DATA_HOME"), std::env::var_os("HOME"))
}

/// Resolves a caller-requested destination to an absolute path inside
/// `root`, or says why it can't be. Refuses (rather than silently
/// rewriting) an absolute path, any `..`, and any component that
/// [`sanitize`] would change -- a caller told "not a safe file name" can
/// fix its request; one whose file silently lands under another name can't
/// tell. Also refuses a path that *resolves* outside `root` through a
/// symlink, including a dangling one (writing through it would still land
/// outside).
pub fn resolve_requested(root: &Path, requested: &str) -> Result<PathBuf, String> {
    if requested.trim().is_empty() {
        return Err("the destination is empty".to_string());
    }
    let mut parts: Vec<&str> = Vec::new();
    for component in Path::new(requested).components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str().ok_or_else(|| "the destination is not valid UTF-8".to_string())?;
                if sanitize(part) != part {
                    return Err(format!("{part:?} is not a safe file name"));
                }
                parts.push(part);
            }
            Component::CurDir => {}
            Component::ParentDir => return Err("the destination must not contain '..'".to_string()),
            Component::RootDir | Component::Prefix(_) => return Err("the destination must be relative to the downloads directory, not absolute".to_string()),
        }
    }
    if parts.is_empty() {
        return Err("the destination has no file name".to_string());
    }

    let canonical_root = std::fs::canonicalize(root).map_err(|e| format!("the downloads directory is unavailable: {e}"))?;
    let mut joined = canonical_root.clone();
    joined.extend(&parts);

    // Resolve through whatever part of the path already exists, and require
    // the result to stay inside the root. `symlink_metadata` (not `exists`)
    // so a dangling symlink counts as existing -- and then fails to resolve.
    let mut existing = joined.as_path();
    while std::fs::symlink_metadata(existing).is_err() {
        existing = existing.parent().ok_or_else(|| "the destination has no existing parent".to_string())?;
    }
    let resolved = std::fs::canonicalize(existing).map_err(|e| format!("cannot resolve {}: {e}", existing.display()))?;
    if !resolved.starts_with(&canonical_root) {
        return Err("the destination resolves to a location outside the downloads directory".to_string());
    }
    Ok(joined)
}

/// `root/file_name`, or -- if that name is taken (a finished file, an
/// in-progress download's part or sidecar file, or a name another transfer
/// has claimed, per `taken`) -- `root/stem (n).ext` for the first free `n`.
pub fn unique_path(root: &Path, file_name: &str, taken: &dyn Fn(&Path) -> bool) -> PathBuf {
    let is_taken = |path: &Path| path.exists() || part_path(path).exists() || sidecar_path(path).exists() || taken(path);
    let first = root.join(file_name);
    if !is_taken(&first) {
        return first;
    }
    let (stem, extension) = match file_name.rfind('.') {
        Some(i) if i > 0 => (&file_name[..i], &file_name[i..]),
        _ => (file_name, ""),
    };
    (1u32..).map(|n| root.join(format!("{stem} ({n}){extension}"))).find(|path| !is_taken(path)).expect("an unbounded range always yields a free name")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::Path;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!("bd-policy-{label}-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::create_dir_all(&path).unwrap();
            Scratch(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn os(s: &str) -> Option<OsString> {
        Some(OsString::from(s))
    }

    #[test]
    fn the_download_dir_comes_from_the_environment_then_the_home_directory() {
        assert_eq!(download_dir_from(os("/custom/dir"), os("/home/u")), Path::new("/custom/dir"));
        assert_eq!(download_dir_from(None, os("/home/u")), Path::new("/home/u/Downloads/BlueIce"));
        assert_eq!(download_dir_from(os(""), os("/home/u")), Path::new("/home/u/Downloads/BlueIce"), "an empty override is no override");
        assert_eq!(download_dir_from(None, None), std::env::temp_dir().join("blueice-downloads"));
    }

    #[test]
    fn the_data_dir_follows_the_xdg_convention() {
        assert_eq!(data_dir_from(os("/xdg/data"), os("/home/u")), Path::new("/xdg/data/blueice/downloads"));
        assert_eq!(data_dir_from(None, os("/home/u")), Path::new("/home/u/.local/share/blueice/downloads"));
        assert_eq!(data_dir_from(os(""), os("/home/u")), Path::new("/home/u/.local/share/blueice/downloads"));
        assert_eq!(data_dir_from(None, None), std::env::temp_dir().join("blueice-downloads-data"));
    }

    #[test]
    fn a_relative_destination_resolves_inside_the_root() {
        let root = Scratch::new("ok");
        assert_eq!(resolve_requested(&root.0, "file.bin").unwrap(), std::fs::canonicalize(&root.0).unwrap().join("file.bin"));
        assert_eq!(resolve_requested(&root.0, "a/b/file.bin").unwrap(), std::fs::canonicalize(&root.0).unwrap().join("a").join("b").join("file.bin"));
        assert_eq!(resolve_requested(&root.0, "./file.bin").unwrap(), std::fs::canonicalize(&root.0).unwrap().join("file.bin"), "a leading ./ is harmless");
    }

    #[test]
    fn absolute_and_escaping_destinations_are_refused() {
        let root = Scratch::new("escape");
        for bad in ["/etc/passwd", "../outside.bin", "a/../../outside.bin", "a/..", "..", "/"] {
            let err = resolve_requested(&root.0, bad).unwrap_err();
            assert!(err.contains("outside") || err.contains("absolute") || err.contains("'..'"), "{bad}: {err}");
        }
    }

    #[test]
    fn an_empty_destination_is_refused() {
        let root = Scratch::new("empty");
        assert!(resolve_requested(&root.0, "").is_err());
        assert!(resolve_requested(&root.0, "   ").is_err());
        assert!(resolve_requested(&root.0, ".").is_err(), "there is no file name in '.'");
    }

    #[test]
    fn a_component_that_would_be_rewritten_by_sanitizing_is_refused_not_silently_changed() {
        let root = Scratch::new("unsafe");
        for bad in ["a:b.txt", "what?.bin", "dir/CON", "back\\slash.txt", ".hidden", "trailing.", "nul\u{0}byte"] {
            let err = resolve_requested(&root.0, bad).unwrap_err();
            assert!(err.contains("not a safe file name"), "{bad:?}: {err}");
        }
    }

    #[test]
    fn a_symlink_that_leads_out_of_the_root_is_refused() {
        let root = Scratch::new("symlink-root");
        let outside = Scratch::new("symlink-outside");
        std::os::unix::fs::symlink(&outside.0, root.0.join("link")).unwrap();
        let err = resolve_requested(&root.0, "link/file.bin").unwrap_err();
        assert!(err.contains("outside"), "{err}");
        // ...and so is a symlink to a file outside, named directly.
        std::fs::write(outside.0.join("target.bin"), b"x").unwrap();
        std::os::unix::fs::symlink(outside.0.join("target.bin"), root.0.join("file-link.bin")).unwrap();
        assert!(resolve_requested(&root.0, "file-link.bin").is_err());
    }

    #[test]
    fn a_symlink_that_stays_inside_the_root_is_fine() {
        let root = Scratch::new("symlink-inside");
        std::fs::create_dir(root.0.join("real")).unwrap();
        std::os::unix::fs::symlink(root.0.join("real"), root.0.join("alias")).unwrap();
        assert!(resolve_requested(&root.0, "alias/file.bin").is_ok());
    }

    #[test]
    fn a_missing_root_is_an_error_not_a_panic() {
        assert!(resolve_requested(Path::new("/definitely/not/here/blueice"), "file.bin").is_err());
    }

    #[test]
    fn a_free_name_is_used_as_is() {
        let root = Scratch::new("unique-free");
        assert_eq!(unique_path(&root.0, "a.bin", &|_| false), root.0.join("a.bin"));
    }

    #[test]
    fn a_taken_name_gets_a_number_before_its_extension() {
        let root = Scratch::new("unique-taken");
        std::fs::write(root.0.join("a.tar.gz"), b"x").unwrap();
        assert_eq!(unique_path(&root.0, "a.tar.gz", &|_| false), root.0.join("a.tar (1).gz"));
        std::fs::write(root.0.join("a.tar (1).gz"), b"x").unwrap();
        assert_eq!(unique_path(&root.0, "a.tar.gz", &|_| false), root.0.join("a.tar (2).gz"));
        std::fs::write(root.0.join("noext"), b"x").unwrap();
        assert_eq!(unique_path(&root.0, "noext", &|_| false), root.0.join("noext (1)"));
        std::fs::write(root.0.join(".hidden"), b"x").unwrap();
        assert_eq!(unique_path(&root.0, ".hidden", &|_| false), root.0.join(".hidden (1)"), "a leading dot is not an extension separator");
    }

    #[test]
    fn a_name_is_also_taken_by_an_in_progress_download_or_a_claim_by_another_transfer() {
        let root = Scratch::new("unique-partial");
        std::fs::write(root.0.join("a.bin.blueice-part"), b"x").unwrap();
        assert_eq!(unique_path(&root.0, "a.bin", &|_| false), root.0.join("a (1).bin"), "an in-progress download owns its name");
        let claimed = root.0.join("b.bin");
        assert_eq!(unique_path(&root.0, "b.bin", &|p| p == claimed), root.0.join("b (1).bin"), "so does a name another transfer has claimed");
    }
}
