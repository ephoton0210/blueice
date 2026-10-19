// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pre-commit evidence uses immutable blobs, with an exact index/worktree check.

use super::*;

pub(super) fn read(root: &Path, revision: &str) -> Result<Snapshot> {
    if !revision.starts_with("backend-tree:") {
        return Err("staged measurement requires an immutable backend-tree revision".into());
    }
    let snapshot = Snapshot::read(root, revision)?;
    let mut args = vec!["ls-files", "--stage", "-z", "--"];
    args.extend(SOURCE_ROOTS);
    let mut indexed = BTreeMap::new();
    for row in git(root, &args)?.split('\0').filter(|row| !row.is_empty()) {
        let (metadata, path) = row.split_once('\t').ok_or("invalid index row")?;
        let fields: Vec<_> = metadata.split_whitespace().collect();
        if fields.len() != 3 || fields[0] != "100644" || fields[2] != "0" {
            return Err("conflicted or non-regular index input".into());
        }
        if !excluded(path) {
            indexed.insert(path.to_string(), fields[1].to_string());
        }
    }
    if indexed.is_empty() || indexed != snapshot.blobs {
        return Err("index differs from the frozen backend tree".into());
    }
    let mut args = vec!["diff", "--exit-code", "--no-ext-diff", "--"];
    args.extend(SOURCE_ROOTS);
    git(root, &args).map_err(|_| "unstaged source differs from the measured index")?;
    let mut args = vec!["ls-files", "--others", "--exclude-standard", "-z", "--"];
    args.extend(SOURCE_ROOTS);
    if !git(root, &args)?.is_empty() {
        return Err("untracked source is not part of the measured index".into());
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn immutable_tree_measurement_rejects_unstaged_untracked_and_replaced_inputs() {
        let root =
            std::env::temp_dir().join(format!("bluets-inventory-tree-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        git(&root, &["init", "--quiet"]).unwrap();
        let directory = root.join("backend/bluets/src");
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("lib.rs");
        fs::write(&source, "// original\n").unwrap();
        git(&root, &["add", "--", "backend"]).unwrap();
        let tree = git(&root, &["write-tree"]).unwrap();
        let backend = git(&root, &["rev-parse", &format!("{}:backend", tree.trim())]).unwrap();
        let revision = format!("backend-tree:{}", backend.trim());
        let original = Snapshot::read(&root, &revision).unwrap();
        assert_eq!(original.blobs, read(&root, &revision).unwrap().blobs);
        let blob = git(&root, &["hash-object", "backend/bluets/src/lib.rs"]).unwrap();
        assert!(Snapshot::read(&root, &format!("backend-tree:{}", blob.trim())).is_err());
        assert!(read(&root, "HEAD").is_err());
        assert!(git(&root, &["rev-parse", "--verify", "HEAD"]).is_err());
        fs::write(&source, "// changed\n").unwrap();
        assert!(read(&root, &revision).is_err());
        git(&root, &["add", "--", "backend"]).unwrap();
        assert!(read(&root, &revision).is_err());
        fs::write(&source, "// original\n").unwrap();
        git(&root, &["add", "--", "backend"]).unwrap();
        assert!(read(&root, &revision).is_ok());
        fs::write(directory.join("new.rs"), "// untracked\n").unwrap();
        assert!(read(&root, &revision).is_err());
        // The immutable snapshot remains the original even after worktree edits.
        assert_eq!(
            Snapshot::read(&root, &revision).unwrap().blobs,
            original.blobs
        );
        for invalid in [
            "backend-tree:HEAD",
            "backend-tree:../private",
            "backend-tree:",
        ] {
            assert!(Snapshot::read(&root, invalid).is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }
}
