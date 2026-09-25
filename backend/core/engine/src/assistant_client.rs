// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `core`'s client of `ai-assistant` for live translation
//! (`phase-7-local-ai/PLAN.md`, checklist step T3). It runs on a navigation's
//! background thread -- never `core`'s session loop -- after the gatekeeper has
//! reviewed the *original* HTML, and returns the translations the main thread
//! then substitutes by ordinal.
//!
//! Failure semantics are the opposite of the gatekeeper's: **fail-open to the
//! original**. An absent assistant, a refused handshake, a `Failed` reply, a
//! malformed or mismatched reply, or running past the deadline never blocks or
//! breaks the page. A batch that fails keeps the batches already translated and
//! leaves the rest of the text as the page wrote it.

use blueice_ipc::assistant::{
    read_assistant_reply, write_assistant_request, AssistantReply, AssistantRequest,
    ASSISTANT_PROTOCOL_VERSION, MAX_REQUEST_TEXT_BYTES, MAX_TRANSLATE_ITEMS,
};
use std::ops::Range;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Most batches (requests) one navigation may spend; text beyond them stays
/// original. Together with the per-batch bounds this caps the work a single
/// hostile page can cause.
const MAX_BATCHES: usize = 8;

/// Where the assistant listens and what to translate into.
#[derive(Debug, Clone)]
pub struct AssistantConfig {
    pub socket: PathBuf,
    /// A BCP 47 tag such as `zh-TW`; validated by the wire protocol.
    pub target_language: String,
    /// The whole navigation's translation budget, all batches together.
    pub deadline: Duration,
}

/// Splits `texts` into consecutive batches within the protocol's item and byte
/// bounds, at most [`MAX_BATCHES`] of them.
fn plan_batches(texts: &[String]) -> Vec<Range<usize>> {
    let mut batches = Vec::new();
    let mut start = 0;
    let mut bytes = 0;
    for (index, text) in texts.iter().enumerate() {
        if index > start
            && (index - start == MAX_TRANSLATE_ITEMS || bytes + text.len() > MAX_REQUEST_TEXT_BYTES)
        {
            batches.push(start..index);
            start = index;
            bytes = 0;
        }
        bytes += text.len();
    }
    if start < texts.len() {
        batches.push(start..texts.len());
    }
    batches.truncate(MAX_BATCHES);
    batches
}

/// Translates the translatable text of `html`, one string per entry of
/// [`crate::translation::translatable_texts`] in the same order (untranslated
/// entries carry their original text). `None` means nothing was translated --
/// keep the original page.
pub fn translate_html(config: &AssistantConfig, html: &str) -> Option<Vec<String>> {
    let doc = blueice_html::parse(html);
    let sources: Vec<String> = crate::translation::translatable_texts(&doc)
        .into_iter()
        .map(|entry| entry.text)
        .collect();
    if sources.is_empty() {
        return None;
    }
    let started = Instant::now();
    let mut stream = UnixStream::connect(&config.socket).ok()?;
    handshake(&mut stream, remaining(started, config.deadline)?).ok()?;

    let mut out = sources.clone();
    let mut translated_any = false;
    for (n, range) in plan_batches(&sources).into_iter().enumerate() {
        let Some(budget) = remaining(started, config.deadline) else {
            break;
        };
        match translate_batch(
            &mut stream,
            budget,
            n as u64 + 1,
            &config.target_language,
            &sources[range.clone()],
        ) {
            Some(translated) => {
                out[range].clone_from_slice(&translated);
                translated_any = true;
            }
            None => break,
        }
    }
    translated_any.then_some(out)
}

fn remaining(started: Instant, deadline: Duration) -> Option<Duration> {
    deadline
        .checked_sub(started.elapsed())
        .filter(|left| !left.is_zero())
}

fn handshake(stream: &mut UnixStream, budget: Duration) -> Result<(), ()> {
    limit(stream, budget).map_err(|_| ())?;
    write_assistant_request(
        stream,
        &AssistantRequest::Hello {
            protocol_version: ASSISTANT_PROTOCOL_VERSION,
        },
    )
    .map_err(|_| ())?;
    match read_assistant_reply(stream) {
        Ok(AssistantReply::HelloAck { protocol_version })
            if protocol_version == ASSISTANT_PROTOCOL_VERSION =>
        {
            Ok(())
        }
        _ => Err(()),
    }
}

fn limit(stream: &UnixStream, budget: Duration) -> std::io::Result<()> {
    stream.set_read_timeout(Some(budget))?;
    stream.set_write_timeout(Some(budget))
}

