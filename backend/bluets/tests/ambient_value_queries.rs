// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleSource, RuntimePolicy};

fn verdicts(declarations: &[(&str, &str)], cases: &[(&str, bool)]) {
    let mut failures = Vec::new();
    for (source, accepted) in cases {
        let result = compile(
            "memory:///consumer.ts",
            &MapLoader::from([ModuleSource::new("memory:///consumer.ts", *source)]),
            CompilerOptions {
                runtime_policy: RuntimePolicy::Checked,
                require_declared_global_calls: true,
                ambient_declaration_modules: declarations
                    .iter()
                    .map(|(name, text)| ModuleSource::new(*name, *text))
                    .collect(),
                ..CompilerOptions::default()
            },
        );
        if result.has_errors() == *accepted || result.output.is_some() != *accepted {
            failures.push((source, accepted, result.diagnostics));
        }
    }
    assert!(
        failures.is_empty(),
        "unexpected typing verdicts: {failures:#?}"
    );
}

#[test]
fn ambient_function_queries_across_declaration_modules_keep_calls_and_results() {
    let host = "interface Host { schedule: typeof schedule; } declare const host: Host;";
    let function = "declare function schedule(callback: () => void, delay?: number): number;";
    for declarations in [
        [("memory:///a.d.ts", host), ("memory:///z.d.ts", function)],
        [("memory:///z.d.ts", host), ("memory:///a.d.ts", function)],
    ] {
        verdicts(&declarations, &[
            ("const id: number = host.schedule(function () {}, 1);", true),
            ("host.schedule(7);", false),
            ("host.schedule(function () {}, 'soon');", false),
            ("const id: string = host.schedule(function () {});", false),
            ("const call: typeof schedule = schedule; const id: number = call(function () {});", true),
        ]);
    }
}

#[test]
fn ambient_overload_queries_keep_every_call_signature() {
    verdicts(&[("memory:///host.d.ts", "declare function convert(value: string): string; declare function convert(value: number): number; interface Host { convert: typeof convert; } declare const host: Host;")], &[
        ("const text: string = host.convert('text'); const count: number = host.convert(1);", true),
        ("host.convert(true);", false),
        ("const count: number = host.convert('text');", false),
        ("const text: string = host.convert(1);", false),
    ]);
}

#[test]
fn ambient_generic_function_queries_keep_parameter_and_result_relationships() {
    verdicts(&[("memory:///host.d.ts", "declare function identity<T>(value: T): T; interface Host { identity: typeof identity; } declare const host: Host;")], &[
        ("const count: number = host.identity(1); const text: string = host.identity('text');", true),
        ("const text: string = host.identity(1);", false),
        ("host.identity();", false),
    ]);
}

#[test]
fn ambient_queries_respect_local_value_and_parameter_shadowing() {
    verdicts(&[("memory:///host.d.ts", "declare function schedule(value: number): number;")], &[
        ("function use(schedule: string): typeof schedule { return schedule; }", true),
        ("function use(schedule: string): typeof schedule { return 7; }", false),
        ("function use(schedule: (value: string) => string): string { const call: typeof schedule = schedule; return call('text'); }", true),
        ("function use(schedule: (value: string) => string): string { const call: typeof schedule = schedule; return call(7); }", false),
        ("function use(): string { const schedule: string = 'local'; const value: typeof schedule = schedule; return value; }", true),
    ]);
}

#[test]
fn ambient_queries_bind_at_the_declaration_instead_of_consumer_shadowing() {
    verdicts(&[("memory:///host.d.ts", "declare function schedule(value: number): number; interface Host { schedule: typeof schedule; } declare const host: Host;")], &[
        ("function use(schedule: string): number { return host.schedule(7); }", true),
        ("function use(schedule: string): number { return host.schedule('wrong'); }", false),
    ]);
}

#[test]
fn declaration_modules_keep_identical_local_query_names_and_offsets_distinct() {
    verdicts(&[
        ("memory:///a.d.ts", "export {}; declare const value: number; declare global { interface HostA { item: typeof value; } const hostA: HostA; }"),
        ("memory:///b.d.ts", "export {}; declare const value: string; declare global { interface HostB { item: typeof value; } const hostB: HostB; }"),
    ], &[
        ("const count: number = hostA.item; const text: string = hostB.item;", true),
        ("const text: string = hostA.item;", false),
        ("const count: number = hostB.item;", false),
        ("const privateValue: number = value;", false),
    ]);
}

#[test]
fn ambient_qualified_queries_retain_the_selected_callable() {
    verdicts(&[("memory:///host.d.ts", "declare const service: { apply: (value: string) => number; }; interface Host { apply: typeof service.apply; } declare const host: Host;")], &[
        ("const count: number = host.apply('text');", true),
        ("host.apply(7);", false),
        ("const text: string = host.apply('text');", false),
    ]);
}

#[test]
fn unresolved_ambient_queries_prevent_checked_output() {
    verdicts(
        &[(
            "memory:///host.d.ts",
            "interface Host { apply: typeof missing; } declare const host: Host;",
        )],
        &[("host.apply('text');", false)],
    );
}

#[test]
fn ambient_query_resolution_obeys_the_type_expansion_budget() {
    let result = compile("memory:///consumer.ts", &MapLoader::from([ModuleSource::new("memory:///consumer.ts", "const value: number = host.first;")]), CompilerOptions {
        ambient_declaration_modules: vec![ModuleSource::new("memory:///host.d.ts", "declare const first: number; declare const second: number; interface Host { first: typeof first; second: typeof second; } declare const host: Host;")],
        limits: blueice_bluets::CompilerLimits { max_type_expansions: 1, ..Default::default() },
        ..Default::default()
    });
    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == blueice_bluets::DiagnosticCode::ResourceLimit),
        "{:?}",
        result.diagnostics
    );
}
