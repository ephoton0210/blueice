// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Platform-neutral Windows path handling for the pinned TypeScript oracle.

use std::ffi::OsStr;
use std::path::PathBuf;

pub(super) fn compiler_program(program: &OsStr, windows: bool) -> PathBuf {
    let mut result = program.to_os_string();
    let display = program.to_string_lossy();
    let basename = display.rsplit(['/', '\\']).next().unwrap_or_default();
    if windows && !basename.contains('.') {
        result.push(".cmd");
    }
    PathBuf::from(result)
}

pub(super) fn diagnostic_location(line: &str) -> Option<(String, usize)> {
    let (prefix, _) = line.split_once("): error TS")?;
    let (path, position) = prefix.rsplit_once('(')?;
    let (line, column) = position.split_once(',')?;
    let line = line.parse().ok()?;
    let _: usize = column.parse().ok()?;
    (!path.is_empty()).then(|| (normalize_path(path), line))
}

fn normalize_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    if let Some(unc) = path.strip_prefix("//?/UNC/") {
        format!("//{unc}")
    } else if let Some(verbatim) = path.strip_prefix("//?/") {
        verbatim.to_string()
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_oracle_uses_the_cmd_shim_without_changing_explicit_programs() {
        assert_eq!(
            compiler_program(OsStr::new("tsc"), true),
            PathBuf::from("tsc.cmd")
        );
        assert_eq!(
            compiler_program(OsStr::new("tsc"), false),
            PathBuf::from("tsc")
        );
        assert_eq!(
            compiler_program(OsStr::new(r"C:\npm.v1\tsc"), true),
            PathBuf::from(r"C:\npm.v1\tsc.cmd")
        );
        assert_eq!(
            compiler_program(OsStr::new(r"\\server\share\tsc.cmd"), true),
            PathBuf::from(r"\\server\share\tsc.cmd")
        );
        assert_eq!(
            compiler_program(OsStr::new(r"\\?\C:\npm\tsc"), true),
            PathBuf::from(r"\\?\C:\npm\tsc.cmd")
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_command_launches_the_selected_cmd_shim() {
        use std::fs;
        use std::process::Command;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("bluets oracle shim {} {nonce}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let stem = directory.join("tsc");
        let path = compiler_program(stem.as_os_str(), true);
        fs::write(&path, b"@echo off\r\necho Version 5.9.3\r\n").unwrap();
        let output = Command::new(&path).arg("--version").output();
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&directory).unwrap();
        let output = output.unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "Version 5.9.3"
        );
    }

    #[test]
    fn drive_path_diagnostic_is_parsed_on_every_os() {
        assert_eq!(
            diagnostic_location(r"C:\work\file.ts(2,3): error TS2322: mismatch"),
            Some(("C:/work/file.ts".into(), 2))
        );
    }

    #[test]
    fn unc_path_diagnostic_is_parsed_on_every_os() {
        assert_eq!(
            diagnostic_location(r"\\server\share\file.ts(12,8): error TS2322: mismatch"),
            Some(("//server/share/file.ts".into(), 12))
        );
    }

    #[test]
    fn verbatim_paths_are_parsed_on_every_os() {
        assert_eq!(
            diagnostic_location(r"\\?\C:\work\file.ts(4,1): error TS2322: mismatch"),
            Some(("C:/work/file.ts".into(), 4))
        );
        assert_eq!(
            diagnostic_location(r"\\?\UNC\server\share\file.ts(5,1): error TS2322: mismatch"),
            Some(("//server/share/file.ts".into(), 5))
        );
    }

    #[test]
    fn malformed_or_global_diagnostic_has_no_file_location() {
        assert_eq!(
            diagnostic_location("error TS18003: No inputs were found"),
            None
        );
        assert_eq!(diagnostic_location("file.ts(x,1): error TS2322"), None);
        assert_eq!(diagnostic_location("file.ts(1,x): error TS2322"), None);
    }
}
