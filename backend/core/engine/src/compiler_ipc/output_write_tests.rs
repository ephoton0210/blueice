// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_bluets::{AuthorizedModule, AuthorizedModuleLoader, CompilerOptions};
use std::path::Path;

fn registration(project: &Path, output: &Path) -> RegisteredProjectRegistration {
    let entry = project.join("main.ts").to_str().unwrap().to_string();
    RegisteredProjectRegistration {
        canonical_project_root: project.to_str().unwrap().to_string(),
        canonical_config_root: project.join("blue-ts.json").to_str().unwrap().to_string(),
        canonical_output_root: output.to_str().unwrap().to_string(),
        entry_module: entry.clone(),
        loader: AuthorizedModuleLoader::new(
            [AuthorizedModule::new(entry, "export const answer = 42;")],
            [],
        )
        .unwrap(),
        compiler_options: CompilerOptions::default(),
    }
}

fn make_project(directory: &Path, name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let project = directory.join(name);
    let output = directory.join(format!("{name}-output"));
    std::fs::create_dir(&project).unwrap();
    std::fs::create_dir(&output).unwrap();
    std::fs::write(project.join("blue-ts.json"), "{}").unwrap();
    std::fs::write(project.join("main.ts"), "export const answer = 42;").unwrap();
    (project, output)
}

#[test]
fn output_write_grant_is_separate_default_denied_and_bound_to_physical_owner_roots() {
    let directory = std::env::temp_dir().join(format!(
        "blueice-owner-output-grant-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let directory = directory.canonicalize().unwrap();
    let (plain_project, plain_output) = make_project(&directory, "plain");
    let (granted_project, granted_output) = make_project(&directory, "granted");
    let mut catalog = CoreCompilerProjectCatalog::default();

    let plain = catalog
        .register_startup_project(registration(&plain_project, &plain_output))
        .unwrap();
    let mut virtual_registration = registration(&granted_project, &granted_output);
    virtual_registration.canonical_output_root = "project:///virtual-output".into();
    assert!(catalog
        .register_startup_project_with_output_write_grant(virtual_registration)
        .is_err());
    assert_eq!(catalog.registered_project_count(), 1);

    let outside_config = directory.join("outside-blue-ts.json");
    std::fs::write(&outside_config, "{}").unwrap();
    let mut escaped_registration = registration(&granted_project, &granted_output);
    escaped_registration.canonical_config_root = outside_config.to_str().unwrap().to_string();
    assert!(catalog
        .register_startup_project_with_output_write_grant(escaped_registration)
        .is_err());
    assert_eq!(catalog.registered_project_count(), 1);

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let alias = directory.join("output-alias");
        symlink(&granted_output, &alias).unwrap();
        assert!(catalog
            .register_startup_project_with_output_write_grant(registration(
                &granted_project,
                &alias
            ))
            .is_err());
        assert_eq!(catalog.registered_project_count(), 1);
    }

    let granted = catalog
        .register_startup_project_with_output_write_grant(registration(
            &granted_project,
            &granted_output,
        ))
        .unwrap();
    let session = catalog.seal();
    let plain_id = RegisteredProjectId::from_wire(plain.id).unwrap();
    let granted_id = RegisteredProjectId::from_wire(granted.id).unwrap();
    assert!(session.output_write_grant(plain_id).is_none());
    assert_eq!(
        session
            .output_write_grant(granted_id)
            .unwrap()
            .canonical_root(),
        granted_output
    );
    assert!(std::fs::read_dir(&granted_output).unwrap().next().is_none());
    std::fs::remove_dir_all(directory).unwrap();
}
