// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn rejects_assignments_to_readonly_event_fields() {
    let ambient = ModuleSource::new(
        "memory:///events.d.ts",
        "interface Node { id: string; }\n\
         interface Event { readonly type: 'click'; readonly target: Node; readonly currentTarget: Node; mutable: string; }",
    );
    let check = |source| {
        crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions {
                ambient_declaration_modules: vec![ambient.clone()],
                ..CompilerOptions::default()
            },
        )
    };
    let valid = check(
        "function onClick(event: Event): void { event.mutable = 'ok'; event['mutable'] = 'ok'; }",
    );
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
    let ordinary_computed = check(
        "interface Mutable { value: string; } function write(value: Mutable, key: string): void { value[key] = 'ok'; }",
    );
    assert!(
        !ordinary_computed.has_errors(),
        "{:#?}",
        ordinary_computed.diagnostics
    );
    for source in [
        "function onClick(event: Event): void { event.type = 'click'; }",
        "function onClick(event: Event): void { event.type += 'click'; }",
        "function onClick(event: Event): void { event.type++; }",
        "function onClick(event: Event): void { ++event.type; }",
        "function onClick(event: Event): void { delete event.type; }",
        "function onClick(event: Event): void { event.target = event.target; }",
        "function onClick(event: Event): void { event.currentTarget = event.target; }",
        "function onClick(event: Event): void { event['type'] = 'click'; }",
        "function onClick(event: Event): void { event[\"target\"] = event.target; }",
        "function onClick(event: Event): void { event['currentTarget'] += event.target; }",
        "function onClick(event: Event): void { event['type']++; }",
        "function onClick(event: Event): void { ++event['type']; }",
        "function onClick(event: Event): void { delete event['type']; }",
        "function onClick(event: Event, key: string): void { event[key] = 'click'; }",
        "function onClick(event: Event, keys: string[]): void { event[keys[0]] = 'click'; }",
        "function onClick(event: Event): void { event['typ\\u0065'] = 'click'; }",
    ] {
        let invalid = check(source);
        assert!(
            invalid.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::TypeMismatch
                    && diagnostic.message.contains("readonly")
            }),
            "{source}: {:#?}",
            invalid.diagnostics
        );
    }
}

#[test]
fn readonly_survives_inherited_and_generic_property_lookup() {
    let source = "interface Base { readonly value: string; }\n\
                  interface Derived extends Base { mutable: string; }\n\
                  interface Box<T> { readonly item: T; }\n\
                  type Detail = { readonly currentTarget: string; mutable: string };\n\
                  function update(derived: Derived, box: Box<string>, detail: Detail): void {\n\
                    derived.value = 'x'; box.item = 'x'; detail.currentTarget = 'x';\n\
                    derived.mutable = 'x'; detail.mutable = 'x';\n\
                  }";
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions::default(),
    );
    let readonly = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.message.contains("readonly property"))
        .collect::<Vec<_>>();
    assert_eq!(readonly.len(), 3, "{:#?}", result.diagnostics);
    assert_eq!(result.diagnostics.len(), 3, "{:#?}", result.diagnostics);
}

#[test]
fn computed_write_fails_closed_after_generic_inheritance_expansion() {
    let source = "interface Base<T> { readonly item: T; }\n\
                  interface Derived extends Base<string> { mutable: string; }\n\
                  function write(value: Derived, key: string): void { value[key] = 'x'; }";
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions::default(),
    );
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::TypeMismatch
                && diagnostic.message.contains("readonly")
        }),
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn readonly_checks_follow_parenthesized_chained_and_called_receivers() {
    let ambient = ModuleSource::new(
        "memory:///events.d.ts",
        "interface Event { readonly type: 'click'; readonly target: string; mutable: string; }\n\
         interface Holder { event: Event; }",
    );
    let check = |source| {
        crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions {
                ambient_declaration_modules: vec![ambient.clone()],
                ..CompilerOptions::default()
            },
        )
    };
    let valid = check(
        "function write(holder: Holder, event: Event): void {\n\
           (event).mutable = 'ok'; holder.event.mutable = 'ok'; holder['event'].mutable = 'ok';\n\
         }\n\
         interface Counter { value: number; }\n\
         function read(counter: Counter): number { return 1 + counter.value; }\n\
         function kind(event: Event): string { return typeof event.type; }",
    );
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
    for source in [
        "function write(event: Event): void { (event).type = 'click'; }",
        "function write(holder: Holder): void { holder.event.type = 'click'; }",
        "function write(holder: Holder): void { holder['event'].type = 'click'; }",
        "function write(holder: Holder): void { holder.event['target'] = 'wrong'; }",
        "function write(holder: Holder, key: string): void { holder.event[key] = 'wrong'; }",
        "function getEvent(event: Event): Event { return event; }\n\
         function write(event: Event): void { getEvent(event).type = 'click'; }",
    ] {
        let invalid = check(source);
        assert!(
            invalid.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::TypeMismatch
                    && diagnostic.message.contains("readonly")
            }),
            "{source}: {:#?}",
            invalid.diagnostics
        );
    }
}

