// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn immutable_core_authorizer_routes_external_javascript_and_bluets_graphs_to_real_child() {
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
            concat!(
                "<script src=\"/assets/external.js\"></script>",
                "<script type=\"application/x-blueice-typescript-module\" src=\"/assets/external.ts\"></script>",
                "<script>if (globalThis.externalJavaScript !== 41) throw new Error('order');</script>"
            ),
            "https://example.test/app/index.html",
        );
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
        &path,
        &token,
        ExternalGraphsAuthorizer {
            requests: Arc::clone(&requests),
        },
    )
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();

    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: BlueJsPageScriptKind::Classic,
            },
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 2,
                kind: BlueJsPageScriptKind::Classic,
            },
        ]
    );
    assert_eq!(
        executor.drain_blue_ts_reports_for_tab(tab_id),
        vec![BlueTsPageExecutionReport::Executed {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            ordinal: 1,
            kind: DirectPageScriptKind::Module,
        }]
    );
    assert_eq!(
        requests.lock().unwrap().as_slice(),
        [
            OutOfProcessPageScriptSourceRequest {
                tab_id,
                document_generation: 1,
                ordinal: 0,
                language: CombinedPageScriptLanguage::JavaScript(BlueJsPageScriptKind::Classic,),
                document_url: "https://example.test/app/index.html".to_string(),
                declared_src: "/assets/external.js".to_string(),
            },
            OutOfProcessPageScriptSourceRequest {
                tab_id,
                document_generation: 1,
                ordinal: 1,
                language: CombinedPageScriptLanguage::BlueTs(DirectPageScriptKind::Module),
                document_url: "https://example.test/app/index.html".to_string(),
                declared_src: "/assets/external.ts".to_string(),
            },
        ]
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn startup_http_authorizer_builds_closed_classic_module_and_bluets_graphs_source_free() {
    let classic = "globalThis.externalClassic = (globalThis.externalClassic || 40) + 1;";
    let javascript_entry =
        "import { answer } from './answer.js'; globalThis.externalModule = answer;";
    let javascript_dependency = "export const answer = 42;";
    let bluets_entry =
        "import { answer } from './answer.ts'; export const result: number = answer;";
    let bluets_dependency = "export const answer: number = 41;";
    let integrity_mismatch = "globalThis.redactedIntegrityMarker = 'private bytes';";
    let bad_mime = "globalThis.redactedMimeMarker = 'private bytes';";
    let mut responses = BTreeMap::new();
    responses.insert(
        "/assets/classic.js".to_string(),
        HttpTestResponse::script("text/javascript; charset=utf-8", classic),
    );
    responses.insert(
        "/assets/main.js".to_string(),
        HttpTestResponse::script("application/javascript", javascript_entry),
    );
    responses.insert(
        "/assets/answer.js".to_string(),
        HttpTestResponse::script("application/javascript", javascript_dependency),
    );
    responses.insert(
        "/assets/main.ts".to_string(),
        HttpTestResponse::script("text/typescript", bluets_entry),
    );
    responses.insert(
        "/assets/answer.ts".to_string(),
        HttpTestResponse::script("application/typescript", bluets_dependency),
    );
    responses.insert(
        "/assets/bad-integrity.js".to_string(),
        HttpTestResponse::script("application/javascript", integrity_mismatch),
    );
    responses.insert(
        "/assets/bad-mime.js".to_string(),
        HttpTestResponse::script("text/plain", bad_mime),
    );
    responses.insert(
        "/assets/redirect.js".to_string(),
        HttpTestResponse::redirect("/assets/classic.js"),
    );
    // Eight requests: the repeated classic declaration uses the private
    // `(URL, integrity, MIME lane)` cache key, and the cross-origin URL
    // is rejected before any network I/O.
    let (origin, requested, server) = spawn_local_resource_server(responses, 8);
    let resource = |path: &str| format!("{origin}{path}");
    let manifest = HttpScriptIntegrityManifest::new([
        (
            resource("/assets/classic.js"),
            sha256_integrity(classic.as_bytes()),
        ),
        (
            resource("/assets/main.js"),
            sha256_integrity(javascript_entry.as_bytes()),
        ),
        (
            resource("/assets/answer.js"),
            sha256_integrity(javascript_dependency.as_bytes()),
        ),
        (
            resource("/assets/main.ts"),
            sha256_integrity(bluets_entry.as_bytes()),
        ),
        (
            resource("/assets/answer.ts"),
            sha256_integrity(bluets_dependency.as_bytes()),
        ),
        (
            resource("/assets/bad-integrity.js"),
            sha256_integrity(b"owner-selected different bytes"),
        ),
        (
            resource("/assets/bad-mime.js"),
            sha256_integrity(bad_mime.as_bytes()),
        ),
        (
            resource("/assets/redirect.js"),
            sha256_integrity(b"redirect body is not admitted"),
        ),
        (
            "http://127.0.0.1:1/cross-origin.js".to_string(),
            sha256_integrity(b"not fetched"),
        ),
    ])
    .unwrap();
    let policy = HttpScriptResourcePolicy::new(
        HttpScriptResourceOriginRule::same_document_origin(),
        manifest,
        HttpScriptResourceLimits::default(),
    )
    .unwrap();
    let authorizer = HttpOutOfProcessPageScriptSourceAuthorizer::new(policy);
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
            concat!(
                "<script src=\"/assets/classic.js\"></script>",
                "<script type=\"module\" src=\"/assets/main.js\"></script>",
                "<script type=\"application/x-blueice-typescript-module\" src=\"/assets/main.ts\"></script>",
                "<script src=\"/assets/classic.js\"></script>",
                "<script src=\"/assets/bad-integrity.js\"></script>",
                "<script type=\"module\" src=\"/assets/bad-mime.js\"></script>",
                "<script src=\"/assets/redirect.js\"></script>",
                "<script src=\"http://127.0.0.1:1/cross-origin.js\"></script>",
                "<script>if (globalThis.externalClassic !== 42 || globalThis.externalModule !== 42) throw 'graph';</script>"
            ),
            &format!("{origin}/app/index.html"),
        );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
        &path, &token, authorizer,
    )
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();

    let reports = executor.drain_reports_for_tab(tab_id);
    assert_eq!(
        reports,
        vec![
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: BlueJsPageScriptKind::Classic,
            },
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 1,
                kind: BlueJsPageScriptKind::Module,
            },
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 3,
                kind: BlueJsPageScriptKind::Classic,
            },
            JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 4,
                kind: BlueJsPageScriptKind::Classic,
                category: "external JavaScript source authorization rejected the page script",
            },
            JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 5,
                kind: BlueJsPageScriptKind::Module,
                category: "external JavaScript source authorization rejected the page script",
            },
            JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 6,
                kind: BlueJsPageScriptKind::Classic,
                category: "external JavaScript source authorization rejected the page script",
            },
            JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 7,
                kind: BlueJsPageScriptKind::Classic,
                category: "external JavaScript source authorization rejected the page script",
            },
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 8,
                kind: BlueJsPageScriptKind::Classic,
            },
        ]
    );
    assert_eq!(
        executor.drain_blue_ts_reports_for_tab(tab_id),
        vec![BlueTsPageExecutionReport::Executed {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            ordinal: 2,
            kind: DirectPageScriptKind::Module,
        }]
    );
    let source_free = format!("{reports:?}");
    for protected in [
        "redactedIntegrityMarker",
        "redactedMimeMarker",
        "bad-integrity.js",
        "bad-mime.js",
        "redirect.js",
        "cross-origin.js",
    ] {
        assert!(!source_free.contains(protected));
    }
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
    server.join().unwrap();
    let requested = requested.lock().unwrap();
    assert_eq!(requested.len(), 8);
    assert_eq!(
        requested
            .iter()
            .filter(|path| path.as_str() == "/assets/classic.js")
            .count(),
        1
    );
    assert!(!requested.iter().any(|path| path.contains("cross-origin")));
}