fn translate_batch(
    stream: &mut UnixStream,
    budget: Duration,
    request_id: u64,
    target_language: &str,
    texts: &[String],
) -> Option<Vec<String>> {
    limit(stream, budget).ok()?;
    let request = AssistantRequest::Translate {
        request_id,
        target_language: target_language.to_string(),
        texts: texts.to_vec(),
    };
    request.validate().ok()?;
    write_assistant_request(stream, &request).ok()?;
    match read_assistant_reply(stream).ok()? {
        AssistantReply::Translated {
            request_id: replied,
            texts: translated,
        } if replied == request_id && translated.len() == texts.len() => Some(translated),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::assistant::read_assistant_request;
    use std::os::unix::net::UnixListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    fn socket_path(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("as-{tag}-{}-{n}.sock", std::process::id()))
    }

    /// How the fake assistant answers one `Translate` batch.
    enum Answer {
        Upper,
        Fail,
        WrongLength,
        WrongId,
        Silent,
    }

    /// A one-connection fake assistant: acks `Hello`, then answers each
    /// `Translate` per `answers` (the last answer repeats). Records the size
    /// of every batch it saw.
    fn fake_assistant(
        socket: &std::path::Path,
        answers: Vec<Answer>,
    ) -> (thread::JoinHandle<()>, Arc<Mutex<Vec<usize>>>) {
        let listener = UnixListener::bind(socket).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let Ok(AssistantRequest::Hello { .. }) = read_assistant_request(&mut stream) else {
                return;
            };
            write_assistant_reply(
                &mut stream,
                &AssistantReply::HelloAck {
                    protocol_version: ASSISTANT_PROTOCOL_VERSION,
                },
            )
            .unwrap();
            let mut answered = 0;
            while let Ok(AssistantRequest::Translate {
                request_id, texts, ..
            }) = read_assistant_request(&mut stream)
            {
                record.lock().unwrap().push(texts.len());
                let answer = &answers[answered.min(answers.len() - 1)];
                answered += 1;
                let reply = match answer {
                    Answer::Upper => AssistantReply::Translated {
                        request_id,
                        texts: texts.iter().map(|t| t.to_uppercase()).collect(),
                    },
                    Answer::Fail => AssistantReply::Failed {
                        request_id,
                        reason: "down".into(),
                    },
                    Answer::WrongLength => AssistantReply::Translated {
                        request_id,
                        texts: vec![],
                    },
                    Answer::WrongId => AssistantReply::Translated {
                        request_id: request_id + 100,
                        texts,
                    },
                    Answer::Silent => {
                        thread::sleep(Duration::from_secs(1));
                        return;
                    }
                };
                if write_assistant_reply(&mut stream, &reply).is_err() {
                    return;
                }
            }
        });
        (worker, seen)
    }

    use blueice_ipc::assistant::write_assistant_reply;

    fn config(socket: &std::path::Path) -> AssistantConfig {
        AssistantConfig {
            socket: socket.to_path_buf(),
            target_language: "zh-TW".into(),
            deadline: Duration::from_secs(5),
        }
    }

    #[test]
    fn a_page_is_translated_in_order_over_one_connection() {
        let socket = socket_path("ok");
        let (worker, seen) = fake_assistant(&socket, vec![Answer::Upper]);
        let out = translate_html(&config(&socket), "<h1>one</h1><p>two <b>three</b></p>");
        assert_eq!(out.unwrap(), ["ONE", "TWO", "THREE"]);
        worker.join().unwrap();
        assert_eq!(*seen.lock().unwrap(), [3]);
        let _ = std::fs::remove_file(&socket);
    }

    #[test]
    fn many_texts_are_split_into_bounded_batches() {
        let socket = socket_path("batch");
        let (worker, seen) = fake_assistant(&socket, vec![Answer::Upper]);
        let html: String = (0..300).map(|i| format!("<p>t{i}</p>")).collect();
        let out = translate_html(&config(&socket), &html).unwrap();
        assert_eq!(out.len(), 300);
        assert_eq!(out[0], "T0");
        assert_eq!(out[299], "T299");
        worker.join().unwrap();
        assert_eq!(
            *seen.lock().unwrap(),
            [MAX_TRANSLATE_ITEMS, 300 - MAX_TRANSLATE_ITEMS]
        );
        let _ = std::fs::remove_file(&socket);
    }

    #[test]
    fn a_failed_later_batch_keeps_the_earlier_batches_translated() {
        let socket = socket_path("partial");
        let (worker, _) = fake_assistant(&socket, vec![Answer::Upper, Answer::Fail]);
        let html: String = (0..300).map(|i| format!("<p>t{i}</p>")).collect();
        let out = translate_html(&config(&socket), &html).unwrap();
        assert_eq!(out[0], "T0");
        assert_eq!(
            out[MAX_TRANSLATE_ITEMS - 1],
            format!("T{}", MAX_TRANSLATE_ITEMS - 1)
        );
        // The failed batch is the page's own text, not a guess.
        assert_eq!(out[MAX_TRANSLATE_ITEMS], format!("t{MAX_TRANSLATE_ITEMS}"));
        worker.join().unwrap();
        let _ = std::fs::remove_file(&socket);
    }

    #[test]
    fn every_failure_mode_on_the_first_batch_keeps_the_original_page() {
        for answer in [Answer::Fail, Answer::WrongLength, Answer::WrongId] {
            let socket = socket_path("fail");
            let (worker, _) = fake_assistant(&socket, vec![answer]);
            assert_eq!(translate_html(&config(&socket), "<p>hello</p>"), None);
            worker.join().unwrap();
            let _ = std::fs::remove_file(&socket);
        }
    }

    #[test]
    fn an_absent_assistant_keeps_the_original_page() {
        let cfg = config(&socket_path("absent"));
        assert_eq!(translate_html(&cfg, "<p>hello</p>"), None);
    }

    #[test]
    fn a_refused_handshake_keeps_the_original_page() {
        let socket = socket_path("hello");
        let listener = UnixListener::bind(&socket).unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_assistant_request(&mut stream);
            write_assistant_reply(
                &mut stream,
                &AssistantReply::Failed {
                    request_id: 0,
                    reason: "version".into(),
                },
            )
            .unwrap();
        });
        assert_eq!(translate_html(&config(&socket), "<p>hello</p>"), None);
        worker.join().unwrap();
        let _ = std::fs::remove_file(&socket);
    }

    #[test]
    fn a_silent_assistant_is_abandoned_at_the_deadline() {
        let socket = socket_path("slow");
        let (worker, _) = fake_assistant(&socket, vec![Answer::Silent]);
        let cfg = AssistantConfig {
            deadline: Duration::from_millis(300),
            ..config(&socket)
        };
        let started = Instant::now();
        assert_eq!(translate_html(&cfg, "<p>hello</p>"), None);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "must give up at the deadline, not wait for the assistant"
        );
        worker.join().unwrap();
        let _ = std::fs::remove_file(&socket);
    }

    #[test]
    fn a_page_with_nothing_to_translate_never_contacts_the_assistant() {
        let socket = socket_path("empty");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let cfg = config(&socket);
        assert_eq!(translate_html(&cfg, "<script>var a=1</script>"), None);
        assert_eq!(translate_html(&cfg, ""), None);
        assert!(
            matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "no connection may be opened for a page with no translatable text"
        );
        let _ = std::fs::remove_file(&socket);
    }

    #[test]
    fn an_exhausted_budget_stops_before_any_request() {
        let socket = socket_path("zero");
        let (_worker, seen) = fake_assistant(&socket, vec![Answer::Upper]);
        let cfg = AssistantConfig {
            deadline: Duration::ZERO,
            ..config(&socket)
        };
        assert_eq!(translate_html(&cfg, "<p>hello</p>"), None);
        assert!(seen.lock().unwrap().is_empty());
        let _ = std::fs::remove_file(&socket);
    }

    fn strings(sizes: &[usize]) -> Vec<String> {
        sizes.iter().map(|n| "a".repeat(*n)).collect()
    }

    #[test]
    fn batches_respect_the_item_and_byte_bounds() {
        assert_eq!(plan_batches(&strings(&[])), Vec::<Range<usize>>::new());
        assert_eq!(plan_batches(&strings(&[1, 1, 1])), vec![0..3]);
        let many = vec!["a".to_string(); MAX_TRANSLATE_ITEMS + 1];
        assert_eq!(
            plan_batches(&many),
            [
                0..MAX_TRANSLATE_ITEMS,
                MAX_TRANSLATE_ITEMS..MAX_TRANSLATE_ITEMS + 1
            ]
        );
        // Eight 8 KiB items fill a 64 KiB batch exactly; the ninth starts the next.
        let eight_k = 8 * 1024;
        assert_eq!(plan_batches(&strings(&[eight_k; 9])), [0..8, 8..9]);
    }

    #[test]
    fn batching_is_capped_so_a_hostile_page_cannot_cause_unbounded_work() {
        let eight_k = 8 * 1024;
        let batches = plan_batches(&strings(&vec![eight_k; 8 * (MAX_BATCHES + 3)]));
        assert_eq!(batches.len(), MAX_BATCHES);
        assert_eq!(batches[0], 0..8);
    }
}
