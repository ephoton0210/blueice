// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frozen, machine-readable K.11 public compiler-oracle matrix.

use blueice_bluets::{EcmaTarget, ModuleKind};
use std::path::Path;

const MATRIX: &str =
    include_str!("../fixtures/k11_source_maps/source-map-tokens-checker-matrix.tsv");

const MATRIX_CASES: [&str; 11] = [
    "k11-basic-copy",
    "k11-erased-annotation",
    "k11-standard-method",
    "k11-compacted-relocation",
    "k11-erased-relocation",
    "k11-composed-decorators",
    "k11-legacy-class-member",
    "k11-legacy-parameter-metadata",
    "k11-nondecorator-helper",
    "k11-system-wrapper",
    "k11-crlf-astral",
];

pub(super) struct MatrixRow<'a> {
    pub(super) name: &'a str,
    pub(super) file: &'a str,
    pub(super) line_endings: &'a str,
    pub(super) target: EcmaTarget,
    pub(super) module: ModuleKind,
    pub(super) legacy: bool,
    pub(super) metadata: bool,
    pub(super) system_runner: bool,
}

pub(super) fn matrix_rows() -> Vec<MatrixRow<'static>> {
    let mut lines = MATRIX.lines();
    assert_eq!(
        lines.next(),
        Some("case\tverdict\tfile\tline_endings\ttarget\tmodule\texperimental_decorators\temit_decorator_metadata\tsource_map\tinline_sources\trunner")
    );
    let rows = lines
        .map(|line| {
            let fields = line.split('\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 11, "invalid matrix row: {line}");
            assert_eq!(fields[1], "accept", "{}: verdict", fields[0]);
            let target = EcmaTarget::parse(fields[4]).expect("matrix target");
            let module = match fields[5] {
                "ES2022" => ModuleKind::Esm,
                "System" => ModuleKind::System,
                other => panic!("{}: unsupported module {other}", fields[0]),
            };
            let flag = |value| match value {
                "true" => true,
                "false" => false,
                other => panic!("{}: invalid Boolean {other}", fields[0]),
            };
            assert_eq!(fields[8], "external", "{}: map mode", fields[0]);
            assert_eq!(fields[9], "both", "{}: inlineSources modes", fields[0]);
            let system_runner = match fields[10] {
                "node" => false,
                "system" => true,
                other => panic!("{}: unsupported runner {other}", fields[0]),
            };
            assert_eq!(system_runner, module == ModuleKind::System);
            MatrixRow {
                name: fields[0],
                file: fields[2],
                line_endings: fields[3],
                target,
                module,
                legacy: flag(fields[6]),
                metadata: flag(fields[7]),
                system_runner,
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), MATRIX_CASES.len(), "K.11 row count");
    assert_eq!(
        rows.iter().map(|row| row.name).collect::<Vec<_>>(),
        MATRIX_CASES,
        "missing, duplicated or reordered K.11 matrix row"
    );
    rows
}

#[test]
fn matrix_has_all_required_public_oracle_cases() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/k11_source_maps");
    for row in matrix_rows() {
        assert!(
            corpus.join(row.file).is_file(),
            "{}: missing fixture",
            row.name
        );
        assert!(
            !row.metadata || row.legacy,
            "{}: metadata needs legacy decorators",
            row.name
        );
    }
}
