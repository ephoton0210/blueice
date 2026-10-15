// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

struct ProfileHost {
    host: Option<BlueJsChildHost>,
    owner: Option<std::thread::JoinHandle<()>>,
    socket: PathBuf,
}

impl ProfileHost {
    fn new() -> Self {
        static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
        let socket = std::env::temp_dir().join(format!(
            "bi-source-profile-{}-{}.sock",
            std::process::id(),
            NEXT_OWNER.fetch_add(1, Ordering::Relaxed)
        ));
        let listener = UnixListener::bind(&socket).unwrap();
        let token = "d".repeat(script::SCRIPT_SESSION_TOKEN_HEX_BYTES);
        let expected_token = token.clone();
        let owner = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            assert_eq!(
                script::read_script_request(&mut stream).unwrap(),
                ScriptRequest::Hello {
                    protocol_version: script::SCRIPT_PROTOCOL_VERSION,
                    session_token: expected_token
                }
            );
            script::write_script_reply(
                &mut stream,
                &ScriptReply::HelloAck {
                    protocol_version: script::SCRIPT_PROTOCOL_VERSION,
                },
            )
            .unwrap();
            loop {
                let request = match script::read_script_request(&mut stream) {
                    Ok(request) => request,
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset
                        ) =>
                    {
                        break
                    }
                    Err(error) => panic!("source fixture owner: {error}"),
                };
                let ScriptRequest::Call {
                    request_id,
                    request,
                } = request
                else {
                    panic!("document call")
                };
                let target = request.document_target().unwrap();
                assert!(
                    matches!(*request, ScriptRequest::GetDocument { .. }),
                    "{request:?}"
                );
                script::write_script_reply(
                    &mut stream,
                    &ScriptReply::CallResult {
                        request_id,
                        target,
                        reply: Box::new(ScriptReply::NodeCreated {
                            node: target.document_generation + 1,
                        }),
                    },
                )
                .unwrap();
            }
        });
        let mut host = BlueJsChildHost::default();
        host.configure_script_file_dom_capability(socket.clone(), token)
            .unwrap();
        Self {
            host: Some(host),
            owner: Some(owner),
            socket,
        }
    }

    fn host(&mut self) -> &mut BlueJsChildHost {
        self.host.as_mut().unwrap()
    }
}

impl Drop for ProfileHost {
    fn drop(&mut self) {
        drop(self.host.take());
        let result = self.owner.take().unwrap().join();
        let _ = fs::remove_file(&self.socket);
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}

const SYSTEM_LOADER: &str = "var System={register:function(dependencies,declare){var body=declare(function(name,value){return value;},{id:'source'});body.execute();}};";

#[derive(Debug, Clone, Copy)]
struct ProfileRoute {
    language: PageHostScriptLanguage,
    kind: PageHostScriptKind,
    profile: PageHostBlueTsEmissionProfile,
    awaited: bool,
    classic_console: bool,
}

fn profile_routes() -> Vec<ProfileRoute> {
    use PageHostBlueTsEmissionProfile::*;
    let mut routes = Vec::new();
    for language in [
        PageHostScriptLanguage::JavaScript,
        PageHostScriptLanguage::BlueTs,
    ] {
        routes.push(ProfileRoute {
            language,
            kind: PageHostScriptKind::Classic,
            profile: Standard,
            awaited: false,
            classic_console: false,
        });
        for awaited in [false, true] {
            routes.push(ProfileRoute {
                language,
                kind: PageHostScriptKind::Module,
                profile: Standard,
                awaited,
                classic_console: false,
            });
        }
    }
    for profile in [
        Standard,
        Es5,
        LegacyDecorators,
        LegacyDecoratorsWithMetadata,
    ] {
        for classic_console in [false, true] {
            routes.push(ProfileRoute {
                language: PageHostScriptLanguage::BlueTsEmitted,
                kind: PageHostScriptKind::Classic,
                profile,
                awaited: false,
                classic_console,
            });
        }
    }
    for awaited in [false, true] {
        routes.push(ProfileRoute {
            language: PageHostScriptLanguage::BlueTsEmitted,
            kind: PageHostScriptKind::Classic,
            profile: System,
            awaited,
            classic_console: false,
        });
        for profile in [Standard, ModulePageConsole] {
            routes.push(ProfileRoute {
                language: PageHostScriptLanguage::BlueTsEmitted,
                kind: PageHostScriptKind::Module,
                profile,
                awaited,
                classic_console: false,
            });
        }
    }
    routes
}

fn run_profile_document(
    selected: PageHostDocument,
) -> (PageHostReply, blueice_ipc::console::ConsoleMessages) {
    let deferred = selected.debugger_execution_control;
    let mut fixture = ProfileHost::new();
    let host = fixture.host();
    let synchronized =
        host.handle_request(PageHostRequest::SynchronizeDocument { document: selected });
    let reply = if deferred
        && matches!(synchronized,PageHostReply::Synchronized {ref reports,..} if reports.is_empty())
    {
        host.handle_request(PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 1,
        })
    } else {
        synchronized
    };
    let flush = host.handle_request(PageHostRequest::FlushDragTasks {
        tab_id: 7,
        document_generation: 1,
    });
    assert!(
        matches!(flush, PageHostReply::DragTasksFlushed { .. }),
        "{flush:?}"
    );
    let PageHostReply::ConsoleMessages { messages, .. } =
        host.handle_request(PageHostRequest::DrainConsole {
            tab_id: 7,
            document_generation: 1,
        })
    else {
        panic!("copied console")
    };
    (reply, messages)
}

