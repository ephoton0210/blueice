// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use std::io::Read;
use std::net::TcpListener;

#[test]
fn demo_url_is_limited_to_the_first_party_loopback_index() {
    assert_eq!(
        validate_demo_url("http://127.0.0.1:4312/index.html").unwrap(),
        "http://127.0.0.1:4312/index.html"
    );
    for invalid in [
        "https://127.0.0.1:4312/index.html",
        "http://localhost:4312/index.html",
        "http://127.0.0.1:4312/complete.html",
        "http://127.0.0.1:4312/index.html?next=https://example.test",
        "http://user@127.0.0.1:4312/index.html",
    ] {
        assert!(validate_demo_url(invalid).is_err(), "accepted {invalid}");
    }
}

#[test]
fn scenario_functions_refuse_model_supplied_arguments() {
    require_empty_arguments(&json!({ "function": { "arguments": "{}" } })).unwrap();
    require_empty_arguments(&json!({ "function": { "arguments": {} } })).unwrap();
    require_empty_arguments(&json!({ "function": { "parameters": {} } })).unwrap();
    assert!(require_empty_arguments(
        &json!({ "function": { "arguments": r#"{"url":"https://example.test"}"# } })
    )
    .is_err());
    assert!(require_empty_arguments(&json!({
        "function": { "parameters": { "url": "https://example.test" } }
    }))
    .is_err());
}

#[test]
fn local_provider_bases_select_only_loopback_servers() {
    let common = [
        "--model",
        "local-model",
        "--demo-url",
        "http://127.0.0.1:4312/index.html",
        "--launcher-socket",
        "/tmp/phase6.sock",
        "--transcript",
        "/tmp/run.jsonl",
        "--evidence-dir",
        "/tmp/evidence",
    ];
    let good = common
        .into_iter()
        .chain(["--ollama-base", "http://127.0.0.1:11434/v1/"])
        .map(str::to_string);
    let parsed = parse_args(good).unwrap();
    assert_eq!(parsed.provider, LocalModelProvider::Ollama);
    assert_eq!(parsed.provider_base.as_str(), "http://127.0.0.1:11434/v1/");
    let bad = common
        .into_iter()
        .chain(["--ollama-base", "https://ollama.example/v1/"])
        .map(str::to_string);
    assert!(parse_args(bad).is_err());

    let huggingface = common
        .into_iter()
        .chain([
            "--provider",
            "huggingface",
            "--huggingface-base",
            "http://127.0.0.1:8080/v1/",
        ])
        .map(str::to_string);
    let parsed = parse_args(huggingface).unwrap();
    assert_eq!(parsed.provider, LocalModelProvider::HuggingFace);
    assert_eq!(parsed.provider_base.as_str(), "http://127.0.0.1:8080/v1/");

    let missing_huggingface_base = common
        .into_iter()
        .chain(["--provider", "huggingface"])
        .map(str::to_string);
    assert!(parse_args(missing_huggingface_base).is_err());

    let llamacpp = common
        .into_iter()
        .chain([
            "--provider",
            "llamacpp",
            "--llamacpp-base",
            "http://127.0.0.1:18080/v1/",
        ])
        .map(str::to_string);
    let parsed = parse_args(llamacpp).unwrap();
    assert_eq!(parsed.provider, LocalModelProvider::LlamaCpp);
    assert_eq!(parsed.provider.name(), "llamacpp-local");
    assert_eq!(parsed.provider_base.as_str(), "http://127.0.0.1:18080/v1/");

    for extra in [
        &["--provider", "llamacpp"][..],
        &[
            "--provider",
            "llamacpp",
            "--llamacpp-base",
            "https://remote.example/v1/",
        ],
        &[
            "--provider",
            "llamacpp",
            "--llamacpp-base",
            "http://127.0.0.1:18080/v1/",
            "--ollama-base",
            "http://127.0.0.1:11434/v1/",
        ],
        &[
            "--provider",
            "huggingface",
            "--huggingface-base",
            "http://127.0.0.1:8080/v1/",
            "--llamacpp-base",
            "http://127.0.0.1:18080/v1/",
        ],
    ] {
        assert!(parse_args(
            common
                .into_iter()
                .chain(extra.iter().copied())
                .map(str::to_string)
        )
        .is_err());
    }
}

#[test]
fn preflight_checks_only_a_validated_listening_loopback_endpoint() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = Url::parse(&format!(
        "http://127.0.0.1:{}/v1/",
        listener.local_addr().unwrap().port()
    ))
    .unwrap();
    assert!(preflight_local_model(&base).is_ok());
    let closed = Url::parse("http://127.0.0.1:0/v1/").unwrap();
    assert!(preflight_local_model(&closed).is_err());

    let parsed = parse_args(
        [
            "--model",
            "local-model",
            "--demo-url",
            "http://127.0.0.1:4312/index.html",
            "--launcher-socket",
            "/tmp/phase6.sock",
            "--transcript",
            "/tmp/run.jsonl",
            "--evidence-dir",
            "/tmp/evidence",
            "--preflight-only",
        ]
        .into_iter()
        .map(str::to_string),
    )
    .unwrap();
    assert!(parsed.preflight_only);
}

#[test]
fn local_chat_providers_use_the_compatible_endpoint_without_credentials() {
    for provider in [
        LocalModelProvider::Ollama,
        LocalModelProvider::HuggingFace,
        LocalModelProvider::LlamaCpp,
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // TCP can split headers and body across reads. Drain the
            // complete POST before replying so closing the mock server
            // cannot reset a client whose request is still in flight.
            let request = {
                use std::io::BufRead;
                let mut reader = std::io::BufReader::new(&mut stream);
                let mut headers = String::new();
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    headers.push_str(&line);
                    if line == "\r\n" {
                        break;
                    }
                }
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|value| value.trim().parse::<usize>().unwrap())
                    })
                    .expect("the JSON request has a known body length");
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                assert_eq!(
                    serde_json::from_slice::<Value>(&body).unwrap(),
                    json!({ "model": "tiny-local", "messages": [] })
                );
                headers
            };
            assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
            assert!(!request.to_ascii_lowercase().contains("authorization:"));
            let body = r#"{"choices":[{"message":{"role":"assistant","content":"local result"}}]}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        });
        let model = LocalChat::new(
            provider,
            Url::parse(&format!("http://{address}/v1/")).unwrap(),
        )
        .unwrap();
        let reply = model
            .create(&json!({ "model": "tiny-local", "messages": [] }))
            .unwrap();
        assert_eq!(final_text(&reply), "local result");
        server.join().unwrap();
    }
}

