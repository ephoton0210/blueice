use std::cell::RefCell;
use std::collections::BTreeMap;

use super::*;

struct Fake {
    bodies: BTreeMap<String, Vec<u8>>,
    calls: RefCell<Vec<String>>,
}

impl Fake {
    fn new(bodies: &[(&str, &[u8])]) -> Self {
        Self {
            bodies: bodies
                .iter()
                .map(|(url, body)| (url.to_string(), body.to_vec()))
                .collect(),
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl RemoteFetcher for Fake {
    fn fetch(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
        self.calls.borrow_mut().push(url.to_string());
        let body = self.bodies.get(url).ok_or("404")?;
        // A well-behaved fetcher stops at the limit; this one overshoots by one
        // byte so the caller's own check is exercised too.
        Ok(body.iter().copied().take(max_bytes + 1).collect())
    }
}

fn directory(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("bluets-remote-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    path
}

fn source(specifier: &str, url: &str, body: &[u8]) -> RemoteDeclarationSource {
    RemoteDeclarationSource {
        specifier: specifier.to_string(),
        url: url.to_string(),
        sha256: sha256_hex(body),
    }
}

const BODY: &[u8] = b"export declare function f(): number;\n";

#[test]
fn a_pinned_source_is_fetched_verified_cached_and_then_served_without_fetching() {
    let cache = DeclarationCache::new(directory("ok"), RemoteLimits::default());
    let sources = [source("remote-lib", "https://example.test/lib.d.ts", BODY)];
    let fetcher = Fake::new(&[("https://example.test/lib.d.ts", BODY)]);
    assert!(matches!(
        cache.load_all(&sources),
        Err(RemoteError::NotCached { .. })
    ));
    assert!(fetcher.calls.borrow().is_empty(), "loading never fetches");

    let report = cache.fetch_missing(&sources, &fetcher).unwrap();
    assert_eq!(report.fetched, ["remote-lib"]);
    let loaded = cache.load_all(&sources).unwrap();
    assert_eq!(loaded[0].1.as_bytes(), BODY);

    let report = cache.fetch_missing(&sources, &fetcher).unwrap();
    assert_eq!(report.already_cached, ["remote-lib"]);
    assert_eq!(
        fetcher.calls.borrow().len(),
        1,
        "a cached source is not fetched again"
    );
    let _ = fs::remove_dir_all(cache.directory());
}

#[test]
fn bytes_that_do_not_match_the_pin_are_rejected_and_never_cached() {
    let cache = DeclarationCache::new(directory("mismatch"), RemoteLimits::default());
    let sources = [source("remote-lib", "https://example.test/lib.d.ts", BODY)];
    let fetcher = Fake::new(&[(
        "https://example.test/lib.d.ts",
        b"export declare const evil: 1;\n",
    )]);
    let error = cache.fetch_missing(&sources, &fetcher).unwrap_err();
    assert!(matches!(error, RemoteError::PinMismatch { .. }), "{error}");
    assert!(matches!(cache.read(&sources[0].sha256), Ok(None)));
    assert_eq!(
        fs::read_dir(cache.directory()).unwrap().count(),
        0,
        "no partial file remains"
    );
    let _ = fs::remove_dir_all(cache.directory());
}

#[test]
fn a_tampered_cache_entry_is_corrupt_not_trusted() {
    let cache = DeclarationCache::new(directory("tamper"), RemoteLimits::default());
    let sources = [source("remote-lib", "https://example.test/lib.d.ts", BODY)];
    cache
        .fetch_missing(
            &sources,
            &Fake::new(&[("https://example.test/lib.d.ts", BODY)]),
        )
        .unwrap();
    fs::write(
        cache
            .directory()
            .join(format!("{}.d.ts", sources[0].sha256)),
        "export declare const evil: 1;\n",
    )
    .unwrap();
    assert!(matches!(
        cache.load_all(&sources),
        Err(RemoteError::CorruptCache { .. })
    ));
    let _ = fs::remove_dir_all(cache.directory());
}

#[test]
fn size_count_and_cache_bounds_are_enforced() {
    let limits = RemoteLimits {
        max_source_bytes: 8,
        max_sources: 2,
        max_cache_bytes: 20,
    };
    let big = source("big", "https://example.test/big.d.ts", BODY);
    let cache = DeclarationCache::new(directory("bounds"), limits.clone());
    let error = cache
        .fetch_missing(
            &[big],
            &Fake::new(&[("https://example.test/big.d.ts", BODY)]),
        )
        .unwrap_err();
    assert!(matches!(error, RemoteError::TooLarge { .. }), "{error}");

    let many: Vec<_> = (0..3)
        .map(|index| source(&format!("p{index}"), "https://example.test/x.d.ts", b"x"))
        .collect();
    assert!(matches!(
        validate_sources(&many, &limits),
        Err(RemoteError::TooManySources(2))
    ));

    let first = source("a", "https://example.test/a.d.ts", b"12345678");
    let second = source("b", "https://example.test/b.d.ts", b"abcdefgh");
    let third = source("c", "https://example.test/c.d.ts", b"ABCDEFGH");
    let fetcher = Fake::new(&[
        ("https://example.test/a.d.ts", b"12345678"),
        ("https://example.test/b.d.ts", b"abcdefgh"),
        ("https://example.test/c.d.ts", b"ABCDEFGH"),
    ]);
    let small = RemoteLimits {
        max_sources: 3,
        ..limits
    };
    let cache = DeclarationCache::new(directory("bounds-total"), small);
    let error = cache
        .fetch_missing(&[first, second, third], &fetcher)
        .unwrap_err();
    assert!(
        matches!(error, RemoteError::CacheFull { limit: 20 }),
        "{error}"
    );
    let _ = fs::remove_dir_all(cache.directory());
}

#[test]
fn the_owners_list_is_validated_before_anything_runs() {
    let limits = RemoteLimits::default();
    let ok = source("a", "https://example.test/a.d.ts", b"x");
    assert!(validate_sources(std::slice::from_ref(&ok), &limits).is_ok());
    for bad_url in [
        "http://example.test/a.d.ts",
        "file:///etc/passwd",
        "https://user:pw@example.test/a.d.ts",
        "https://",
        "https://example.test/a.d.ts#frag",
        "https://example.test/a b.d.ts",
        "//example.test/a.d.ts",
        "ftp://example.test/a.d.ts",
    ] {
        let mut candidate = ok.clone();
        candidate.url = bad_url.to_string();
        assert!(
            matches!(
                validate_sources(&[candidate], &limits),
                Err(RemoteError::InvalidUrl { .. })
            ),
            "{bad_url}"
        );
    }
    for bad_specifier in ["", "./rel", "/abs", "https://x/y", "a/../b", "a\\b"] {
        let mut candidate = ok.clone();
        candidate.specifier = bad_specifier.to_string();
        assert!(
            matches!(
                validate_sources(&[candidate], &limits),
                Err(RemoteError::InvalidSpecifier(_))
            ),
            "{bad_specifier}"
        );
    }
    for bad_pin in ["", "ABC", &"g".repeat(64), &"A".repeat(64), &"a".repeat(63)] {
        let mut candidate = ok.clone();
        candidate.sha256 = bad_pin.to_string();
        assert!(
            matches!(
                validate_sources(&[candidate], &limits),
                Err(RemoteError::InvalidPin(_))
            ),
            "{bad_pin}"
        );
    }
    assert!(matches!(
        validate_sources(&[ok.clone(), ok], &limits),
        Err(RemoteError::DuplicateSpecifier(_))
    ));
}

#[test]
fn a_fetch_failure_and_non_utf8_bytes_are_errors() {
    let cache = DeclarationCache::new(directory("failure"), RemoteLimits::default());
    let missing = source("a", "https://example.test/missing.d.ts", b"x");
    assert!(matches!(
        cache.fetch_missing(&[missing], &Fake::new(&[])),
        Err(RemoteError::FetchFailed { .. })
    ));
    let binary: &[u8] = &[0xff, 0xfe, 0x00];
    let source = source("b", "https://example.test/b.d.ts", binary);
    assert!(matches!(
        cache.fetch_missing(
            &[source],
            &Fake::new(&[("https://example.test/b.d.ts", binary)])
        ),
        Err(RemoteError::NotUtf8(_))
    ));
    let _ = fs::remove_dir_all(cache.directory());
}

#[test]
fn sha256_matches_the_standard_vector() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
