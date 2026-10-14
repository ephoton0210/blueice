// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Worker ownership and memo contracts using an independent pool and memo.

use super::*;
use std::sync::Arc;

#[cfg_attr(test, test)]
fn worker_path_refusals_preserve_the_original_io_error() {
    let missing = || io::Error::new(io::ErrorKind::NotFound, "missing executable path");
    for result in [
        Worker::sibling_worker_path(Err(missing())),
        Worker::start_from_path(Err(missing())).map(|_| PathBuf::new()),
    ] {
        let error = result.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(error.to_string(), "missing executable path");
    }
    let suffix = format!("bluejs-regexp-worker{}", std::env::consts::EXE_SUFFIX);
    for executable in [
        "/build/debug/app",
        "/build/debug/deps/test",
        "/build/debug/examples/demo",
    ] {
        assert_eq!(
            Worker::sibling_worker_path(Ok(PathBuf::from(executable))).unwrap(),
            PathBuf::from("/build/debug").join(&suffix)
        );
    }
}

#[cfg_attr(test, test)]
fn healthy_worker_reuse_and_excess_retirement_release_the_pool_lock() {
    let pool = Arc::new(Mutex::new(Vec::new()));
    let make_worker = || {
        #[cfg(windows)]
        let child = Command::new("cmd.exe")
            .args(["/C", "exit", "0"])
            .spawn()
            .unwrap();
        #[cfg(not(windows))]
        let child = Command::new("true").spawn().unwrap();
        let (requests, incoming) = mpsc::channel();
        let (reply, replies) = mpsc::channel();
        reply.send(Ok(encode_reply(&Reply::Compiled))).unwrap();
        (
            Worker {
                child,
                requests: Some(requests),
                replies,
                io_thread: None,
                failed: false,
                pattern: None,
                input: None,
            },
            incoming,
        )
    };
    let mut transports = Vec::new();
    let mut identities = Vec::new();
    for _ in 0..MAX_IDLE_WORKERS {
        let (worker, incoming) = make_worker();
        identities.push(worker.child.id());
        transports.push(incoming);
        recycle_worker(worker, &pool);
    }
    assert_eq!(pool.lock().unwrap().len(), MAX_IDLE_WORKERS);
    let mut reused = pool.lock().unwrap().pop().unwrap();
    assert_eq!(reused.child.id(), *identities.last().unwrap());
    assert!(matches!(
        reused.compile(vec![97], String::new(), DEFAULT_TIMEOUT),
        Ok(Reply::Compiled)
    ));
    let request = transports
        .last()
        .unwrap()
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let request: serde_json::Value = serde_json::from_slice(&request).unwrap();
    assert_eq!(request["operation"], "compile");
    assert_eq!(request["source"], serde_json::json!([97]));
    recycle_worker(reused, &pool);

    let (mut excess, incoming) = make_worker();
    let retiring_pool = Arc::clone(&pool);
    let (observed, retired) = mpsc::channel();
    excess.io_thread = Some(std::thread::spawn(move || {
        let request = incoming.recv_timeout(Duration::from_secs(2)).unwrap();
        let request: serde_json::Value = serde_json::from_slice(&request).unwrap();
        assert_eq!(request["operation"], "shutdown");
        observed.send(retiring_pool.try_lock().is_ok()).unwrap();
    }));
    recycle_worker(excess, &pool);
    assert!(
        retired.recv_timeout(Duration::from_secs(2)).unwrap(),
        "shutdown held the pool lock"
    );
    assert_eq!(
        pool.lock()
            .unwrap()
            .iter()
            .map(|worker| worker.child.id())
            .collect::<Vec<_>>(),
        identities
    );
    assert!(transports
        .iter()
        .all(|incoming| incoming.try_recv().is_err()));
}

#[cfg_attr(test, test)]
fn memo_entry_eviction_preserves_cached_misses_and_duplicate_identity() {
    let key = |index: usize| FindKey {
        source: vec![if index.is_multiple_of(2) { 97 } else { 122 }],
        flags: String::new(),
        input: format!("{index}a").encode_utf16().collect(),
        start: 0,
    };
    let mut memo = FindMemo::default();
    for index in 0..=MEMO_ENTRIES {
        let answer = index
            .is_multiple_of(2)
            .then(|| Match::whole(index.to_string().len()..index.to_string().len() + 1));
        memo.remember(key(index), answer);
    }
    assert_eq!(memo.results.len(), MEMO_ENTRIES);
    assert_eq!(memo.order.len(), MEMO_ENTRIES);
    assert!(!memo.results.contains_key(&key(0)));
    assert!(matches!(memo.results.get(&key(1)), Some(None)));
    let before = (memo.order.clone(), memo.units);
    memo.remember(key(1), None);
    assert!(memo.order == before.0);
    assert_eq!(memo.units, before.1);
    assert_eq!(memo.order.front().unwrap().input, key(1).input);
    assert_eq!(
        memo.units,
        memo.order
            .iter()
            .map(|key| key.source.len() + key.input.len())
            .sum::<usize>()
    );
}

#[cfg_attr(test, test)]
fn memo_byte_eviction_and_oversized_admission_preserve_the_exact_budget() {
    let key = |start| FindKey {
        source: vec![97],
        flags: String::new(),
        input: vec![98; MEMO_KEY_UNITS - 1],
        start,
    };
    let entries = MEMO_UNITS / MEMO_KEY_UNITS;
    let mut memo = FindMemo::default();
    for start in 0..=entries {
        memo.remember(key(start), None);
    }
    assert_eq!(memo.units, MEMO_UNITS);
    assert_eq!(memo.results.len(), entries);
    assert_eq!(memo.order.len(), entries);
    assert!(!memo.results.contains_key(&key(0)));
    assert!(matches!(memo.results.get(&key(1)), Some(None)));
    let mut oversized = key(entries + 1);
    oversized.input.push(98);
    memo.remember(oversized.clone(), None);
    assert!(!memo.results.contains_key(&oversized));
    assert_eq!(memo.units, MEMO_UNITS);
    assert_eq!(memo.results.len(), entries);
    assert_eq!(memo.order.front().unwrap().start, 1);
}

#[cfg(coverage)]
#[doc(hidden)]
pub fn verify_regex_pool_boundary_contracts() {
    worker_path_refusals_preserve_the_original_io_error();
    healthy_worker_reuse_and_excess_retirement_release_the_pool_lock();
    memo_entry_eviction_preserves_cached_misses_and_duplicate_identity();
    memo_byte_eviction_and_oversized_admission_preserve_the_exact_budget();
}
