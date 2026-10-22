// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original G-T7 declaration layers, selected by library edition, not emit target.
use crate::EcmaTarget;

pub(super) fn sources(target: EcmaTarget) -> Vec<(&'static str, &'static str)> {
    const SOURCES: &[(EcmaTarget, &str, &str)] = &[
        (
            EcmaTarget::Es5,
            "ecma-core.v3.d.ts",
            include_str!("ecma-core.v3.d.ts"),
        ),
        (
            EcmaTarget::Es5,
            "ecma-buffers.v3.d.ts",
            include_str!("ecma-buffers.v3.d.ts"),
        ),
        (
            EcmaTarget::Es2015,
            "ecma-iterable.v3.d.ts",
            include_str!("ecma-iterable.v3.d.ts"),
        ),
        (
            EcmaTarget::Es2015,
            "ecma-promise.v3.d.ts",
            include_str!("ecma-promise.v3.d.ts"),
        ),
        (
            EcmaTarget::Es2017,
            "ecma-es2017.v3.d.ts",
            include_str!("ecma-es2017.v3.d.ts"),
        ),
        (
            EcmaTarget::Es2018,
            "ecma-es2018.v3.d.ts",
            include_str!("ecma-es2018.v3.d.ts"),
        ),
        (
            EcmaTarget::Es2019,
            "ecma-es2019.v3.d.ts",
            include_str!("ecma-es2019.v3.d.ts"),
        ),
        (
            EcmaTarget::Es2020,
            "ecma-es2020.v3.d.ts",
            include_str!("ecma-es2020.v3.d.ts"),
        ),
        (
            EcmaTarget::Es2021,
            "ecma-es2021.v3.d.ts",
            include_str!("ecma-es2021.v3.d.ts"),
        ),
        (
            EcmaTarget::Es2022,
            "ecma-es2022.v3.d.ts",
            include_str!("ecma-es2022.v3.d.ts"),
        ),
    ];
    SOURCES
        .iter()
        .filter(|(edition, _, _)| *edition <= target)
        .map(|(_, name, source)| (*name, *source))
        .collect()
}
