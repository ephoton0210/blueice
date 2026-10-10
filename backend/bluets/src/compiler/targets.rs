// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Ordered ECMAScript emit targets, shared by API and configuration readers.

/// The selected output syntax edition. Library selection is independent:
/// choosing an older target does not remove owner-provided types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EcmaTarget {
    Es5,
    Es2015,
    Es2016,
    Es2017,
    Es2018,
    Es2019,
    Es2020,
    Es2021,
    Es2022,
    Es2023,
    EsNext,
}

impl EcmaTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Es5 => "es5",
            Self::Es2015 => "es2015",
            Self::Es2016 => "es2016",
            Self::Es2017 => "es2017",
            Self::Es2018 => "es2018",
            Self::Es2019 => "es2019",
            Self::Es2020 => "es2020",
            Self::Es2021 => "es2021",
            Self::Es2022 => "es2022",
            Self::Es2023 => "es2023",
            Self::EsNext => "esnext",
        }
    }

    /// TypeScript's case-insensitive option spelling; ES6 aliases ES2015.
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "es5" => Some(Self::Es5),
            "es6" | "es2015" => Some(Self::Es2015),
            "es2016" => Some(Self::Es2016),
            "es2017" => Some(Self::Es2017),
            "es2018" => Some(Self::Es2018),
            "es2019" => Some(Self::Es2019),
            "es2020" => Some(Self::Es2020),
            "es2021" => Some(Self::Es2021),
            "es2022" => Some(Self::Es2022),
            "es2023" => Some(Self::Es2023),
            "esnext" => Some(Self::EsNext),
            _ => None,
        }
    }
}
