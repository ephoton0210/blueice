// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn verified_source_cache_is_byte_bounded_and_evicts_least_recently_used() {
    let key = |name: &str| ResourceCacheKey {
        canonical_url: format!("https://example.test/{name}.js"),
        expected_integrity: sha256_integrity(name.as_bytes()),
        mime_lane: "javascript",
    };
    let mut cache = VerifiedResourceCache::new(8);
    cache.insert_verified(key("a"), "aaaa".into());
    cache.insert_verified(key("b"), "bbbb".into());
    assert_eq!(cache.get(&key("a")), Some("aaaa".into()));
    cache.insert_verified(key("c"), "cccc".into());
    assert_eq!(cache.get(&key("b")), None);
    assert_eq!(cache.get(&key("a")), Some("aaaa".into()));
    assert_eq!(cache.get(&key("c")), Some("cccc".into()));
    assert_eq!(cache.source_bytes, 8);
    cache.insert_verified(key("too-large"), "123456789".into());
    assert_eq!(cache.get(&key("too-large")), None);
    assert_eq!(cache.source_bytes, 8);
    cache.insert_verified(key("a"), "aa".into());
    assert_eq!(cache.source_bytes, 6);
}

#[test]
fn evicted_http_source_is_refetched_and_integrity_checked_again() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    const A: &str = "var a = 1;";
    const B: &str = "var b = 2;";
    const CHANGED_A: &str = "var a = 3;";
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let mut paths = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while paths.len() < 3 && Instant::now() < deadline {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0u8; 1024];
            let count = stream.read(&mut request).unwrap();
            let path = std::str::from_utf8(&request[..count])
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .to_string();
            let body = match (paths.len(), path.as_str()) {
                (0, "/a.js") => A,
                (1, "/b.js") => B,
                (2, "/a.js") => CHANGED_A,
                _ => panic!("unexpected cache fixture request: {path}"),
            };
            paths.push(path);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
        paths
    });
    let a = format!("{origin}/a.js");
    let b = format!("{origin}/b.js");
    let policy = HttpScriptResourcePolicy::new(
        HttpScriptResourceOriginRule::same_document_origin(),
        HttpScriptIntegrityManifest::new([
            (a.clone(), sha256_integrity(A.as_bytes())),
            (b.clone(), sha256_integrity(B.as_bytes())),
        ])
        .unwrap(),
        HttpScriptResourceLimits::default(),
    )
    .unwrap();
    let authorizer = HttpOutOfProcessPageScriptSourceAuthorizer::new(policy);
    authorizer
        .cache
        .replace(VerifiedResourceCache::new(A.len()));
    let language = GraphLanguage::JavaScript(BlueJsPageScriptKind::Classic);
    assert_eq!(authorizer.fetch_resource(&origin, &a, language).unwrap(), A);
    assert_eq!(authorizer.fetch_resource(&origin, &b, language).unwrap(), B);
    assert_eq!(
        authorizer.fetch_resource(&origin, &a, language),
        Err(ResourceAuthorizationFailure::IntegrityMismatch)
    );
    assert_eq!(server.join().unwrap(), ["/a.js", "/b.js", "/a.js"]);
}

