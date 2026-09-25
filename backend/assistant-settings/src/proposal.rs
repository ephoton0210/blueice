// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The rule-base for *proposed* assistant settings changes
//! (`phase-7-local-ai/PLAN.md`, step R4).
//!
//! An AI agent may propose a change, but it takes effect only after the person
//! approves it in the trusted window. Before a proposal can even reach that
//! window it must pass these deterministic rules; a proposal that breaks any is
//! blocked outright and the person is never asked, so an agent cannot use the
//! consent prompt as a way to wear the person down.
//!
//! The rules bound what an agent can *ask for*, not what the person can do: a
//! direct edit by the person in the trusted window passes
//! [`AssistantSettings::validate`] only. Everything the rules need from the
//! outside world (the filesystem, physical memory) arrives through
//! [`Environment`], so the rule-base itself is pure and every rule is a plain
//! unit test. [`evaluate`] reports **every** violated rule, not just the first,
//! so the agent (and the log) see the whole picture at once.

use crate::{AssistantSettings, BackendKind};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::{Component, Path, PathBuf};

/// An agent may not ask for a priority higher than this niceness.
pub const MIN_PROPOSED_NICE: i32 = 5;
/// An existing memory ceiling may be raised by an agent at most to this multiple.
pub const MAX_CEILING_RAISE_FACTOR: u64 = 2;

/// What the rules need to know about the machine, supplied by the caller.
pub struct Environment<'a> {
    /// Whether `path` is an existing regular file.
    pub is_regular_file: &'a dyn Fn(&Path) -> bool,
    /// The path with symlinks resolved, or `None` if it cannot be resolved. A
    /// lexical prefix check alone would let a symlink inside an allowed
    /// directory point anywhere.
    pub canonicalize: &'a dyn Fn(&Path) -> Option<PathBuf>,
    /// Physical memory in MiB; no ceiling above half of it may be proposed.
    pub physical_memory_mb: u64,
    /// The BlueIce models directory, always an allowed place for model files.
    pub models_dir: PathBuf,
}

/// One rule a proposal broke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    /// The proposal is not valid settings at all (carries the validator's reason).
    Invalid(String),
    /// The proposal equals the current settings.
    NoChange,
    /// A candle path that is not an acceptable model file.
    PathNotAllowed {
        field: &'static str,
        reason: &'static str,
    },
    CeilingRemoved,
    CeilingRaisedTooMuch {
        current_mb: u64,
        proposed_mb: u64,
    },
    CeilingTooLarge {
        proposed_mb: u64,
        limit_mb: u64,
    },
    PriorityTooHigh {
        proposed: i32,
    },
    BothNeedsACeiling,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Violation::Invalid(reason) => write!(f, "the settings are not valid: {reason}"),
            Violation::NoChange => write!(f, "the proposal changes nothing"),
            Violation::PathNotAllowed { field, reason } => write!(f, "{field}: {reason}"),
            Violation::CeilingRemoved => {
                write!(f, "an existing memory ceiling may not be removed by a proposal")
            }
            Violation::CeilingRaisedTooMuch { current_mb, proposed_mb } => write!(
                f,
                "the memory ceiling may not be raised from {current_mb} MiB to more than {MAX_CEILING_RAISE_FACTOR}x that ({proposed_mb} MiB proposed)"
            ),
            Violation::CeilingTooLarge { proposed_mb, limit_mb } => write!(
                f,
                "a memory ceiling of {proposed_mb} MiB exceeds half of physical memory ({limit_mb} MiB)"
            ),
            Violation::PriorityTooHigh { proposed } => write!(
                f,
                "niceness {proposed} asks for a higher priority than a proposal may (minimum {MIN_PROPOSED_NICE})"
            ),
            Violation::BothNeedsACeiling => {
                write!(f, "running both backends at once needs a memory ceiling")
            }
        }
    }
}

/// Where a candle model file may live, given the current settings: the
/// directory of the model already configured, and the BlueIce models directory.
fn allowed_roots(current: &AssistantSettings, env: &Environment<'_>) -> Vec<PathBuf> {
    let mut roots = vec![env.models_dir.clone()];
    if let Some(candle) = &current.candle {
        if let Some(parent) = Path::new(&candle.model_path).parent() {
            roots.push(parent.to_path_buf());
        }
    }
    roots
        .into_iter()
        .filter_map(|root| (env.canonicalize)(&root))
        .collect()
}

