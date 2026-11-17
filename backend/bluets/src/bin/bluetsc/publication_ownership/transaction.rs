// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Stage the complete file plan and roll back a failed multi-file installation.

use super::*;

struct Installed {
    path: PathBuf,
    backup: Option<PathBuf>,
    published: bool,
}

pub(super) fn publish(
    root: &Path,
    writes: BTreeMap<PathBuf, Vec<u8>>,
    retired: Vec<PathBuf>,
    commit: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let mut plan = writes
        .into_iter()
        .map(|(path, bytes)| (path, Some(bytes)))
        .collect::<Vec<_>>();
    plan.extend(retired.into_iter().map(|path| (path, None)));
    for (path, _) in &plan {
        let name = path
            .strip_prefix(root)
            .map_err(|_| io::Error::other("publication leaves owner root"))?;
        checked_path(root, name)?;
        match fs::symlink_metadata(path) {
            Ok(metadata) if !metadata.is_file() => {
                return Err(io::Error::other("output destination is not a regular file"))
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stage = root.join(format!(".bluetsc-publish-{}-{nonce}", std::process::id()));
    fs::create_dir(&stage)?;
    let mut installed = Vec::<Installed>::new();
    let result = (|| {
        for (index, (_, bytes)) in plan.iter().enumerate() {
            if let Some(bytes) = bytes {
                fs::write(stage.join(format!("new-{index}")), bytes)?;
            }
        }
        for (index, (path, bytes)) in plan.iter().enumerate() {
            checked_path(
                root,
                path.strip_prefix(root).expect("validated destination"),
            )?;
            let backup = if path.exists() {
                let backup = stage.join(format!("old-{index}"));
                fs::rename(path, &backup)?;
                Some(backup)
            } else {
                None
            };
            installed.push(Installed {
                path: path.clone(),
                backup,
                published: false,
            });
            if bytes.is_some() {
                fs::create_dir_all(path.parent().expect("output parent"))?;
                fs::rename(stage.join(format!("new-{index}")), path)?;
                installed
                    .last_mut()
                    .expect("recorded installation")
                    .published = true;
            }
        }
        // Private state may live on a different filesystem. Its own atomic
        // commit happens last while every public backup is still available.
        commit()
    })();
    if let Err(error) = result {
        let restored = (|| -> io::Result<()> {
            for entry in installed.into_iter().rev() {
                if entry.published {
                    fs::remove_file(&entry.path)?;
                }
                if let Some(backup) = entry.backup {
                    fs::rename(backup, entry.path)?;
                }
            }
            Ok(())
        })();
        if let Err(restore) = restored {
            return Err(io::Error::other(format!(
                "publication failed: {error}; rollback failed: {restore}; backups retained at {}",
                stage.display()
            )));
        }
        let _ = fs::remove_dir_all(stage);
        return Err(error);
    }
    fs::remove_dir_all(stage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_private_commit_restores_overwritten_retired_and_new_outputs() {
        let root = env::temp_dir().join(format!(
            "bluets-transaction-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("active.js"), b"previous active").unwrap();
        fs::write(root.join("retired.js"), b"previous retired").unwrap();
        fs::write(root.join("user.js"), b"user sentinel").unwrap();
        let writes = BTreeMap::from([
            (root.join("active.js"), b"new active".to_vec()),
            (root.join("created.js"), b"new output".to_vec()),
        ]);
        let result = publish(&root, writes, vec![root.join("retired.js")], || {
            assert_eq!(fs::read(root.join("active.js")).unwrap(), b"new active");
            assert!(!root.join("retired.js").exists());
            Err(io::Error::other("private state commit refused"))
        });
        assert!(result.is_err());
        assert_eq!(
            fs::read(root.join("active.js")).unwrap(),
            b"previous active"
        );
        assert_eq!(
            fs::read(root.join("retired.js")).unwrap(),
            b"previous retired"
        );
        assert_eq!(fs::read(root.join("user.js")).unwrap(), b"user sentinel");
        assert!(!root.join("created.js").exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 3);
        fs::remove_dir_all(root).unwrap();
    }
}