#[test]
fn sha256_integrity_matches_fips_vectors() {
    assert_eq!(
        sha256_integrity(b""),
        "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_integrity(b"abc"),
        "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn resource_urls_are_canonical_and_static_path_only() {
    assert_eq!(
        canonical_http_resource_url("HTTP://EXAMPLE.test:80/assets/./main.js").unwrap(),
        "http://example.test/assets/main.js"
    );
    assert_eq!(
        resolve_resource_url(
            "https://example.test/app/index.html?ignored=1",
            "./scripts/../main.js",
            true,
        )
        .unwrap(),
        "https://example.test/app/main.js"
    );
    assert!(matches!(
        resolve_resource_url("https://example.test/app/index.html", "pkg", false),
        Err(ResourceAuthorizationFailure::BareStaticSpecifier)
    ));
    assert!(canonical_http_resource_url("https://example.test/a%2fb.js").is_err());
}

#[test]
fn manifest_and_policy_require_canonical_owner_configuration() {
    assert!(HttpScriptResourceOriginRule::exact_origin("https://EXAMPLE.test").is_err());
    let exact = HttpScriptResourceOriginRule::exact_origin("https://assets.example.test").unwrap();
    assert!(exact.permits("https://page.example.test", "https://assets.example.test"));
    assert!(!exact.permits("https://page.example.test", "https://other.example.test"));
    assert!(HttpScriptIntegrityManifest::new([(
        "https://example.test/a.js".to_string(),
        "sha256:not-a-digest".to_string(),
    )])
    .is_err());
    let manifest = HttpScriptIntegrityManifest::new([(
        "https://example.test/a.js".to_string(),
        sha256_integrity(b"a"),
    )])
    .unwrap();
    let policy = HttpScriptResourcePolicy::new(
        HttpScriptResourceOriginRule::same_document_origin(),
        manifest.clone(),
        HttpScriptResourceLimits::default(),
    )
    .unwrap();
    assert!(policy
        .resolver_fingerprint()
        .starts_with("core-page-http-resource-authorizer-v2:sha256:"));
    let zero_depth = HttpScriptResourceLimits {
        max_module_depth: 0,
        ..HttpScriptResourceLimits::default()
    };
    assert!(HttpScriptResourcePolicy::new(
        HttpScriptResourceOriginRule::same_document_origin(),
        manifest,
        zero_depth,
    )
    .is_err());
}

#[test]
fn fixed_core_profile_rejects_every_declaration_except_its_compiled_classic_path() {
    let authorizer = CoreHttpPageScriptFixtureAuthorizer::new();
    let request = |language, declared_src: &str| OutOfProcessPageScriptSourceRequest {
        tab_id: crate::TabId::from_u64(1),
        document_generation: 1,
        ordinal: 0,
        language,
        document_url: "http://127.0.0.1:45678/app/index.html".to_string(),
        declared_src: declared_src.to_string(),
    };
    assert!(authorizer
        .authorize(&request(
            CombinedPageScriptLanguage::JavaScript(BlueJsPageScriptKind::Classic),
            "/not-the-fixed-profile.js",
        ))
        .is_err());
    assert!(authorizer
        .authorize(&request(
            CombinedPageScriptLanguage::JavaScript(BlueJsPageScriptKind::Module),
            CORE_HTTP_PAGE_SCRIPT_FIXTURE_PATH,
        ))
        .is_err());
}

#[test]
fn in_process_adapter_uses_the_same_manifest_checked_bluets_graph_builder() {
    let source = "export const answer: number = 42;";
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 1024];
        let _ = std::io::Read::read(&mut stream, &mut request);
        std::io::Write::write_all(
            &mut stream,
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/typescript; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{source}",
                source.len()
            )
            .as_bytes(),
        )
        .unwrap();
    });
    let entry = format!("http://{address}/assets/main.ts");
    let policy = HttpScriptResourcePolicy::new(
        HttpScriptResourceOriginRule::same_document_origin(),
        HttpScriptIntegrityManifest::new([(entry.clone(), sha256_integrity(source.as_bytes()))])
            .unwrap(),
        HttpScriptResourceLimits::default(),
    )
    .unwrap();
    let fingerprint = policy.resolver_fingerprint().to_string();
    let mut authorizer = HttpOutOfProcessPageScriptSourceAuthorizer::new(policy);

    let graph = PageScriptSourceAuthorizer::authorize(
        &mut authorizer,
        &PageScriptSourceRequest {
            tab_id: crate::TabId::from_u64(1),
            document_generation: 1,
            ordinal: 0,
            kind: DirectPageScriptKind::Module,
            document_url: format!("http://{address}/app/index.html"),
            declared_src: "/assets/main.ts".to_string(),
        },
    )
    .unwrap();
    server.join().unwrap();

    assert_eq!(graph.entry, entry);
    assert_eq!(graph.resolver_fingerprint, fingerprint);
    assert_eq!(graph.loader.module_count(), 1);
}

#[test]
fn in_process_adapter_denies_cross_origin_before_any_fetch() {
    let entry = "https://example.test/assets/main.ts";
    let policy = HttpScriptResourcePolicy::new(
        HttpScriptResourceOriginRule::same_document_origin(),
        HttpScriptIntegrityManifest::new([(
            entry.to_string(),
            sha256_integrity(b"export const answer: number = 42;"),
        )])
        .unwrap(),
        HttpScriptResourceLimits::default(),
    )
    .unwrap();
    let mut authorizer = HttpOutOfProcessPageScriptSourceAuthorizer::new(policy);

    let error = PageScriptSourceAuthorizer::authorize(
        &mut authorizer,
        &PageScriptSourceRequest {
            tab_id: crate::TabId::from_u64(1),
            document_generation: 1,
            ordinal: 0,
            kind: DirectPageScriptKind::Module,
            document_url: "https://example.test/app/index.html".to_string(),
            declared_src: "https://attacker.test/secret.ts".to_string(),
        },
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "resource origin is not permitted by the startup policy"
    );
}