#[test]
fn admitted_source_profiles_keep_authored_throw_report_and_rejection_origins() {
    let mut failures = Vec::new();
    for route in profile_routes() {
        let ProfileRoute {
            language,
            kind,
            profile,
            awaited,
            classic_console,
        } = route;
        for deferred in [false, true] {
            for (name, setup_js, setup_ts, operation, known) in [
                ("throw", "", "", "throw 'profile';", true),
                ("report", "", "", "reportError('profile');", true),
                ("rejection", "var Factory=ProfilePromise;", "const Factory: any=ProfilePromise;", "Factory.reject('profile');", true),
                ("unknown", "var Factory=ProfileFunction;var execute=Factory(\"throw 'unknown';\");", "const Factory: any=ProfileFunction;const execute: any=Factory(\"throw 'unknown';\");", "execute();", false),
            ] {
                let typed=language!=PageHostScriptLanguage::JavaScript;
                let setup=if typed {setup_ts} else {setup_js};
                let setup=if language==PageHostScriptLanguage::BlueTs {setup.replace("ProfilePromise","Promise").replace("ProfileFunction","Function")} else {setup.to_string()};
                let prefix=format!("{}{}",if kind==PageHostScriptKind::Module || profile==PageHostBlueTsEmissionProfile::System {"export const marker=0;"} else {""},if awaited {"await 0;"} else {""});
                let declarations=if language==PageHostScriptLanguage::BlueTsEmitted {"declare const ProfilePromise: any;declare const ProfileFunction: any;"} else {""};
                let prelude=if typed {"  const face: string='😀';  "} else {"  var face='😀';  "};
                let source=if typed {format!("{prefix}{declarations}function fail(): void {{\n{prelude}{setup}{operation}\n}}fail();")} else {format!("{prefix}function fail(){{\n{prelude}{setup}{operation}\n}}fail();")};
                let id="blueice://page/profile.ts";
                let loader=format!("var ProfilePromise=Promise;var ProfileFunction=Function;{}",if profile==PageHostBlueTsEmissionProfile::System {SYSTEM_LOADER} else {""});
                let mut scripts=vec![classic(0,&loader)];
                scripts.push(PageHostScript {ordinal:1,language,kind,graph:graph(id,vec![PageHostSource::new(id,&source)])});
                scripts.push(classic(2,"try{throw 'later';}catch(error){}"));
                let mut selected=document(1,scripts);
                selected.debugger_execution_control=deferred;
                selected.emitted_bluets_profile=profile;
                selected.emitted_classic_page_console_ambient=classic_console;
                selected.emitted_module_page_console_ambient=profile==PageHostBlueTsEmissionProfile::ModulePageConsole;
                let (reply,messages)=run_profile_document(selected);
                let actual=messages.records.first().and_then(|r|r.source.as_ref()).map(|p|(p.filename.as_str(),p.line,p.column));
                let column=(prelude.encode_utf16().count()+setup.encode_utf16().count()+1) as u32;
                let expected=known.then_some((id,2,column));
                if messages.records.len()!=1 || actual!=expected || !messages.records[0].message.contains(if known {"profile"} else {"unknown"}) {
                    failures.push(format!("{route:?}/{deferred}/{name}: expected {expected:?}; {messages:?}; {reply:?}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn generated_profile_helpers_keep_gaps_and_preserve_authored_callback_sources() {
    use PageHostBlueTsEmissionProfile::*;
    let mut failures = Vec::new();
    for (profile, kind) in [
        (Standard, PageHostScriptKind::Classic),
        (Standard, PageHostScriptKind::Module),
        (ModulePageConsole, PageHostScriptKind::Module),
        (Es5, PageHostScriptKind::Classic),
        (LegacyDecorators, PageHostScriptKind::Classic),
        (LegacyDecoratorsWithMetadata, PageHostScriptKind::Classic),
        (System, PageHostScriptKind::Classic),
    ] {
        for deferred in [false, true] {
            for known in [false, true] {
                let prelude = "  const face: string='😀';  ";
                let operation = if known {
                    "throw 'authored';"
                } else if matches!(profile, LegacyDecorators | LegacyDecoratorsWithMetadata) {
                    "return 1;"
                } else {
                    "return 0;"
                };
                let prefix = if kind == PageHostScriptKind::Module || profile == System {
                    "export const marker: number=0;"
                } else {
                    ""
                };
                let body=match profile {
                    System if !known => "const value: number=1;".to_string(),
                    System => format!("class Handler {{fail(): void {{\n{prelude}{operation}\n}}}}new Handler().fail();"),
                    Es5 => format!("declare const ProfileProxy: any;class Handler {{ownKeys(): any {{\n{prelude}{operation}\n}}}}const input: {{value: number}}=new ProfileProxy({{value:1}},new Handler());const result: any={{...input}};"),
                    LegacyDecorators | LegacyDecoratorsWithMetadata => format!("function bad(target: any,key: string,descriptor: any): any {{\n{prelude}{operation}\n}}class Decorated {{@bad method(value: number): number {{return value;}}}}"),
                    _ => format!("function bad(value: any,context: any): any {{\n{prelude}{operation}\n}}@bad class Decorated {{}}"),
                };
                let source = format!("{prefix}{body}");
                let system_loader = if profile == System {
                    if known {
                        SYSTEM_LOADER
                    } else {
                        "var System={register:0};"
                    }
                } else {
                    ""
                };
                let id = "blueice://page/generated-profile.ts";
                let script = PageHostScript {
                    ordinal: 1,
                    language: PageHostScriptLanguage::BlueTsEmitted,
                    kind,
                    graph: graph(id, vec![PageHostSource::new(id, &source)]),
                };
                let mut selected = document(
                    1,
                    vec![
                        classic(0, &format!("var ProfileProxy=Proxy;{system_loader}")),
                        script,
                        classic(2, "try{throw 'later';}catch(error){}"),
                    ],
                );
                selected.debugger_execution_control = deferred;
                selected.emitted_bluets_profile = profile;
                selected.emitted_module_page_console_ambient = profile == ModulePageConsole;
                let (reply, messages) = run_profile_document(selected);
                let actual = messages
                    .records
                    .first()
                    .and_then(|r| r.source.as_ref())
                    .map(|p| (p.filename.as_str(), p.line, p.column));
                let expected =
                    known.then_some((id, 2, (prelude.encode_utf16().count() + 1) as u32));
                if messages.records.len() != 1
                    || actual != expected
                    || known && !messages.records[0].message.contains("authored")
                {
                    failures.push(format!("{profile:?}/{kind:?}/{deferred}/{known}: expected {expected:?}; {messages:?}; {reply:?}; {source}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
