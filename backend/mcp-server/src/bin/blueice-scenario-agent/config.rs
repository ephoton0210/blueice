// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[derive(Debug)]
pub(super) struct Args {
    pub(super) model: String,
    pub(super) provider: LocalModelProvider,
    pub(super) provider_base: Url,
    pub(super) demo_url: String,
    pub(super) launcher_socket: PathBuf,
    pub(super) mcp_server: PathBuf,
    pub(super) transcript: PathBuf,
    pub(super) evidence_dir: PathBuf,
    pub(super) max_turns: usize,
    pub(super) highlight_hold_secs: u64,
    pub(super) preflight_only: bool,
}

pub(super) fn usage() -> &'static str {
    r#"usage: blueice-phase6-agent --model <model> --demo-url <http://127.0.0.1:port/index.html>
  --launcher-socket <rendezvous.sock> --transcript <run.jsonl> --evidence-dir <dir>
  [--mcp-server <blueice-mcp-server>] [--provider <ollama|huggingface|llamacpp>]
  [--ollama-base <http://127.0.0.1:11434/v1/>]
  [--huggingface-base <http://127.0.0.1:8080/v1/>]
  [--llamacpp-base <http://127.0.0.1:8080/v1/>]
  [--max-turns <n>] [--highlight-hold-seconds <n>] [--preflight-only]"#
}

/// The model backend is deliberately a local server implementation, rather
/// than a cloud account. Hugging Face means a self-operated local TGI server,
/// not Hugging Face Inference Endpoints. llama.cpp is a separate compatible
/// local server, so a real run never needs to be mislabeled as a TGI run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LocalModelProvider {
    Ollama,
    HuggingFace,
    LlamaCpp,
}

impl LocalModelProvider {
    pub(super) fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "ollama" => Ok(Self::Ollama),
            "huggingface" | "hf" => Ok(Self::HuggingFace),
            "llamacpp" => Ok(Self::LlamaCpp),
            _ => Err("--provider must be ollama, huggingface, or llamacpp".to_string()),
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::HuggingFace => "huggingface-local",
            Self::LlamaCpp => "llamacpp-local",
        }
    }
}

pub(super) fn next_value(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{flag} requires a value"))
}

pub(super) fn default_mcp_server() -> Result<PathBuf, String> {
    let executable =
        env::current_exe().map_err(|error| format!("locating own executable: {error}"))?;
    let directory = executable
        .parent()
        .ok_or_else(|| "the agent executable has no parent directory".to_string())?;
    let name = if cfg!(windows) {
        "blueice-mcp-server.exe"
    } else {
        "blueice-mcp-server"
    };
    Ok(directory.join(name))
}

/// Ensures this runner can never instruct the shared browser to leave the
/// first-party loopback fixture. The continuation page is reached only by the
/// core-owned click operation, never by a model-supplied navigation URL.
pub(super) fn validate_demo_url(raw: &str) -> Result<String, String> {
    let url = Url::parse(raw).map_err(|error| format!("invalid --demo-url: {error}"))?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none()
        || url.path() != "/index.html"
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(
            "--demo-url must be exactly a credential-free http://127.0.0.1:<port>/index.html URL"
                .to_string(),
        );
    }
    Ok(url.into())
}

/// Restrict model requests to a self-operated local server. In particular,
/// this prevents an apparently interchangeable OpenAI-compatible endpoint
/// from becoming an unrecorded cloud-model integration.
pub(super) fn parse_loopback_chat_base(
    raw: &str,
    flag: &str,
    provider: LocalModelProvider,
) -> Result<Url, String> {
    let base = Url::parse(raw).map_err(|error| format!("invalid {flag}: {error}"))?;
    if !matches!(base.scheme(), "http" | "https")
        || !matches!(
            base.host_str(),
            Some("127.0.0.1") | Some("localhost") | Some("::1")
        )
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
        || !base.path().ends_with("/v1/")
    {
        return Err(format!(
            "{flag} must be a credential-free loopback http(s)://<host>:<port>/v1/ {} server",
            provider.name()
        ));
    }
    Ok(base)
}

/// Check only whether a previously validated local endpoint accepts TCP.
/// Do not send a model request: readiness of a particular model is confirmed
/// by the actual run, and no task content should leave the run before then.
pub(super) fn preflight_local_model(base: &Url) -> Result<(), String> {
    let host = base.host_str().ok_or("the local model URL has no host")?;
    let port = base
        .port_or_known_default()
        .ok_or("the local model URL has no usable port")?;
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|error| format!("resolving the local model endpoint: {error}"))?;
    for address in addresses.filter(|address| address.ip().is_loopback()) {
        if TcpStream::connect_timeout(&address, Duration::from_secs(2)).is_ok() {
            return Ok(());
        }
    }
    Err(format!(
        "the local model endpoint {base} is not accepting loopback TCP connections; start the selected provider before opening the human evidence window"
    ))
}

