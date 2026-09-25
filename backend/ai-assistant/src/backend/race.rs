// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Simultaneous mode (`phase-7-local-ai/PLAN.md`, step C1): the same
//! completion goes to two backends at once and the first *successful* answer
//! wins.
//!
//! This costs double the resources and is an explicit opt-in. Answers are never
//! merged -- the caller gets exactly one backend's text. A backend that fails
//! does not end the race while the other may still succeed; only when both
//! fail does the completion fail, naming both reasons.
//!
//! The loser runs on a detached thread, not a scoped one: a blocking HTTP call
//! cannot be interrupted, and the winner must return at once rather than wait
//! for it. The loser is asked to cancel (see [`CancelToken`]) and its answer is
//! dropped. At most two threads exist per completion.

use super::{CancelToken, Completion, InferenceBackend};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

pub struct Race {
    first: Arc<dyn InferenceBackend>,
    second: Arc<dyn InferenceBackend>,
    name: String,
}

impl Race {
    pub fn new(first: Arc<dyn InferenceBackend>, second: Arc<dyn InferenceBackend>) -> Self {
        let name = format!("{}+{}", first.name(), second.name());
        Race {
            first,
            second,
            name,
        }
    }
}

impl InferenceBackend for Race {
    fn name(&self) -> &str {
        &self.name
    }

    fn complete(&self, completion: Completion<'_>) -> Result<String, String> {
        let tokens = [Arc::new(CancelToken::new()), Arc::new(CancelToken::new())];
        let (tx, rx) = mpsc::channel::<(usize, Result<String, String>)>();
        let system = completion.system.to_string();
        let user = completion.user.to_string();
        let max_tokens = completion.max_tokens;
        for (index, backend) in [self.first.clone(), self.second.clone()]
            .into_iter()
            .enumerate()
        {
            let tx = tx.clone();
            let token = tokens[index].clone();
            let (system, user) = (system.clone(), user.clone());
            thread::spawn(move || {
                let outcome = backend.complete(Completion {
                    system: &system,
                    user: &user,
                    max_tokens,
                    cancel: Some(&token),
                });
                let _ = tx.send((index, outcome));
            });
        }
        drop(tx);

        let mut failures: [Option<String>; 2] = [None, None];
        while let Ok((index, outcome)) = rx.recv() {
            match outcome {
                Ok(text) => {
                    // The other side no longer matters.
                    tokens[1 - index].cancel();
                    return Ok(text);
                }
                Err(reason) => failures[index] = Some(reason),
            }
            // The caller stopped caring (not used today, but cheap to honour):
            // cancel both and report it.
            if completion.is_cancelled() {
                tokens.iter().for_each(|token| token.cancel());
                return Err("cancelled".to_string());
            }
        }
        let describe = |index: usize, backend: &Arc<dyn InferenceBackend>| {
            format!(
                "{}: {}",
                backend.name(),
                failures[index].as_deref().unwrap_or("no answer")
            )
        };
        Err(format!(
            "both backends failed ({}; {})",
            describe(0, &self.first),
            describe(1, &self.second)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    /// Answers `result` after `delay`, polling for cancellation so a test can
    /// see whether the loser was told to stop.
    struct Fake {
        name: &'static str,
        delay: Duration,
        result: Result<&'static str, &'static str>,
        saw_cancel: Arc<AtomicBool>,
    }

    impl Fake {
        fn new(
            name: &'static str,
            delay_ms: u64,
            result: Result<&'static str, &'static str>,
        ) -> (Arc<Self>, Arc<AtomicBool>) {
            let saw_cancel = Arc::new(AtomicBool::new(false));
            (
                Arc::new(Fake {
                    name,
                    delay: Duration::from_millis(delay_ms),
                    result,
                    saw_cancel: saw_cancel.clone(),
                }),
                saw_cancel,
            )
        }
    }

    impl InferenceBackend for Fake {
        fn name(&self) -> &str {
            self.name
        }

        fn complete(&self, completion: Completion<'_>) -> Result<String, String> {
            let started = Instant::now();
            while started.elapsed() < self.delay {
                if completion.is_cancelled() {
                    self.saw_cancel.store(true, Ordering::Relaxed);
                    return Err("cancelled".to_string());
                }
                thread::sleep(Duration::from_millis(5));
            }
            self.result.map(str::to_string).map_err(str::to_string)
        }
    }

    fn ask(race: &Race) -> Result<String, String> {
        race.complete(Completion {
            system: "s",
            user: "u",
            max_tokens: 8,
            cancel: None,
        })
    }

    fn wait_until(flag: &AtomicBool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if flag.load(Ordering::Relaxed) {
                return true;
            }
            thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn the_first_success_wins_immediately_and_the_loser_is_cancelled() {
        let (slow, slow_cancelled) = Fake::new("slow", 5_000, Ok("slow answer"));
        let (fast, fast_cancelled) = Fake::new("fast", 20, Ok("fast answer"));
        let race = Race::new(slow, fast);
        let started = Instant::now();
        assert_eq!(ask(&race), Ok("fast answer".to_string()));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the winner must not wait for the loser"
        );
        assert!(
            wait_until(&slow_cancelled),
            "the loser must be told to stop"
        );
        assert!(!fast_cancelled.load(Ordering::Relaxed));
    }

    #[test]
    fn a_fast_failure_does_not_end_the_race() {
        let (broken, _) = Fake::new("broken", 0, Err("down"));
        let (working, _) = Fake::new("working", 60, Ok("answer"));
        assert_eq!(
            ask(&Race::new(broken, working.clone())),
            Ok("answer".to_string())
        );
        // Order does not matter.
        let (broken, _) = Fake::new("broken", 0, Err("down"));
        assert_eq!(ask(&Race::new(working, broken)), Ok("answer".to_string()));
    }

    #[test]
    fn both_failing_reports_both_reasons() {
        let (a, _) = Fake::new("llamacpp", 0, Err("not running"));
        let (b, _) = Fake::new("candle", 10, Err("no weights"));
        let error = ask(&Race::new(a, b)).unwrap_err();
        assert!(error.contains("llamacpp: not running"), "{error}");
        assert!(error.contains("candle: no weights"), "{error}");
    }

    #[test]
    fn answers_are_never_merged() {
        let (a, _) = Fake::new("a", 0, Ok("from a"));
        let (b, _) = Fake::new("b", 400, Ok("from b"));
        assert_eq!(ask(&Race::new(a, b)), Ok("from a".to_string()));
    }

    #[test]
    fn the_race_is_named_for_both_backends() {
        let (a, _) = Fake::new("llamacpp", 0, Ok("x"));
        let (b, _) = Fake::new("candle", 0, Ok("x"));
        assert_eq!(Race::new(a, b).name(), "llamacpp+candle");
    }

    #[test]
    fn a_race_can_itself_be_cancelled_by_its_caller() {
        let (a, _) = Fake::new("a", 0, Err("down"));
        let (b, _) = Fake::new("b", 0, Err("down"));
        let token = CancelToken::new();
        token.cancel();
        let race = Race::new(a, b);
        assert_eq!(
            race.complete(Completion {
                system: "s",
                user: "u",
                max_tokens: 8,
                cancel: Some(&token),
            }),
            Err("cancelled".to_string())
        );
    }
}
