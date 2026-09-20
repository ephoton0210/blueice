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
