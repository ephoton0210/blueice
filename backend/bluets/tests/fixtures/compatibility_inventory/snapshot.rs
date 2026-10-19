// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Read one immutable Git tree; never mix an index with working-tree files.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[path = "staged_snapshot.rs"]
mod staged_snapshot;

const SOURCE_ROOTS: [&str; 4] = [
    "backend/bluets/src",
    "backend/bluets/tests",
    "backend/bluets-bluejs/src",
    "backend/bluets-bluejs/tests",
];

pub fn valid_revision(revision: &str) -> bool {
    let hash = revision.strip_prefix("backend-tree:").unwrap_or(revision);
    hash.len() == 40 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn excluded(path: &str) -> bool {
    path.starts_with("backend/bluets/tests/fixtures/compatibility_inventory/")
        || path == "backend/bluets/tests/compatibility_inventory.rs"
}

pub struct Snapshot {
    pub revision: String,
    pub blobs: BTreeMap<String, String>,
    sources: BTreeMap<String, String>,
}

impl Snapshot {
    pub fn read(root: &Path, revision: &str) -> Result<Self> {
        let backend_tree = revision.starts_with("backend-tree:");
        let (revision, object) = if let Some(hash) = revision.strip_prefix("backend-tree:") {
            if !valid_revision(revision) || git(root, &["cat-file", "-t", hash])?.trim() != "tree" {
                return Err("invalid immutable backend tree".into());
            }
            (revision.to_string(), hash.to_string())
        } else {
            let commit = git(
                root,
                &["rev-parse", "--verify", &format!("{revision}^{{commit}}")],
            )?
            .trim()
            .to_string();
            (commit.clone(), commit)
        };
        let paths = SOURCE_ROOTS.map(|path| {
            if backend_tree {
                path.strip_prefix("backend/").unwrap()
            } else {
                path
            }
        });
        let mut args = vec!["ls-tree", "-r", "-z", "--full-tree", &object, "--"];
        args.extend(paths);
        let tree = git(root, &args)?;
        let mut blobs = BTreeMap::new();
        for line in tree.split('\0').filter(|line| !line.is_empty()) {
            let (metadata, path) = line.split_once('\t').ok_or("invalid git ls-tree row")?;
            let fields: Vec<_> = metadata.split_whitespace().collect();
            if fields.len() != 3 || fields[0] != "100644" || fields[1] != "blob" {
                return Err(format!("unexpected tree entry: {line}").into());
            }
            let path = if backend_tree {
                format!("backend/{path}")
            } else {
                path.to_string()
            };
            // The inventory's own tests are not pinned compiler oracle inputs.
            if excluded(&path) {
                continue;
            }
            blobs.insert(path.to_string(), fields[2].to_string());
        }
        let selected: Vec<_> = blobs
            .iter()
            .filter(|(path, _)| {
                path.contains("/tests/")
                    && [".rs", ".json", ".tsv", ".cjs", ".txt"]
                        .iter()
                        .any(|extension| path.ends_with(extension))
            })
            .collect();
        // Batch avoids a subprocess for every fixture. The tree, not user input,
        // supplies each object id; read the whole pipe while the writer runs.
        let mut child = Command::new("git")
            .current_dir(root)
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut input = child.stdin.take().ok_or("missing cat-file stdin")?;
        let requests = selected
            .iter()
            .map(|(_, blob)| format!("{blob}\n"))
            .collect::<String>();
        let writer = std::thread::spawn(move || input.write_all(requests.as_bytes()));
        let output = child.wait_with_output()?;
        writer.join().map_err(|_| "cat-file writer panicked")??;
        if !output.status.success() {
            return Err(String::from_utf8(output.stderr)?.into());
        }
        let sources = decode_batch(&output.stdout, &selected)?;
        Ok(Self {
            revision,
            blobs,
            sources,
        })
    }

    pub fn read_staged(root: &Path, revision: &str) -> Result<Self> {
        staged_snapshot::read(root, revision)
    }

    pub fn source(&self, path: &str) -> Result<&str> {
        self.sources
            .get(path)
            .map(String::as_str)
            .ok_or_else(|| format!("source not in snapshot: {path}").into())
    }
}

pub fn decode_batch(
    bytes: &[u8],
    selected: &[(&String, &String)],
) -> Result<BTreeMap<String, String>> {
    let mut sources = BTreeMap::new();
    let mut offset = 0;
    for (path, expected) in selected {
        let newline = bytes[offset..]
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or("truncated cat-file header")?
            + offset;
        let header = std::str::from_utf8(&bytes[offset..newline])?;
        let fields: Vec<_> = header.split_whitespace().collect();
        if fields.len() != 3 || fields[0] != expected.as_str() || fields[1] != "blob" {
            return Err(format!("invalid cat-file response: {header}").into());
        }
        let length: usize = fields[2].parse()?;
        offset = newline + 1;
        let end = offset
            .checked_add(length)
            .ok_or("cat-file length overflow")?;
        let body = bytes.get(offset..end).ok_or("truncated cat-file body")?;
        sources.insert((*path).clone(), String::from_utf8(body.to_vec())?);
        if bytes.get(end) != Some(&b'\n') {
            return Err("missing cat-file separator".into());
        }
        offset = end + 1;
    }
    if offset != bytes.len() {
        return Err("extra cat-file output".into());
    }
    Ok(sources)
}

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    if !output.status.success() {
        return Err(String::from_utf8(output.stderr)?.into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

/// Resolve only repository-relative paths. Reject escapes instead of reading
/// arbitrary host files when a harness module/include changes.
pub fn relative(parent: &str, child: &str) -> Result<String> {
    let mut parts: Vec<_> = parent.split('/').filter(|part| !part.is_empty()).collect();
    for part in child.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop().ok_or("path escapes repository")?;
            }
            part if !part.contains('\\') && !part.contains(':') => parts.push(part),
            _ => return Err("non-portable repository path".into()),
        }
    }
    if child.starts_with('/') {
        return Err("absolute repository path".into());
    }
    Ok(parts.join("/"))
}
