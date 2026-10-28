// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Revalidate sources, manifests, symlinks and absent resolver candidates.

use super::*;

pub(super) fn fingerprint(graph: &Graph, base: &str, inputs: &[String]) -> Result<String, String> {
    let mut bytes = base.as_bytes().to_vec();
    for input in inputs {
        let path = Path::new(input);
        graph
            .reader
            .authorize_future(path, "resolved build input", false)?;
        let relative = path
            .strip_prefix(&graph.root)
            .map_err(|_| "resolved build input is outside project root")?;
        bytes.extend(relative.to_string_lossy().bytes());
        bytes.push(0);
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => bytes.push(0),
            Err(error) => return Err(format!("cannot inspect resolved build input: {error}")),
            Ok(_) => {
                let canonical = fs::canonicalize(path)
                    .map_err(|error| format!("cannot resolve build input: {error}"))?;
                graph
                    .reader
                    .authorize_future(&canonical, "resolved build input", false)?;
                let relative = canonical
                    .strip_prefix(&graph.root)
                    .map_err(|_| "resolved build input resolves outside project root")?;
                bytes.push(1);
                bytes.extend(relative.to_string_lossy().bytes());
                bytes.push(0);
                let metadata = fs::metadata(&canonical)
                    .map_err(|error| format!("cannot stat resolved build input: {error}"))?;
                if metadata.is_file() {
                    if metadata.len() > 32 * 1024 * 1024 {
                        return Err("resolved build input exceeds byte limit".into());
                    }
                    bytes.push(2);
                    bytes.extend(
                        sha256_label(&fs::read(&canonical).map_err(|error| error.to_string())?)
                            .bytes(),
                    );
                } else if metadata.is_dir() {
                    bytes.push(3);
                } else {
                    return Err("resolved build input is not a regular file or directory".into());
                }
            }
        }
        bytes.push(0);
    }
    Ok(sha256_label(&bytes))
}