#[test]
fn readonly_checks_follow_dynamic_array_receivers() {
    let ambient = ModuleSource::new(
        "memory:///events.d.ts",
        "interface Event { readonly type: 'click'; readonly target: string; readonly currentTarget: string; mutable: string; }",
    );
    let check = |source| {
        crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions {
                ambient_declaration_modules: vec![ambient.clone()],
                ..CompilerOptions::default()
            },
        )
    };
    for source in [
        "function write(events: Event[], index: number): void { events[index].type = 'click'; }",
        "function write(events: Event[], index: number): void { events[index].target = 'wrong'; }",
        "function write(events: Event[], index: number): void { events[index].currentTarget++; }",
        "function write(events: Event[]): void { events['0'].type = 'click'; }",
        "function write(values: [Event, number]): void { values[0].type = 'click'; }",
        "function write(events: Event[], index: number): void { consume(events[index].type = 'click'); } function consume(value: string): void {}",
        "type Events = Event[]; function write(events: Events, index: number): void { events[index].type = 'click'; }",
        "interface Slots { first: Event; second: Event; } function write(slots: Slots, key: string): void { slots[key].target = 'wrong'; }",
    ] {
        let invalid = check(source);
        assert!(
            invalid.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::TypeMismatch
                    && diagnostic.message.contains("readonly")
            }),
            "{source}: {:#?}",
            invalid.diagnostics
        );
    }
    let valid = check(
        "interface Slots { first: Event; second: Event; } function write(events: Event[], index: number, slots: Slots, key: string): void { events[index].mutable = 'ok'; slots[key].mutable = 'ok'; }",
    );
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
}

#[test]
fn readonly_checks_follow_heterogeneous_dynamic_receivers() {
    let source = "interface Event { readonly type: 'click'; mutable: string; }\n\
                  interface Holder { event: Event; }\n\
                  interface Other { count: number; }\n\
                  interface Slots { first: Holder; second: Other; }\n\
                  function consume(value: string): void {}\n\
                  function write(slots: Slots, key: string): void {\n\
                    slots[key].event.type = 'click';\n\
                    consume(slots[key].event.type = 'click');\n\
                  }";
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions::default(),
    );
    let readonly = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.message.contains("readonly"))
        .count();
    assert_eq!(readonly, 2, "{:#?}", result.diagnostics);

    for source in [
        "interface Event { readonly type: 'click'; } interface Other { count: number; } function write(values: Event[] | Other[], index: number): void { values[index].type = 'click'; }",
        "interface Event { readonly type: 'click'; } interface Other { count: number; } type Possible = Event | Other; function write(values: Possible[], index: number): void { values[index].type = 'click'; }",
        "interface Event { readonly type: 'click'; } interface Other { count: number; } function write(values: [Event, Other], index: number): void { values[index].type = 'click'; }",
    ] {
        let invalid = crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions::default(),
        );
        assert!(
            invalid
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("readonly")),
            "{source}: {:#?}",
            invalid.diagnostics
        );
    }

    let valid = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "interface First { mutable: string; } interface Second { mutable: string; extra: number; } interface Slots { first: First; second: Second; } function write(slots: Slots, key: string): void { slots[key].mutable = 'ok'; }",
        )]),
        CompilerOptions::default(),
    );
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);

    let bounded = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions {
            limits: crate::compiler::CompilerLimits {
                max_type_expansions: 4,
                ..crate::compiler::CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(
        bounded.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::ResourceLimit
                && diagnostic.message.contains("computed readonly receiver")
        }),
        "{:#?}",
        bounded.diagnostics
    );
}

#[test]
fn readonly_survives_an_inferred_heterogeneous_index_alias() {
    let source = "interface Event { readonly type: 'click'; mutable: string; }\n\
                  interface Other { type: 'click'; mutable: string; }\n\
                  interface Slots { first: Event; second: Other; }\n\
                  function write(slots: Slots, key: string): void {\n\
                    const selected = slots[key];\n\
                    selected.type = 'click';\n\
                  }";
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions::default(),
    );
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::TypeMismatch
                && diagnostic.message.contains("readonly")
        }),
        "{:#?}",
        result.diagnostics
    );
    let valid = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "interface First { mutable: string; readonly type: 'click'; } interface Second { mutable: string; type: 'click'; } interface Slots { first: First; second: Second; } function write(slots: Slots, key: string): void { const selected = slots[key]; selected.mutable = 'ok'; }",
        )]),
        CompilerOptions::default(),
    );
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
}

