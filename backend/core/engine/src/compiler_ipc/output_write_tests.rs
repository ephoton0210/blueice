// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_bluets::{
    AuthorizedModule, AuthorizedModuleLoader, CompilerOptions, EmittedRuntimeSite,
    EmittedStrictModule, SourceSpan,
};
use std::path::Path;

fn registration(project: &Path, output: &Path) -> RegisteredProjectRegistration {
    registration_with_source(project, output, "export const answer = 42;")
}

fn registration_with_source(
    project: &Path,
    output: &Path,
    source: &str,
) -> RegisteredProjectRegistration {
    let entry = project.join("main.ts").to_str().unwrap().to_string();
    RegisteredProjectRegistration {
        canonical_project_root: project.to_str().unwrap().to_string(),
        canonical_config_root: project.join("blue-ts.json").to_str().unwrap().to_string(),
        canonical_output_root: output.to_str().unwrap().to_string(),
        entry_module: entry.clone(),
        loader: AuthorizedModuleLoader::new([AuthorizedModule::new(entry, source)], []).unwrap(),
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
fn output_owner_inventories_only_exposed_grants_and_requires_its_own_receipt() {
    let directory = std::env::temp_dir().join(format!(
        "blueice-output-owner-session-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let directory = directory.canonicalize().unwrap();
    let (plain_root, plain_output) = make_project(&directory, "plain");
    let (granted_root, granted_output) = make_project(&directory, "granted");
    let (private_root, private_output) = make_project(&directory, "private");
    let (invalid_root, invalid_output) = make_project(&directory, "invalid");
    let invalid_source = "export const answer: number = 'wrong';";
    std::fs::write(invalid_root.join("main.ts"), invalid_source).unwrap();
    let mut catalog = CoreCompilerProjectCatalog::default();
    let plain = catalog
        .register_startup_project(registration(&plain_root, &plain_output))
        .unwrap();
    let granted = catalog
        .register_startup_project_with_output_write_grant(registration(
            &granted_root,
            &granted_output,
        ))
        .unwrap();
    let private = catalog
        .register_startup_project_private_with_output_write_grant(registration(
            &private_root,
            &private_output,
        ))
        .unwrap();
    let invalid = catalog
        .register_startup_project_with_output_write_grant(registration_with_source(
            &invalid_root,
            &invalid_output,
            invalid_source,
        ))
        .unwrap();
    let mut owner = catalog.seal();
    let receipt = CompilerOutputSessionReceipt {
        id: format!("ow-{}", "a1".repeat(32)),
    };
    let other = CompilerOutputSessionReceipt {
        id: format!("ow-{}", "b2".repeat(32)),
    };
    let build = |receipt: &CompilerOutputSessionReceipt, project| CompilerOutputRequest::Build {
        receipt: receipt.clone(),
        project,
    };
    assert!(matches!(
        owner.handle_output_request(&receipt, build(&receipt, granted)),
        CompilerOutputReply::Error {
            code: CompilerOutputErrorCode::UnobservedProject,
            ..
        }
    ));
    assert!(matches!(
        owner.handle_output_request(
            &receipt,
            CompilerOutputRequest::ListProjects {
                receipt: CompilerOutputSessionReceipt {
                    id: "a1".repeat(32),
                },
            },
        ),
        CompilerOutputReply::Error {
            code: CompilerOutputErrorCode::InvalidReceipt,
            ..
        }
    ));
    let CompilerOutputReply::Projects(inventory) = owner.handle_output_request(
        &receipt,
        CompilerOutputRequest::ListProjects {
            receipt: receipt.clone(),
        },
    ) else {
        panic!("output owner must list only its granted public projects")
    };
    assert_eq!(inventory.projects, vec![granted, invalid]);
    for denied in [plain, private] {
        assert!(matches!(
            owner.handle_output_request(&receipt, build(&receipt, denied)),
            CompilerOutputReply::Error {
                code: CompilerOutputErrorCode::UnobservedProject,
                ..
            }
        ));
    }
    assert!(matches!(
        owner.handle_output_request(&other, build(&receipt, granted)),
        CompilerOutputReply::Error {
            code: CompilerOutputErrorCode::InvalidReceipt,
            ..
        }
    ));
    let CompilerReply::Check(prior_check) = owner
        .adapter
        .handle(CompilerRequest::Check { project: granted })
    else {
        panic!("the query owner must check its exposed project")
    };
    let CompilerOutputReply::Build(result) =
        owner.handle_output_request(&receipt, build(&receipt, granted))
    else {
        panic!("an inventoried grant must publish")
    };
    assert!(result.is_well_formed_for_project(granted));
    assert!(result.published);
    assert_ne!(result.generation, prior_check.generation);
    assert!(matches!(
        owner.adapter.handle(CompilerRequest::ListDiagnostics {
            generation: prior_check.generation,
            cursor: None,
            limit: Some(1),
        }),
        CompilerReply::Error {
            code: CompilerErrorCode::StaleGeneration,
            ..
        }
    ));
    assert_eq!(std::fs::read_dir(&granted_output).unwrap().count(), 1);
    let CompilerOutputReply::Build(diagnostic) =
        owner.handle_output_request(&receipt, build(&receipt, invalid))
    else {
        panic!("a diagnostic build must return its no-output result")
    };
    assert!(diagnostic.has_errors);
    assert!(!diagnostic.published);
    assert_eq!(std::fs::read_dir(&invalid_output).unwrap().count(), 0);
    owner.end_output_session(&receipt);
    assert!(matches!(
        owner.handle_output_request(&receipt, build(&receipt, granted)),
        CompilerOutputReply::Error {
            code: CompilerOutputErrorCode::UnobservedProject,
            ..
        }
    ));
    std::fs::remove_dir_all(directory).unwrap();
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

#[test]
fn owner_staging_publishes_one_complete_generation_and_cleans_failed_stages() {
    let directory = std::env::temp_dir().join(format!(
        "blueice-owner-output-stage-{}-{}",
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
    let granted = catalog
        .register_startup_project_with_output_write_grant(registration(
            &granted_project,
            &granted_output,
        ))
        .unwrap();
    let mut session = catalog.seal();
    let plain_id = RegisteredProjectId::from_wire(plain.id).unwrap();
    let granted_id = RegisteredProjectId::from_wire(granted.id).unwrap();
    let plain_generation = session.adapter.service.check(plain_id).unwrap().generation;
    assert_eq!(
        session
            .stage_owner_output(plain_generation, |_| Ok(()))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::PermissionDenied
    );
    assert!(std::fs::read_dir(&plain_output).unwrap().next().is_none());

    let granted_generation = session
        .adapter
        .service
        .check(granted_id)
        .unwrap()
        .generation;
    let published = session
        .stage_owner_output(granted_generation, |stage| {
            std::fs::write(stage.join("main.js"), "export const answer = 42;")
        })
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(published.join("main.js")).unwrap(),
        "export const answer = 42;"
    );
    assert_eq!(
        session
            .stage_owner_output(granted_generation, |_| Ok(()))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::AlreadyExists
    );

    let next_generation = session
        .adapter
        .service
        .check(granted_id)
        .unwrap()
        .generation;
    assert_eq!(
        session
            .stage_owner_output(granted_generation, |_| Ok(()))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidInput
    );
    assert!(session
        .stage_owner_output(next_generation, |stage| {
            std::fs::write(stage.join("partial.js"), "partial")?;
            Err(std::io::Error::other("populate failed"))
        })
        .is_err());
    let names = std::fs::read_dir(&granted_output)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![published
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()]
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let moved_root = directory.join("moved-output");
        let outside = directory.join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::rename(&granted_output, &moved_root).unwrap();
        symlink(&outside, &granted_output).unwrap();
        assert_eq!(
            session
                .stage_owner_output(next_generation, |_| Ok(()))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
        std::fs::remove_file(&granted_output).unwrap();
        std::fs::rename(moved_root, &granted_output).unwrap();

        let original_root = directory.join("original-output");
        std::fs::rename(&granted_output, &original_root).unwrap();
        std::fs::create_dir(&granted_output).unwrap();
        assert_eq!(
            session
                .stage_owner_output(next_generation, |_| Ok(()))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        std::fs::remove_dir(&granted_output).unwrap();
        std::fs::rename(original_root, &granted_output).unwrap();
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn diagnostic_build_does_not_enter_staging_or_replace_prior_output() {
    let directory = std::env::temp_dir().join(format!(
        "blueice-owner-build-no-emit-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let directory = directory.canonicalize().unwrap();
    let (plain_project, plain_output) = make_project(&directory, "plain");
    let (valid_project, valid_output) = make_project(&directory, "valid");
    let (invalid_project, invalid_output) = make_project(&directory, "invalid");
    let invalid_source = "export const answer: number = 'wrong';";
    std::fs::write(invalid_project.join("main.ts"), invalid_source).unwrap();
    std::fs::write(invalid_output.join("keep.txt"), "unchanged").unwrap();
    let mut catalog = CoreCompilerProjectCatalog::default();
    let plain = catalog
        .register_startup_project(registration(&plain_project, &plain_output))
        .unwrap();
    let mut valid_registration = registration(&valid_project, &valid_output);
    valid_registration.compiler_options.source_map = true;
    valid_registration.compiler_options.declaration = true;
    let valid = catalog
        .register_startup_project_with_output_write_grant(valid_registration)
        .unwrap();
    let invalid = catalog
        .register_startup_project_with_output_write_grant(registration_with_source(
            &invalid_project,
            &invalid_output,
            invalid_source,
        ))
        .unwrap();
    let mut session = catalog.seal();
    let plain_id = RegisteredProjectId::from_wire(plain.id).unwrap();
    assert!(matches!(
        session.build_and_stage_owner_output(plain_id),
        Err(OwnerBuildStageError::NotGranted)
    ));
    assert_eq!(
        session
            .adapter
            .service
            .check(plain_id)
            .unwrap()
            .generation
            .sequence(),
        1,
        "a denied build must not advance the compiler generation"
    );

    let invalid_id = RegisteredProjectId::from_wire(invalid.id).unwrap();
    let (check, published) = session.build_and_stage_owner_output(invalid_id).unwrap();
    assert!(check.has_errors);
    assert!(published.is_none());
    assert_eq!(
        std::fs::read_to_string(invalid_output.join("keep.txt")).unwrap(),
        "unchanged"
    );
    assert_eq!(std::fs::read_dir(&invalid_output).unwrap().count(), 1);

    let valid_id = RegisteredProjectId::from_wire(valid.id).unwrap();
    let (check, published) = session.build_and_stage_owner_output(valid_id).unwrap();
    assert!(!check.has_errors);
    let published = published.unwrap();
    assert!(std::fs::read_to_string(published.join("main.js"))
        .unwrap()
        .contains("answer"));
    assert!(published.join("main.js.map").exists());
    assert!(published.join("main.d.ts").exists());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn artifact_plan_rejects_sources_outside_the_project_aliases_and_output_collisions() {
    let directory = std::env::temp_dir().join(format!(
        "blueice-owner-artifact-paths-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let directory = directory.canonicalize().unwrap();
    let (project, output_root) = make_project(&directory, "project");
    let root = project.to_str().unwrap();
    let mut service = RegisteredProjectCompilerService::default();
    let id = service
        .register(registration(&project, &output_root))
        .unwrap();
    let output = service.build(id).unwrap().output.unwrap();
    assert!(output_artifacts::plan(root, &output).is_ok());

    let outside = directory.join("outside.ts");
    std::fs::write(&outside, "export const outside = 1;").unwrap();
    let mut escaped = output.clone();
    let (_, mut artifact) = escaped.artifacts.pop_first().unwrap();
    artifact.module_id = outside.to_str().unwrap().to_string();
    escaped
        .artifacts
        .insert(artifact.module_id.clone(), artifact);
    assert!(output_artifacts::plan(root, &escaped).is_err());

    std::fs::create_dir(project.join("nested")).unwrap();
    // Preserve the spelling under test: joining onto a Windows verbatim path
    // can normalize away `..` before the planner receives it.
    let separator = std::path::MAIN_SEPARATOR;
    let aliased_root = format!("{root}{separator}nested{separator}..");
    assert_eq!(
        output_artifacts::plan(&aliased_root, &output)
            .err()
            .unwrap()
            .kind(),
        std::io::ErrorKind::InvalidInput
    );
    let alias = format!("{root}{separator}nested{separator}..{separator}main.ts");
    let mut aliased = output.clone();
    let (_, mut artifact) = aliased.artifacts.pop_first().unwrap();
    artifact.module_id = alias;
    aliased
        .artifacts
        .insert(artifact.module_id.clone(), artifact);
    assert_eq!(
        output_artifacts::plan(root, &aliased).err().unwrap().kind(),
        std::io::ErrorKind::InvalidInput
    );

    #[cfg(windows)]
    {
        let mut aliased = output.clone();
        let (_, mut artifact) = aliased.artifacts.pop_first().unwrap();
        artifact.module_id = format!(r"{root}\nested\..\main.ts");
        aliased
            .artifacts
            .insert(artifact.module_id.clone(), artifact);
        assert_eq!(
            output_artifacts::plan(root, &aliased).err().unwrap().kind(),
            std::io::ErrorKind::InvalidInput
        );
    }

    let declaration_id = project.join("main.d.ts");
    std::fs::write(&declaration_id, "declare const answer: number;").unwrap();
    let mut colliding = output.clone();
    colliding.artifacts.values_mut().next().unwrap().declaration =
        Some("declare const answer: number;".into());
    colliding.declaration_modules.insert(
        declaration_id.to_str().unwrap().to_string(),
        "declare const answer: number;".into(),
    );
    assert!(output_artifacts::plan(root, &colliding).is_err());

    let file_source = project.join("foo.ts");
    let nested_directory = project.join("foo.js");
    std::fs::write(&file_source, "export const foo = 1;").unwrap();
    std::fs::create_dir(&nested_directory).unwrap();
    let nested_source = nested_directory.join("bar.ts");
    std::fs::write(&nested_source, "export const bar = 1;").unwrap();
    let mut nested_collision = colliding;
    nested_collision.declaration_modules.clear();
    let mut prototype = nested_collision.artifacts.values().next().unwrap().clone();
    prototype.declaration = None;
    prototype.module_id = file_source.to_str().unwrap().to_string();
    nested_collision
        .artifacts
        .insert(prototype.module_id.clone(), prototype.clone());
    prototype.module_id = nested_source.to_str().unwrap().to_string();
    nested_collision
        .artifacts
        .insert(prototype.module_id.clone(), prototype);
    assert!(output_artifacts::plan(root, &nested_collision).is_err());

    let mut strict = output;
    strict.artifacts.values_mut().next().unwrap().strict_runtime = Some(EmittedStrictModule {
        helper_version: "bluets-runtime-helper-v1".into(),
        helper_import: EmittedRuntimeSite {
            source_span: SourceSpan::new(root, 0, 1),
            generated_start: 0,
            expected_text: "import helper".into(),
        },
        boundaries: Vec::new(),
    });
    assert!(output_artifacts::plan(root, &strict).is_err());
    assert!(std::fs::read_dir(&output_root).unwrap().next().is_none());
    std::fs::remove_dir_all(directory).unwrap();
}
