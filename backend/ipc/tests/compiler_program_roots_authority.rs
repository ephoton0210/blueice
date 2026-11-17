// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Adding owner-selected program roots must not add client-selected inputs.

use blueice_ipc::compiler::{
    read_compiler_request, write_compiler_request, CompilerGeneration, CompilerProject,
    CompilerRequest, CompilerWorkSetKind,
};
use serde_json::{json, Value};
use std::io::Cursor;

fn check() -> CompilerRequest {
    CompilerRequest::Check {
        project: CompilerProject { id: 7 },
    }
}

fn assert_refused_without_consuming_the_next_request(value: &Value) {
    let mut bytes = Vec::new();
    let payload = serde_json::to_vec(value).unwrap();
    bytes.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&payload);
    write_compiler_request(&mut bytes, &check()).unwrap();
    let mut stream = Cursor::new(bytes);
    let error = read_compiler_request(&mut stream).unwrap_err();
    let json_error = error
        .get_ref()
        .and_then(|error| error.downcast_ref::<serde_json::Error>())
        .expect("the framed JSON request must fail protocol deserialization");
    assert!(json_error.is_data(), "{value}: {error}");
    assert_eq!(read_compiler_request(&mut stream).unwrap(), check());
    assert_eq!(stream.position(), stream.get_ref().len() as u64);
}

#[test]
fn owner_program_inputs_remain_unavailable_on_every_relevant_query() {
    let generation = CompilerGeneration {
        project: CompilerProject { id: 7 },
        sequence: 3,
    };
    for (operation, request) in [
        ("Check", check()),
        (
            "DescribeProject",
            CompilerRequest::DescribeProject {
                project: generation.project,
            },
        ),
        (
            "ListWorkSet",
            CompilerRequest::ListWorkSet {
                generation,
                kind: CompilerWorkSetKind::Rechecked,
                cursor: None,
                limit: Some(2),
            },
        ),
        (
            "GetStaticType",
            CompilerRequest::GetStaticType {
                generation,
                type_id: 1,
            },
        ),
    ] {
        let original = serde_json::to_value(&request).unwrap();
        assert_eq!(
            serde_json::from_value::<CompilerRequest>(original.clone()).unwrap(),
            request
        );
        for field in [
            "roots",
            "program_roots",
            "selected_roots",
            "entry_module",
            "loader",
            "source",
            "sources",
            "resolver",
            "resolver_edges",
            "compiler_options",
            "options",
            "output_root",
            "output_path",
            "write_capability",
        ] {
            let mut value = original.clone();
            value[operation]
                .as_object_mut()
                .unwrap()
                .insert(field.into(), json!(["project:///outside/secret.ts"]));
            assert_refused_without_consuming_the_next_request(&value);
        }
    }
}

#[test]
fn opaque_project_and_generation_handles_cannot_carry_program_authority() {
    for field in ["roots", "program_roots", "path", "source", "resolver"] {
        let mut project = json!({"Check":{"project":{"id":7}}});
        project["Check"]["project"]
            .as_object_mut()
            .unwrap()
            .insert(field.into(), json!("project:///outside/secret.ts"));
        assert_refused_without_consuming_the_next_request(&project);

        let mut generation = json!({"GetStaticType":{
            "generation":{"project":{"id":7},"sequence":3},"type_id":1
        }});
        generation["GetStaticType"]["generation"]
            .as_object_mut()
            .unwrap()
            .insert(field.into(), json!(["project:///outside/secret.ts"]));
        assert_refused_without_consuming_the_next_request(&generation);

        let mut nested = json!({"GetStaticType":{
            "generation":{"project":{"id":7},"sequence":3},"type_id":1
        }});
        nested["GetStaticType"]["generation"]["project"]
            .as_object_mut()
            .unwrap()
            .insert(field.into(), json!("project:///outside/secret.ts"));
        assert_refused_without_consuming_the_next_request(&nested);
    }
}

#[test]
fn the_query_protocol_has_no_program_registration_or_mutation_operation() {
    for operation in [
        "RegisterProgram",
        "RegisterProject",
        "UpdateProgramRoots",
        "AddRoot",
        "RemoveRoot",
        "SetSource",
        "SetResolver",
        "SetCompilerOptions",
        "ReadSource",
        "WriteOutput",
    ] {
        let value = json!({(operation):{"project":{"id":7},"roots":["project:///outside.ts"]}});
        assert!(value.get(operation).is_some());
        assert_refused_without_consuming_the_next_request(&value);
    }
}