#[test]
fn external_authorizer_denial_and_invalid_graph_are_source_free_in_real_child_route() {
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
            concat!(
                "<script src=\"https://secret.example.test/denied.js\"></script>",
                "<script type=\"application/x-blueice-typescript\" src=\"https://secret.example.test/invalid.ts\"></script>"
            ),
            "https://example.test/app/index.html",
        );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
        &path,
        &token,
        DeniedAndInvalidGraphsAuthorizer,
    )
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();

    let reports = executor.drain_reports_for_tab(tab_id);
    let blue_ts_reports = executor.drain_blue_ts_reports_for_tab(tab_id);
    assert!(matches!(
        reports.as_slice(),
        [JavaScriptPageExecutionReport::Rejected {
            category: "external JavaScript source authorization rejected the page script",
            ..
        }]
    ));
    assert!(matches!(
        blue_ts_reports.as_slice(),
        [BlueTsPageExecutionReport::Rejected {
            category: "external BlueTS source authorization rejected the page script",
            ..
        }]
    ));
    let observed = format!("{reports:?}{blue_ts_reports:?}");
    assert!(!observed.contains("secret.example.test"));
    assert!(!observed.contains("invalid.ts"));
    assert!(!observed.contains("private policy"));
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn missing_authorized_static_edge_has_no_child_resolver_fallback() {
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
        concat!(
            "<script type=\"module\" src=\"/assets/missing-edge.js\"></script>",
            "<script>globalThis.afterMissingEdge = true;</script>"
        ),
        "https://example.test/app/index.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
        &path,
        &token,
        MissingStaticEdgeAuthorizer,
    )
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();

    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![
            JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: BlueJsPageScriptKind::Module,
                category: "authorized JavaScript graph is missing a static resolution",
            },
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 1,
                kind: BlueJsPageScriptKind::Classic,
            },
        ]
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn external_authorization_is_rechecked_and_old_child_realm_is_invalidated_on_navigation() {
    let (path, token, child) = spawn_child();
    let (mut tabs, tab_id) = loaded_tabs(
        "<script src=\"/assets/navigation.js\"></script>",
        "https://first.example.test/one",
    );
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
        &path,
        &token,
        NavigationAuthorizer {
            requests: Arc::clone(&requests),
        },
    )
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(matches!(
        executor.drain_reports_for_tab(tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Executed {
            document_generation: 1,
            ordinal: 0,
            ..
        }]
    ));
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script src=\"/assets/navigation.js\"></script>",
        Some("https://second.example.test/two".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(matches!(
        executor.drain_reports_for_tab(tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Executed {
            document_generation: 2,
            ordinal: 0,
            ..
        }]
    ));
    assert_eq!(
        requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| (
                request.document_generation,
                request.ordinal,
                request.document_url.as_str(),
                request.declared_src.as_str(),
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                1,
                0,
                "https://first.example.test/one",
                "/assets/navigation.js"
            ),
            (
                2,
                0,
                "https://second.example.test/two",
                "/assets/navigation.js"
            ),
        ]
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn core_routes_explicit_inline_documents_to_the_real_child_and_rejects_external_src() {
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
        concat!(
            "<script>globalThis.answer = 42;</script>",
            "<script type=\"module\">export const moduleAnswer = 43;</script>",
            "<script src=\"untrusted.js\"></script>"
        ),
        "https://example.test/app/index.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let stats = executor
        .realm_stats(tab_id)
        .expect("the real child must supply core-owned realm accounting");
    assert_eq!(stats.tab_id, tab_id.as_u64());
    assert_eq!(stats.document_generation, 1);
    assert_eq!(stats.program_count, 2);
    assert!(
        stats.bytecode_bytes > 0,
        "the aggregate record must charge the admitted classic and module programs"
    );
    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: BlueJsPageScriptKind::Classic,
            },
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 1,
                kind: BlueJsPageScriptKind::Module,
            },
            JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 2,
                kind: BlueJsPageScriptKind::Classic,
                category: "external JavaScript declarations require an authorized loader",
            },
        ]
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}
