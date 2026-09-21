// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Grammar-level rules of the module-related productions: contextual
//! keywords are terminal symbols (no Unicode escapes), and `import.source(...)`
//! / `import.defer(...)` are ImportCall forms of their own rather than member
//! calls on a global. Every rejection must be a *known* SyntaxError: the
//! Test262 adapter refuses to count an unclassified parse failure as a
//! passing negative test.

use blueice_bluejs::{parse, parse_module};

fn assert_module_syntax_error(source: &str) {
    let error = parse_module(source).expect_err(source);
    assert!(error.known_syntax, "{source}: {error:?}");
}

fn assert_script_syntax_error(source: &str) {
    let error = parse(source).expect_err(source);
    assert!(error.known_syntax, "{source}: {error:?}");
}

#[test]
fn contextual_keywords_in_import_and_export_declarations_reject_escapes() {
    for source in [
        "import {a \\u0061s b} from './m.js';",
        "import * \\u0061s ns from './m.js';",
        "import {} \\u0066rom './m.js';",
        "import a \\u0066rom './m.js';",
        "import * as ns \\u0066rom './m.js';",
        "export {a \\u0061s b}; var a;",
        "export {a \\u0061s b} from './m.js';",
        "export * \\u0061s ns from './m.js';",
        "export * \\u0066rom './m.js';",
        "export {} \\u0066rom './m.js';",
        "export d\\u0065fault 0;",
        "import './m.js' w\\u0069th { type: 'json' };",
        "export * from './m.js' w\\u0069th { type: 'json' };",
    ] {
        assert_module_syntax_error(source);
    }
}