pub(super) fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut model = None;
    let mut demo_url = None;
    let mut launcher_socket = None;
    let mut mcp_server = None;
    let mut transcript = None;
    let mut evidence_dir = None;
    let mut provider = None;
    let mut ollama_base = None;
    let mut huggingface_base = None;
    let mut llamacpp_base = None;
    let mut max_turns = None;
    let mut highlight_hold_secs = None;
    let mut preflight_only = false;
    let mut args = args;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--model" => model = Some(next_value(&mut args, "--model")?),
            "--demo-url" => demo_url = Some(next_value(&mut args, "--demo-url")?),
            "--launcher-socket" => {
                launcher_socket = Some(PathBuf::from(next_value(&mut args, "--launcher-socket")?))
            }
            "--mcp-server" => {
                mcp_server = Some(PathBuf::from(next_value(&mut args, "--mcp-server")?))
            }
            "--transcript" => {
                transcript = Some(PathBuf::from(next_value(&mut args, "--transcript")?))
            }
            "--evidence-dir" => {
                evidence_dir = Some(PathBuf::from(next_value(&mut args, "--evidence-dir")?))
            }
            "--provider" => provider = Some(next_value(&mut args, "--provider")?),
            "--ollama-base" => ollama_base = Some(next_value(&mut args, "--ollama-base")?),
            "--huggingface-base" => {
                huggingface_base = Some(next_value(&mut args, "--huggingface-base")?)
            }
            "--llamacpp-base" => llamacpp_base = Some(next_value(&mut args, "--llamacpp-base")?),
            "--max-turns" => {
                let raw = next_value(&mut args, "--max-turns")?;
                let parsed = raw
                    .parse::<usize>()
                    .map_err(|_| "--max-turns must be a positive integer".to_string())?;
                if parsed == 0 {
                    return Err("--max-turns must be a positive integer".to_string());
                }
                max_turns = Some(parsed);
            }
            "--highlight-hold-seconds" => {
                let raw = next_value(&mut args, "--highlight-hold-seconds")?;
                highlight_hold_secs = Some(raw.parse::<u64>().map_err(|_| {
                    "--highlight-hold-seconds must be a non-negative integer".to_string()
                })?);
            }
            "--preflight-only" => preflight_only = true,
            "--help" | "-h" => return Err(usage().to_string()),
            _ => return Err(format!("unknown argument {flag:?}\n{}", usage())),
        }
    }

    let provider = LocalModelProvider::parse(provider.as_deref().unwrap_or("ollama"))?;
    let provider_base = match provider {
        LocalModelProvider::Ollama => {
            if huggingface_base.is_some() || llamacpp_base.is_some() {
                return Err(
                    "--huggingface-base and --llamacpp-base require their matching provider"
                        .to_string(),
                );
            }
            parse_loopback_chat_base(
                ollama_base.as_deref().unwrap_or(DEFAULT_OLLAMA_BASE),
                "--ollama-base",
                provider,
            )?
        }
        LocalModelProvider::HuggingFace => {
            if ollama_base.is_some() || llamacpp_base.is_some() {
                return Err(
                    "--ollama-base and --llamacpp-base require their matching provider".to_string(),
                );
            }
            let base = huggingface_base.ok_or_else(|| {
                "--huggingface-base is required when --provider huggingface is selected".to_string()
            })?;
            parse_loopback_chat_base(&base, "--huggingface-base", provider)?
        }
        LocalModelProvider::LlamaCpp => {
            if ollama_base.is_some() || huggingface_base.is_some() {
                return Err(
                    "--ollama-base and --huggingface-base require their matching provider"
                        .to_string(),
                );
            }
            let base = llamacpp_base.ok_or_else(|| {
                "--llamacpp-base is required when --provider llamacpp is selected".to_string()
            })?;
            parse_loopback_chat_base(&base, "--llamacpp-base", provider)?
        }
    };
    let model = model.ok_or_else(|| format!("--model is required\n{}", usage()))?;
    if model.trim().is_empty() {
        return Err("--model must not be empty".to_string());
    }
    Ok(Args {
        model,
        provider,
        provider_base,
        demo_url: validate_demo_url(
            &demo_url.ok_or_else(|| format!("--demo-url is required\n{}", usage()))?,
        )?,
        launcher_socket: launcher_socket
            .ok_or_else(|| format!("--launcher-socket is required\n{}", usage()))?,
        mcp_server: mcp_server.unwrap_or(default_mcp_server()?),
        transcript: transcript.ok_or_else(|| format!("--transcript is required\n{}", usage()))?,
        evidence_dir: evidence_dir
            .ok_or_else(|| format!("--evidence-dir is required\n{}", usage()))?,
        max_turns: max_turns.unwrap_or(MAX_TURNS_DEFAULT),
        highlight_hold_secs: highlight_hold_secs.unwrap_or(10),
        preflight_only,
    })
}