fn check_path(
    field: &'static str,
    path: &str,
    extension: &str,
    roots: &[PathBuf],
    env: &Environment<'_>,
) -> Option<Violation> {
    let path = Path::new(path);
    let refuse = |reason| Some(Violation::PathNotAllowed { field, reason });
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return refuse("must not contain `..`");
    }
    if path.extension().and_then(|e| e.to_str()) != Some(extension) {
        return refuse(if extension == "gguf" {
            "must be a .gguf file"
        } else {
            "must be a .json file"
        });
    }
    if !(env.is_regular_file)(path) {
        return refuse("must be an existing regular file");
    }
    let Some(resolved) = (env.canonicalize)(path) else {
        return refuse("could not be resolved");
    };
    if !roots.iter().any(|root| resolved.starts_with(root)) {
        return refuse(
            "must be inside the configured model directory or the BlueIce models directory",
        );
    }
    None
}

/// Checks `proposed` against every rule. `Ok(())` means the proposal may be
/// shown to the person for approval; it does *not* mean it has been approved.
pub fn evaluate(
    current: &AssistantSettings,
    proposed: &AssistantSettings,
    env: &Environment<'_>,
) -> Result<(), Vec<Violation>> {
    let mut violations = Vec::new();
    if let Err(reason) = proposed.validate() {
        // Nothing else about invalid settings is worth reporting.
        return Err(vec![Violation::Invalid(reason)]);
    }
    if proposed == current {
        violations.push(Violation::NoChange);
    }
    if let Some(candle) = &proposed.candle {
        let roots = allowed_roots(current, env);
        violations.extend(check_path(
            "candle model_path",
            &candle.model_path,
            "gguf",
            &roots,
            env,
        ));
        violations.extend(check_path(
            "candle tokenizer_path",
            &candle.tokenizer_path,
            "json",
            &roots,
            env,
        ));
    }
    match (current.max_resident_mb, proposed.max_resident_mb) {
        (Some(_), None) => violations.push(Violation::CeilingRemoved),
        (Some(current_mb), Some(proposed_mb))
            if proposed_mb > current_mb * MAX_CEILING_RAISE_FACTOR =>
        {
            violations.push(Violation::CeilingRaisedTooMuch {
                current_mb,
                proposed_mb,
            })
        }
        _ => {}
    }
    if let Some(proposed_mb) = proposed.max_resident_mb {
        let limit_mb = env.physical_memory_mb / 2;
        if proposed_mb > limit_mb {
            violations.push(Violation::CeilingTooLarge {
                proposed_mb,
                limit_mb,
            });
        }
    }
    // (`validate` already bounds niceness above, so only the floor matters here.)
    if proposed.nice < MIN_PROPOSED_NICE {
        violations.push(Violation::PriorityTooHigh {
            proposed: proposed.nice,
        });
    }
    if proposed.backend == BackendKind::Both && proposed.max_resident_mb.is_none() {
        violations.push(Violation::BothNeedsACeiling);
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

/// A stable fingerprint of `settings`, so an approval can name exactly the
/// proposal the person saw. Two equal settings always share one digest.
pub fn digest(settings: &AssistantSettings) -> String {
    let canonical = serde_json::to_vec(settings).expect("settings always serialize");
    Sha256::digest(canonical)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Human-readable `label: current -> proposed` lines for only the fields that
/// differ, for the approval window.
pub fn diff_lines(current: &AssistantSettings, proposed: &AssistantSettings) -> Vec<String> {
    let mut lines = Vec::new();
    let mut push = |label: &str, before: String, after: String| {
        if before != after {
            lines.push(format!("{label}: {before} -> {after}"));
        }
    };
    let backend = |b: BackendKind| format!("{b:?}").to_lowercase();
    push(
        "Backend",
        backend(current.backend),
        backend(proposed.backend),
    );
    let loopback = |s: &AssistantSettings| {
        s.loopback.as_ref().map_or("none".to_string(), |l| {
            format!("{} {} {}", l.provider, l.base_url, l.model)
        })
    };
    push("Loopback model", loopback(current), loopback(proposed));
    let model = |s: &AssistantSettings| {
        s.candle
            .as_ref()
            .map_or("none".to_string(), |c| c.model_path.clone())
    };
    push("Candle model", model(current), model(proposed));
    let tokenizer = |s: &AssistantSettings| {
        s.candle
            .as_ref()
            .map_or("none".to_string(), |c| c.tokenizer_path.clone())
    };
    push("Candle tokenizer", tokenizer(current), tokenizer(proposed));
    let context = |s: &AssistantSettings| {
        s.candle
            .as_ref()
            .map_or("none".to_string(), |c| c.context.to_string())
    };
    push("Candle context", context(current), context(proposed));
    push(
        "Idle timeout (s)",
        current.idle_timeout_secs.to_string(),
        proposed.idle_timeout_secs.to_string(),
    );
    let ceiling = |s: &AssistantSettings| {
        s.max_resident_mb
            .map_or("no limit".to_string(), |mb| format!("{mb} MiB"))
    };
    push("Memory ceiling", ceiling(current), ceiling(proposed));
    push(
        "Priority (nice)",
        current.nice.to_string(),
        proposed.nice.to_string(),
    );
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CandleSettings, LoopbackSettings, DEFAULT_CANDLE_CONTEXT};

    fn candle(dir: &str) -> CandleSettings {
        CandleSettings {
            model_path: format!("{dir}/qwen3.gguf"),
            tokenizer_path: format!("{dir}/tokenizer.json"),
            context: DEFAULT_CANDLE_CONTEXT,
        }
    }

    fn current() -> AssistantSettings {
        AssistantSettings {
            backend: BackendKind::Candle,
            candle: Some(candle("/models/qwen")),
            max_resident_mb: Some(2048),
            nice: 10,
            ..AssistantSettings::default()
        }
    }

    /// A fake world: these files exist, symlink `/models/qwen/escape.gguf` leads
    /// out of the allowed directory, and the machine has 16 GiB.
    fn with_env<R>(f: impl FnOnce(&Environment<'_>) -> R) -> R {
        let exists = |p: &Path| {
            matches!(
                p.to_str().unwrap(),
                "/models/qwen/qwen3.gguf"
                    | "/models/qwen/tokenizer.json"
                    | "/models/qwen/escape.gguf"
                    | "/models/other/qwen3.gguf"
                    | "/models/other/tokenizer.json"
                    | "/store/models/new.gguf"
                    | "/store/models/new.json"
                    | "/etc/passwd.gguf"
            )
        };
        let canon = |p: &Path| -> Option<PathBuf> {
            Some(match p.to_str().unwrap() {
                "/models/qwen/escape.gguf" => PathBuf::from("/etc/passwd.gguf"),
                other => PathBuf::from(other),
            })
        };
        f(&Environment {
            is_regular_file: &exists,
            canonicalize: &canon,
            physical_memory_mb: 16 * 1024,
            models_dir: PathBuf::from("/store/models"),
        })
    }

    fn verdict(proposed: &AssistantSettings) -> Result<(), Vec<Violation>> {
        with_env(|env| evaluate(&current(), proposed, env))
    }

    fn with(edit: impl FnOnce(&mut AssistantSettings)) -> AssistantSettings {
        let mut s = current();
        edit(&mut s);
        s
    }

    #[test]
    fn a_modest_change_within_every_rule_may_be_shown_to_the_person() {
        assert_eq!(verdict(&with(|s| s.nice = 12)), Ok(()));
        assert_eq!(verdict(&with(|s| s.idle_timeout_secs = 120)), Ok(()));
        assert_eq!(verdict(&with(|s| s.max_resident_mb = Some(3000))), Ok(()));
        assert_eq!(
            verdict(&with(|s| s.max_resident_mb = Some(1024))),
            Ok(()),
            "tightening is fine"
        );
        assert_eq!(
            verdict(&with(|s| s.candle.as_mut().unwrap().context = 8192)),
            Ok(())
        );
    }

    #[test]
    fn a_proposal_that_changes_nothing_is_blocked() {
        assert_eq!(verdict(&current()), Err(vec![Violation::NoChange]));
    }

    #[test]
    fn invalid_settings_are_reported_alone_as_invalid() {
        let bad = with(|s| s.idle_timeout_secs = 1);
        let violations = verdict(&bad).unwrap_err();
        assert_eq!(violations.len(), 1);
        assert!(matches!(&violations[0], Violation::Invalid(_)));
    }

    #[test]
    fn a_model_file_must_be_a_real_file_of_the_right_kind_inside_an_allowed_directory() {
        let cases: [(&str, &str, &str); 5] = [
            (
                "/models/qwen/../../etc/x.gguf",
                "/models/qwen/tokenizer.json",
                "`..`",
            ),
            (
                "/models/qwen/qwen3.bin",
                "/models/qwen/tokenizer.json",
                ".gguf",
            ),
            (
                "/models/qwen/missing.gguf",
                "/models/qwen/tokenizer.json",
                "existing regular file",
            ),
            (
                "/models/qwen/escape.gguf",
                "/models/qwen/tokenizer.json",
                "inside the configured",
            ),
            (
                "/models/qwen/qwen3.gguf",
                "/models/qwen/tokenizer.txt",
                ".json",
            ),
        ];
        for (model, tokenizer, expected) in cases {
            let proposed = with(|s| {
                let c = s.candle.as_mut().unwrap();
                c.model_path = model.into();
                c.tokenizer_path = tokenizer.into();
            });
            let violations = verdict(&proposed).unwrap_err();
            assert!(
                violations.iter().any(|v| v.to_string().contains(expected)),
                "{model} / {tokenizer}: expected {expected:?} in {violations:?}"
            );
        }
    }

    #[test]
    fn the_blueice_models_directory_is_allowed_and_an_unconfigured_directory_is_not() {
        // The BlueIce models directory is allowed even though it is not where the
        // current model lives.
        let store = with(|s| {
            let c = s.candle.as_mut().unwrap();
            c.model_path = "/store/models/new.gguf".into();
            c.tokenizer_path = "/store/models/new.json".into();
        });
        assert_eq!(verdict(&store), Ok(()));
        // A different directory the person never configured is not.
        let elsewhere = with(|s| {
            let c = s.candle.as_mut().unwrap();
            c.model_path = "/models/other/qwen3.gguf".into();
            c.tokenizer_path = "/models/other/tokenizer.json".into();
        });
        assert!(verdict(&elsewhere).is_err());
    }

    #[test]
    fn with_no_model_configured_only_the_blueice_models_directory_is_allowed() {
        let none = AssistantSettings::default();
        let proposed = AssistantSettings {
            backend: BackendKind::Candle,
            candle: Some(candle("/models/other")),
            ..AssistantSettings::default()
        };
        assert!(with_env(|env| evaluate(&none, &proposed, env)).is_err());
        let ok = AssistantSettings {
            backend: BackendKind::Candle,
            candle: Some(CandleSettings {
                model_path: "/store/models/new.gguf".into(),
                tokenizer_path: "/store/models/new.json".into(),
                context: 4096,
            }),
            ..AssistantSettings::default()
        };
        assert_eq!(with_env(|env| evaluate(&none, &ok, env)), Ok(()));
    }

    #[test]
    fn an_existing_ceiling_cannot_be_removed_or_more_than_doubled() {
        assert_eq!(
            verdict(&with(|s| s.max_resident_mb = None)),
            Err(vec![Violation::CeilingRemoved])
        );
        assert_eq!(
            verdict(&with(|s| s.max_resident_mb = Some(4096))),
            Ok(()),
            "exactly double"
        );
        let raised = verdict(&with(|s| s.max_resident_mb = Some(4097))).unwrap_err();
        assert_eq!(
            raised,
            vec![Violation::CeilingRaisedTooMuch {
                current_mb: 2048,
                proposed_mb: 4097
            }]
        );
    }

    #[test]
    fn no_ceiling_may_exceed_half_of_physical_memory() {
        // 16 GiB machine: 8192 MiB is the most.
        assert_eq!(verdict(&with(|s| s.max_resident_mb = Some(4096))), Ok(()));
        let none_before = AssistantSettings::default();
        let ask = |mb| AssistantSettings {
            max_resident_mb: Some(mb),
            ..AssistantSettings::default()
        };
        assert_eq!(
            with_env(|env| evaluate(&none_before, &ask(8192), env)),
            Ok(())
        );
        let too_big = with_env(|env| evaluate(&none_before, &ask(8193), env)).unwrap_err();
        assert_eq!(
            too_big,
            vec![Violation::CeilingTooLarge {
                proposed_mb: 8193,
                limit_mb: 8192
            }]
        );
    }

    #[test]
    fn a_proposal_cannot_ask_for_a_higher_priority_than_the_floor() {
        assert_eq!(verdict(&with(|s| s.nice = 5)), Ok(()));
        assert_eq!(
            verdict(&with(|s| s.nice = 4)),
            Err(vec![Violation::PriorityTooHigh { proposed: 4 }])
        );
        assert!(verdict(&with(|s| s.nice = 0)).is_err());
    }

    #[test]
    fn running_both_backends_requires_a_ceiling() {
        let none = AssistantSettings::default();
        let both = |ceiling| AssistantSettings {
            backend: BackendKind::Both,
            loopback: Some(LoopbackSettings {
                provider: "llamacpp".into(),
                base_url: "http://127.0.0.1:8080/v1/".into(),
                model: "m".into(),
            }),
            candle: Some(candle("/store/models").tap_paths()),
            max_resident_mb: ceiling,
            ..AssistantSettings::default()
        };
        assert_eq!(
            with_env(|env| evaluate(&none, &both(None), env)),
            Err(vec![Violation::BothNeedsACeiling])
        );
        assert_eq!(
            with_env(|env| evaluate(&none, &both(Some(2048)), env)),
            Ok(())
        );
    }

    impl CandleSettings {
        /// Points the paths at the fake world's BlueIce models directory files.
        fn tap_paths(mut self) -> Self {
            self.model_path = "/store/models/new.gguf".into();
            self.tokenizer_path = "/store/models/new.json".into();
            self
        }
    }

    #[test]
    fn every_violated_rule_is_reported_together() {
        let bad = with(|s| {
            s.max_resident_mb = None; // removed
            s.nice = 0; // too high a priority
            s.candle.as_mut().unwrap().model_path = "/models/qwen/missing.gguf".into();
        });
        let violations = verdict(&bad).unwrap_err();
        assert!(violations.contains(&Violation::CeilingRemoved));
        assert!(violations.contains(&Violation::PriorityTooHigh { proposed: 0 }));
        assert!(violations
            .iter()
            .any(|v| matches!(v, Violation::PathNotAllowed { .. })));
        assert_eq!(violations.len(), 3);
    }

    #[test]
    fn a_blocked_proposal_never_needs_a_person_to_read_a_bare_code() {
        // Every violation renders as a sentence.
        for violation in [
            Violation::Invalid("x".into()),
            Violation::NoChange,
            Violation::PathNotAllowed {
                field: "candle model_path",
                reason: "must be a .gguf file",
            },
            Violation::CeilingRemoved,
            Violation::CeilingRaisedTooMuch {
                current_mb: 1,
                proposed_mb: 9,
            },
            Violation::CeilingTooLarge {
                proposed_mb: 9,
                limit_mb: 4,
            },
            Violation::PriorityTooHigh { proposed: 1 },
            Violation::BothNeedsACeiling,
        ] {
            assert!(violation.to_string().len() > 10, "{violation:?}");
        }
    }

    #[test]
    fn the_digest_is_stable_and_distinguishes_any_difference() {
        assert_eq!(digest(&current()), digest(&current()));
        assert_eq!(digest(&current()).len(), 64);
        assert_ne!(digest(&current()), digest(&with(|s| s.nice = 11)));
        assert_ne!(
            digest(&current()),
            digest(&with(|s| s.max_resident_mb = Some(2049)))
        );
    }

    #[test]
    fn the_diff_lists_only_what_changed_in_words() {
        let proposed = with(|s| {
            s.nice = 15;
            s.max_resident_mb = Some(3000);
        });
        assert_eq!(
            diff_lines(&current(), &proposed),
            [
                "Memory ceiling: 2048 MiB -> 3000 MiB",
                "Priority (nice): 10 -> 15"
            ]
        );
        assert!(diff_lines(&current(), &current()).is_empty());
        let switched = with(|s| {
            s.backend = BackendKind::Both;
            s.loopback = Some(LoopbackSettings {
                provider: "ollama".into(),
                base_url: "http://127.0.0.1:11434/v1/".into(),
                model: "m".into(),
            });
        });
        let lines = diff_lines(&current(), &switched);
        assert!(lines.contains(&"Backend: candle -> both".to_string()));
        assert!(lines
            .iter()
            .any(|l| l.starts_with("Loopback model: none ->")));
    }
}