#[test]
fn unescaped_contextual_keywords_still_parse_in_every_declaration_form() {
    for source in [
        "import {a as b} from './m.js';",
        "import * as ns from './m.js';",
        "import a from './m.js';",
        "import {} from './m.js';",
        "export {a as b}; var a;",
        "export * as ns from './m.js';",
        "export * from './m.js';",
        "export default 0;",
        "import './m.js' with { type: 'json' };",
        // `as`, `from` and `default` remain usable as ordinary names.
        "import {as as as} from './m.js'; import from from './m.js';",
        "import {\\u0061s as b} from './m.js';",
    ] {
        parse_module(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
    }
}

#[test]
fn import_keyword_and_import_meta_reject_escapes() {
    assert_script_syntax_error("im\\u0070ort('./m.js');");
    assert_module_syntax_error("im\\u0070ort('./m.js');");
    assert_module_syntax_error("im\\u0070ort.meta;");
    assert_module_syntax_error("import.m\\u0065ta;");
    assert_script_syntax_error("var x = im\\u0070ort;");
    parse("import('./m.js');").unwrap();
}

#[test]
fn import_defer_declarations_accept_only_a_namespace_import() {
    for source in [
        "import defer * as ns from './m.js';",
        "import defer * as ns from './m.js' with { type: 'json' };",
        "import defer * as ns from './m.js'\nns;",
        // `defer` stays an ordinary default-binding name everywhere else.
        "import defer from './m.js';",
        "import defer, * as ns from './m.js';",
        "import defer, { a } from './m.js';",
        "import { defer } from './m.js';",
        "import { a as defer } from './m.js';",
        "import * as defer from './m.js';",
        "import defer * as defer from './m.js';",
    ] {
        parse_module(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
    }
    for source in [
        "import defer d from './m.js';",
        "import defer { a } from './m.js';",
        "import defer d, * as ns from './m.js';",
        "import defer * as ns, { a } from './m.js';",
        "import defer as ns from './m.js';",
        "import defer './m.js';",
        "import defer * ns from './m.js';",
        "import defer * as ns;",
        "export defer * as ns from './m.js';",
        "import d\\u0065fer * as ns from './m.js';",
        // A deferred namespace import still binds an immutable, unique name.
        "import defer * as ns from './m.js'; import ns from './n.js';",
        "import defer * as eval from './m.js';",
    ] {
        assert_module_syntax_error(source);
    }
    // Import declarations, deferred or not, belong to modules only.
    assert_script_syntax_error("import defer * as ns from './m.js';");
    assert_module_syntax_error("if (true) { import defer * as ns from './m.js'; }");
}

#[test]
fn phase_import_calls_accept_the_same_argument_shapes_as_import() {
    for source in [
        "import.source('./m.js');",
        "import.defer('./m.js');",
        "import.source('./m.js',);",
        "import.defer('./m.js', { with: {} });",
        "import.defer('./m.js', { with: {} },);",
        "import.source(a in b);",
        "for (var i = import.defer(x in y); false;) ;",
        "async function f() { await import.source('./m.js'); return await import.defer('./m.js'); }",
        "import.source('./m.js').then(x => x); import.defer('./m.js').catch(x => x);",
    ] {
        parse(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
        parse_module(source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
    }
    // Bare `import.meta` inside a nested module statement is an expression.
    parse_module("{ import.meta; } if (true) import.meta;").unwrap();
    assert_script_syntax_error("import.meta;");
}

#[test]
fn phase_import_calls_reject_every_malformed_form() {
    for source in [
        // The specifier is required, and never a spread element.
        "import.source();",
        "import.defer();",
        "import.source(...['./m.js']);",
        "import.defer(...['./m.js']);",
        "import.defer('./m.js', ...[{}]);",
        "import('./m.js', {}, '');",
        "import.source('./m.js', {}, '');",
        "import.defer('./m.js', {}, '');",
        // An ImportCall is a CallExpression: never `new`'d, never a bare
        // property, never an assignment target.
        "new import('./m.js');",
        "new import.source('./m.js');",
        "new import.defer('./m.js');",
        "new import.source('./m.js').prop;",
        "new import.defer('./m.js').prop;",
        "import.source;",
        "import.defer;",
        "typeof import;",
        "typeof import.source;",
        "typeof import.defer;",
        "typeof import.source.UNKNOWN;",
        "import.UNKNOWN('./m.js');",
        "import.sourcex('./m.js');",
        "import.source('./m.js') = 1;",
        "import.defer('./m.js') = 1;",
        "import.defer('./m.js')++;",
        // The words after the dot are terminal symbols.
        "import.s\\u006furce('./m.js');",
        "import.d\\u0065fer('./m.js');",
    ] {
        assert_script_syntax_error(source);
        assert_module_syntax_error(source);
    }
    // `new import.meta` is a MetaProperty, so it remains a valid callee.
    parse_module("new import.meta();").unwrap();
}

#[test]
fn the_type_import_attribute_selects_the_module_type_of_every_request_form() {
    use blueice_bluejs::{ExportEntry, ModuleType};
    let module = parse_module(
        "import a from './a' with { type: 'text' };
         import b from './b' with { type: \"bytes\", };
         import c from './c' with { type: 'json' };
         import d from './d' with { type: 'css' };
         import e from './e' with { other: 'text' };
         import f from './f';
         import './g' with { type: 'text' };
         export { default as h } from './h' with { type: 'bytes' };
         export * from './i' with { type: 'text' };
         export * as j from './j' with { type: 'json' };",
    )
    .unwrap();
    let import_types: Vec<_> = module
        .imports
        .iter()
        .map(|import| (import.module_request.as_str(), import.module_type))
        .collect();
    assert_eq!(
        import_types,
        [
            ("./a", ModuleType::Text),
            ("./b", ModuleType::Bytes),
            ("./c", ModuleType::Json),
            // Unrecognized attribute values and keys leave a Source Text Module.
            ("./d", ModuleType::JavaScript),
            ("./e", ModuleType::JavaScript),
            ("./f", ModuleType::JavaScript),
            ("./g", ModuleType::Text),
        ]
    );
    let export_types: Vec<_> = module
        .exports
        .iter()
        .map(|export| match export {
            ExportEntry::Indirect {
                module_request,
                module_type,
                ..
            }
            | ExportEntry::Star {
                module_request,
                module_type,
            }
            | ExportEntry::Namespace {
                module_request,
                module_type,
                ..
            } => (module_request.as_str(), *module_type),
            ExportEntry::Local { .. } => unreachable!("only re-exports here"),
        })
        .collect();
    assert_eq!(
        export_types,
        [
            ("./h", ModuleType::Bytes),
            ("./i", ModuleType::Text),
            ("./j", ModuleType::Json),
        ]
    );
    // A request's identity is its specifier and type: the same specifier
    // requested as text and as JavaScript is two requested modules.
    let module = parse_module(
        "import './x' with { type: 'text' }; import './x'; import './x' with { type: 'text' };",
    )
    .unwrap();
    let requests: Vec<_> = module
        .requests
        .iter()
        .map(|request| (request.specifier.as_str(), request.module_type))
        .collect();
    assert_eq!(
        requests,
        [
            ("./x", ModuleType::Text),
            ("./x", ModuleType::JavaScript),
            ("./x", ModuleType::Text),
        ]
    );
}
