// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cumulative, original declaration profiles for the additional target years.

use crate::EcmaTarget;

pub(super) const VERSION: &str = "blue-ts-ecma-lib-v2";

pub(super) fn sources(target: EcmaTarget) -> Vec<(&'static str, &'static str)> {
    const SOURCES: &[(EcmaTarget, &str, &str)] = &[
        (
            EcmaTarget::Es5,
            "ecma-es5.v2.d.ts",
            include_str!("ecma-es5.v2.d.ts"),
        ),
        (
            EcmaTarget::Es2015,
            "ecma-es2015.v2.d.ts",
            include_str!("ecma-es2015.v2.d.ts"),
        ),
        (
            EcmaTarget::Es2016,
            "ecma-es2016.v2.d.ts",
            include_str!("ecma-es2016.v2.d.ts"),
        ),
        (
            EcmaTarget::Es2017,
            "ecma-es2017.v2.d.ts",
            include_str!("ecma-es2017.v2.d.ts"),
        ),
        (
            EcmaTarget::Es2018,
            "ecma-es2018.v2.d.ts",
            include_str!("ecma-es2018.v2.d.ts"),
        ),
        (
            EcmaTarget::Es2019,
            "ecma-es2019.v2.d.ts",
            include_str!("ecma-es2019.v2.d.ts"),
        ),
        (
            EcmaTarget::Es2020,
            "ecma-es2020.v2.d.ts",
            include_str!("ecma-es2020.v2.d.ts"),
        ),
        (
            EcmaTarget::Es2021,
            "ecma-es2021.v2.d.ts",
            include_str!("ecma-es2021.v2.d.ts"),
        ),
        (
            EcmaTarget::Es2022,
            "ecma-es2022.v2.d.ts",
            include_str!("ecma-es2022.v2.d.ts"),
        ),
        (
            EcmaTarget::Es2023,
            "ecma-es2023.v2.d.ts",
            include_str!("ecma-es2023.v2.d.ts"),
        ),
    ];
    let mut sources: Vec<_> = SOURCES
        .iter()
        .filter(|(edition, _, _)| *edition <= target)
        .map(|(_, name, text)| (*name, *text))
        .collect();
    if target >= EcmaTarget::Es2018 {
        sources.push(super::ASYNC_ITERATION);
    }
    sources
}
