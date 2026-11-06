// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Every admitted super-write product follows Native tokens, gaps and execution.
#[test]
#[ignore = "requires pinned TypeScript 5.9.3; all 43 frozen super write products"]
fn retained_super_write_all_frozen_products_match_native_roles() {
    super::class_cli::run(
        |row| row["form"] == "ge1_downlevel/super-write-receiver-order",
        43,
        "class-super-write",
        "verify_ge5_class_retained.cjs",
    );
}