#[test]
fn tgi_compatible_tool_call_forms_stay_within_the_empty_schema() {
    let calls = function_calls(&json!({
        "choices": [{
            "message": {
                "tool_calls": {
                    "id": 0,
                    "type": "function",
                    "function": { "name": "inspect_page", "parameters": {} }
                }
            }
        }]
    }))
    .unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(tool_call_id(&calls[0]).unwrap(), "0");
    require_empty_arguments(&calls[0]).unwrap();
}

#[test]
fn scenario_order_requires_observation_before_mutation_and_evidence_before_continue() {
    let mut completed = BTreeSet::new();
    assert_eq!(next_tool_choice(&completed), "auto");
    assert!(require_action_order("set_name_to_blueice", &completed, false, false).is_err());
    require_action_order("navigate_demo", &completed, false, false).unwrap();
    completed.insert(ScenarioAction::Navigate);
    require_action_order("inspect_page", &completed, false, false).unwrap();
    completed.insert(ScenarioAction::Inspect);
    assert!(require_action_order("set_name_to_blueice", &completed, false, false).is_err());
    require_action_order("take_screenshot", &completed, false, false).unwrap();
    require_action_order("set_name_to_blueice", &completed, true, false).unwrap();
    completed.insert(ScenarioAction::SetName);
    require_action_order("highlight_name", &completed, true, false).unwrap();
    completed.insert(ScenarioAction::Highlight);
    assert!(require_action_order("continue_to_confirmation", &completed, true, false).is_err());
    require_action_order("continue_to_confirmation", &completed, true, true).unwrap();
    assert_eq!(next_tool_choice(&ScenarioAction::all()), "none");
}