#[test]
fn readonly_survives_member_values_inside_array_and_record_literals() {
    let source = "interface Event { readonly type: 'click'; }\n\
                  interface Holder { event: Event; type: string; }\n\
                  function identity(event: Event, ignored: number): Event { return event; }\n\
                  function write(holder: Holder): void {\n\
                    const fromArray = [holder.event][0];\n\
                    fromArray.type = 'click';\n\
                    const fromRecord = { picked: holder.event };\n\
                    fromRecord.picked.type = 'click';\n\
                    const fromCall = { picked: identity(holder.event, 1) };\n\
                    fromCall.picked.type = 'click';\n\
                    const nested = { outer: { picked: holder.event } };\n\
                    nested.outer.picked.type = 'click';\n\
                  }";
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions::default(),
    );
    let readonly = result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.message.contains("readonly"))
        .count();
    assert_eq!(readonly, 4, "{:#?}", result.diagnostics);

    let valid = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "interface Holder { type: string; } function write(holder: Holder): void { const fromArray = [holder][0]; fromArray.type = 'ok'; const fromRecord = { picked: holder }; fromRecord.picked.type = 'ok'; }",
        )]),
        CompilerOptions::default(),
    );
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
}

#[test]
fn nested_literal_inference_fails_with_a_bounded_resource_diagnostic() {
    let source = format!("const value = {}1{};", "[".repeat(129), "]".repeat(129));
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", &source)]),
        CompilerOptions::default(),
    );
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::ResourceLimit
                && diagnostic.message.contains("container inference limit")
        }),
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn readonly_mutations_inside_larger_expressions_are_not_skipped() {
    let ambient = ModuleSource::new(
        "memory:///events.d.ts",
        "interface Event { readonly type: 'click'; mutable: string; }",
    );
    let check = |source| {
        crate::compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions {
                ambient_declaration_modules: vec![ambient.clone()],
                ..CompilerOptions::default()
            },
        )
    };
    let valid = check(
        "function write(event: Event): void {\n\
           let other = '';\n\
           other = event.mutable = 'ok'; event.mutable = other = 'ok';\n\
         }\n\
         interface Counter { value: number; }\n\
         function consume(value: number): void {}\n\
         function update(counter: Counter): void { consume(++counter.value); }\n\
         interface Helper { delete(value: string): void; }\n\
         function read(helper: Helper, event: Event): void { helper.delete(event.type); }",
    );
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
    for source in [
        "function consume(value: string): void {} function write(event: Event): void { consume(event.type = 'click'); }",
        "function write(event: Event): void { let other = ''; other = event.type = 'click'; }",
        "function write(event: Event): void { let other = ''; event.type = other = 'click'; }",
        "function write(event: Event): void { let changed = true && (event.type = 'click'); }",
        "function write(event: Event): void { let changed = event.type++ + 1; }",
        "function consume(value: number): void {} function write(event: Event): void { consume(++event.type); }",
        "function write(event: Event): void { let gone = !!(delete event.type); }",
        "function write(event: Event, key: string): void { let changed = (event[key] = 'click'); }",
    ] {
        let invalid = check(source);
        assert!(
            invalid.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::TypeMismatch
                    && diagnostic.message.contains("readonly")
            }),
            "{source}: {:#?}",
            invalid.diagnostics
        );
    }
    let source = "function consume(value: string): void {} function write(event: Event): void { consume(event.type = 'click'); }";
    let invalid = check(source);
    let readonly = invalid
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.contains("readonly property"))
        .expect("nested readonly mutation has a diagnostic");
    assert_eq!(readonly.span.start, source.find("event.type =").unwrap());
    assert_eq!(
        readonly.span.end,
        source.find("event.type =").unwrap() + "event.type =".len()
    );
}

#[test]
fn nested_readonly_scan_does_not_spend_generic_budget_on_plain_assignment() {
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.ts",
            "let value: number = 0; value = 1;",
        )]),
        CompilerOptions {
            limits: crate::compiler::CompilerLimits {
                max_type_expansions: 0,
                ..crate::compiler::CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
}

#[test]
fn nested_readonly_scan_bounds_a_long_assignment_chain() {
    let source = format!("let value: number = 0; {}1;", "value = ".repeat(257));
    let result = crate::compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", &source)]),
        CompilerOptions {
            limits: crate::compiler::CompilerLimits {
                max_type_expansions: 0,
                ..crate::compiler::CompilerLimits::default()
            },
            ..CompilerOptions::default()
        },
    );
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::ResourceLimit
                && diagnostic.message.contains("member mutation scan")
        }),
        "{:#?}",
        result.diagnostics
    );
}
