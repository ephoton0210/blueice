// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleSource};

const DECLARATIONS: &str = "interface Chosen { readonly kind: string; } interface Items { add(data: string, format: string): Chosen; add(file: Chosen): Chosen; } interface Transfer { readonly items: Items; setData(format: string, data: string): void; } declare const Transfer: { new(): Transfer; readonly prototype: Transfer; }; declare const Illegal: { readonly prototype: Transfer; }; declare const Chosen: { new(name: string): Chosen; readonly prototype: Chosen; };";

fn check(source: &str) -> bool {
    !compile(
        "memory:///construct.ts",
        &MapLoader::from([ModuleSource::new("memory:///construct.ts", source)]),
        CompilerOptions {
            require_declared_global_calls: true,
            ambient_declaration_modules: vec![ModuleSource::new(
                "memory:///host.d.ts",
                DECLARATIONS,
            )],
            ..CompilerOptions::default()
        },
    )
    .has_errors()
}

#[test]
fn host_construct_signatures_infer_instances_and_select_plain_member_overloads() {
    for source in [
        "const transfer = new Transfer(); transfer.setData('text', 'owned');",
        "new Transfer().setData('text', 'owned');",
        "const item: Chosen = new Transfer().items.add('owned', 'text');",
        "const item: Chosen = new Transfer().items.add(new Chosen('name'));",
        "const transfer: Transfer = new Transfer(); const item: Chosen = transfer.items.add('owned', 'text');",
    ] {
        assert!(check(source), "{source}");
    }
}

#[test]
fn host_constructors_do_not_erase_argument_checks_readonly_members_or_brands() {
    for source in [
        "new Illegal();",
        "new Transfer(1);",
        "new Chosen();",
        "new Chosen(1);",
        "new Missing();",
        "new Transfer().setData('text', 1);",
        "new Transfer().items.add('text');",
        "new Transfer().items.add(1);",
        "new Transfer().items = {};",
        "const transfer = new Transfer(); transfer.setData('text', 1);",
    ] {
        assert!(!check(source), "{source}");
    }
}