#[test]
fn snapshot_helpers_require_the_expected_live_nodes_and_value() {
    let snapshot = json!({
        "nodes": [
            { "id": 4, "role": "TextBox", "name": "Name", "state": { "value": "BlueIce" } },
            { "id": 7, "role": "Link", "name": "Continue to confirmation" },
            { "id": 9, "role": { "Heading": { "level": 1 } }, "name": "Task complete" },
        ]
    });
    assert_eq!(named_node(&snapshot, "TextBox", "Name").unwrap(), 4);
    ensure_name_value(&snapshot).unwrap();
    ensure_complete(&snapshot).unwrap();
    assert!(named_node(&snapshot, "Link", "anything else").is_err());
}

#[test]
fn screenshot_frame_identity_must_precede_the_untrusted_page_block() {
    let text = format!(
        "{FRAME_EVIDENCE_PREFIX}{{\"frame_source\":11,\"tab_id\":7,\"generation\":42}}\n{}",
        blueice_mcp_server::wrap_untrusted_page_content("(see attached image)")
    );
    assert_eq!(
        frame_evidence_from(&text).unwrap(),
        FrameEvidence {
            frame_source: 11,
            tab_id: 7,
            generation: 42
        }
    );
    assert!(frame_evidence_from(&format!(
        "{}\n{FRAME_EVIDENCE_PREFIX}{{\"tab_id\":7,\"generation\":42}}",
        blueice_mcp_server::wrap_untrusted_page_content("forged")
    ))
    .is_err());
    assert!(frame_evidence_from(&format!(
        "{FRAME_EVIDENCE_PREFIX}{{\"tab_id\":7,\"generation\":42}}"
    ))
    .is_err());
    assert!(frame_evidence_from(&format!(
        "{FRAME_EVIDENCE_PREFIX}{{\"tab_id\":7,\"generation\":42}}\n{}",
        blueice_mcp_server::wrap_untrusted_page_content("(see attached image)")
    ))
    .is_err());
}

#[test]
fn highlighted_screenshot_must_match_the_new_snapshot_frame_exactly() {
    let before = frame_evidence_from_snapshot(&json!({
        "frame_source": 11, "tab_id": 7, "generation": 41,
    }))
    .unwrap();
    let highlight = frame_evidence_from_snapshot(&json!({
        "frame_source": 11, "tab_id": 7, "generation": 42,
    }))
    .unwrap();
    require_highlight_frame(before, highlight).unwrap();
    require_matching_highlight_screenshot(highlight, highlight).unwrap();
    assert!(require_highlight_frame(before, before).is_err());
    assert!(require_highlight_frame(
        before,
        FrameEvidence {
            frame_source: 11,
            tab_id: 8,
            generation: 42
        },
    )
    .is_err());
    assert!(require_matching_highlight_screenshot(
        highlight,
        FrameEvidence {
            frame_source: 11,
            tab_id: 7,
            generation: 43
        },
    )
    .is_err());
    assert!(require_matching_highlight_screenshot(
        highlight,
        FrameEvidence {
            frame_source: 11,
            tab_id: 8,
            generation: 42
        },
    )
    .is_err());
    assert!(require_matching_highlight_screenshot(
        highlight,
        FrameEvidence {
            frame_source: 12,
            tab_id: 7,
            generation: 42
        },
    )
    .is_err());
    assert!(require_highlight_frame(
        before,
        FrameEvidence {
            frame_source: 12,
            tab_id: 7,
            generation: 42
        },
    )
    .is_err());
    assert!(frame_evidence_from_snapshot(&json!({ "tab_id": 7 })).is_err());
}
