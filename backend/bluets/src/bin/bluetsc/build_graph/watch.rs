// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded owner-local polling for `--build --watch`.

use super::*;
use std::io::{BufRead, Write};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(50);
const QUIET: Duration = Duration::from_millis(180);
const MAX_DEPTH: usize = 64;
const MAX_FILES: usize = 8192;
const MAX_BYTES: u64 = 64 * 1024 * 1024;

pub(super) fn run(
    project: &Path,
    pretty: bool,
    verbose: bool,
    stop_build_on_errors: bool,
    diagnostics_json: bool,
) -> ExitCode {
    let root = project.parent().expect("canonical project has parent");
    let stop = stop_channel();
    let mut cycle = 0usize;
    let result = super::build_once(
        project,
        pretty,
        verbose,
        stop_build_on_errors,
        diagnostics_json,
    );
    let mut baseline = match snapshot(root) {
        Ok(snapshot) => snapshot,
        Err(message) => {
            eprintln!("bluetsc: watch scan failed: {message}");
            return ExitCode::FAILURE;
        }
    };
    announce(cycle, result);
    loop {
        if should_stop(&stop) {
            println!("Watch stopped");
            return ExitCode::SUCCESS;
        }
        thread::sleep(POLL);
        let current = match snapshot(root) {
            Ok(snapshot) => snapshot,
            Err(message) => {
                eprintln!("bluetsc: watch scan failed: {message}");
                return ExitCode::FAILURE;
            }
        };
        if current == baseline {
            continue;
        }
        let mut pending = current;
        let mut quiet_until = Instant::now() + QUIET;
        while Instant::now() < quiet_until {
            if should_stop(&stop) {
                println!("Watch stopped");
                return ExitCode::SUCCESS;
            }
            thread::sleep(POLL);
            let next = match snapshot(root) {
                Ok(snapshot) => snapshot,
                Err(message) => {
                    eprintln!("bluetsc: watch scan failed: {message}");
                    return ExitCode::FAILURE;
                }
            };
            if next != pending {
                pending = next;
                quiet_until = Instant::now() + QUIET;
            }
        }
        println!("File change detected; starting build");
        cycle += 1;
        let result = super::build_once(
            project,
            pretty,
            verbose,
            stop_build_on_errors,
            diagnostics_json,
        );
        baseline = match snapshot(root) {
            Ok(snapshot) => snapshot,
            Err(message) => {
                eprintln!("bluetsc: watch scan failed: {message}");
                return ExitCode::FAILURE;
            }
        };
        announce(cycle, result);
    }
}

fn announce(cycle: usize, result: ExitCode) {
    if result == ExitCode::SUCCESS {
        println!("Watch build {cycle} complete");
    } else {
        println!("Watch build {cycle} completed with errors");
    }
    let _ = io::stdout().flush();
}

fn stop_channel() -> Receiver<()> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) if line.trim() == "q" => {
                    let _ = sender.send(());
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
    receiver
}

fn should_stop(stop: &Receiver<()>) -> bool {
    matches!(stop.try_recv(), Ok(()))
}

fn snapshot(root: &Path) -> Result<String, String> {
    snapshot_with(root, |_, _| {})
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanPoint {
    Directory,
    Metadata,
    Read,
}

fn snapshot_with(root: &Path, mut before: impl FnMut(ScanPoint, &Path)) -> Result<String, String> {
    let mut records = BTreeMap::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut bytes = 0u64;
    while let Some((directory, depth)) = pending.pop() {
        if depth > MAX_DEPTH {
            return Err("project directory depth exceeds watch limit".into());
        }
        before(ScanPoint::Directory, &directory);
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound && directory != root => continue,
            Err(error) => return Err(format!("cannot inspect {}: {error}", directory.display())),
        };
        for entry in entries {
            let Some(entry) = present("inspect watch entry", &directory, entry)? else {
                continue;
            };
            let path = entry.path();
            let name = entry.file_name();
            before(ScanPoint::Metadata, &path);
            let Some(metadata) = present("inspect", &path, fs::symlink_metadata(&path))? else {
                continue;
            };
            if metadata.is_dir() {
                if name != ".git" && name != "target" {
                    pending.push((path, depth + 1));
                }
                continue;
            }
            if path.to_string_lossy().ends_with(".tsbuildinfo") {
                continue;
            }
            if records.len() >= MAX_FILES {
                return Err("project file count exceeds watch limit".into());
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "watch entry is outside project root".to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            if metadata.file_type().is_symlink() {
                before(ScanPoint::Read, &path);
                let Some(target) = present("inspect symlink", &path, fs::read_link(&path))? else {
                    continue;
                };
                records.insert(relative, format!("link:{}", target.display()));
            } else if metadata.is_file() {
                bytes = bytes.saturating_add(metadata.len());
                if bytes > MAX_BYTES {
                    return Err("project bytes exceed watch limit".into());
                }
                before(ScanPoint::Read, &path);
                let Some(contents) = present("read", &path, fs::read(&path))? else {
                    continue;
                };
                records.insert(relative, sha256_label(&contents));
            }
        }
    }
    Ok(sha256_label(
        serde_json::to_vec(&records)
            .expect("bounded watch snapshot is serializable")
            .as_slice(),
    ))
}

fn present<T>(operation: &str, path: &Path, result: io::Result<T>) -> Result<Option<T>, String> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("cannot {operation} {}: {error}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_deletion_io_errors_and_missing_owner_roots_remain_errors() {
        let missing = std::env::temp_dir().join(format!(
            "bluets-watch-missing-root-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for kind in [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::InvalidData,
            io::ErrorKind::Other,
        ] {
            assert!(present::<()>("read", &missing, Err(io::Error::from(kind))).is_err());
        }
        assert!(snapshot(&missing).is_err());
    }

    #[test]
    fn deleted_watch_entries_are_absent_at_each_scan_boundary() {
        let root = std::env::temp_dir().join(format!(
            "bluets-watch-scan-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let expected = snapshot(&root).unwrap();
        for point in [ScanPoint::Directory, ScanPoint::Metadata, ScanPoint::Read] {
            let directory = root.join("nested");
            let file = directory.join("entry.ts");
            fs::create_dir_all(&directory).unwrap();
            fs::write(&file, "export const value = 42;").unwrap();
            let mut removed = false;
            let actual = snapshot_with(&root, |step, path| {
                let selected = if point == ScanPoint::Directory {
                    &directory
                } else {
                    &file
                };
                if !removed && step == point && path == selected {
                    if point == ScanPoint::Directory {
                        fs::remove_dir_all(path).unwrap();
                    } else {
                        fs::remove_file(path).unwrap();
                    }
                    removed = true;
                }
            });
            assert!(removed);
            assert_eq!(actual.unwrap(), expected);
        }
        fs::remove_dir_all(root).unwrap();
    }
}
