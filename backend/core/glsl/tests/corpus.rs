// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The interpreter's results on a corpus of shaders, held bit for bit (see `common`).
//! `BI_GOLDEN=print cargo test -p blueice-glsl --test corpus -- --nocapture` lists the
//! current fingerprints, for when a change is meant to alter a result.

mod common;

use common::{fingerprint, CORPUS};

/// `(name, fingerprint)`, from the interpreter as it was before it was optimized.
const GOLDEN: &[(&str, u64)] = &[
    ("arithmetic", 0xa66c2030721b4f52),
    ("matrices", 0xa45dc4164a4b8be3),
    ("float_builtins", 0xfb90925e0c828cf9),
    ("vector_builtins", 0x8e4c0378546b4013),
    ("control_flow", 0xc04ba144dbe07806),
    ("structs_and_arrays", 0x0639b2be8a7c2cf9),
    ("swizzles", 0x2acd2fdd9b83c5c6),
    ("lighting", 0x86fbecc76fde3277),
    ("vertex_like", 0xe53d25a45f5134c1),
    ("texturing", 0xd83dad2389b8a22e),
    ("integer_and_bool_vectors", 0x46a86c718071b37f),
];

#[test]
fn every_corpus_shader_compiles_and_runs() {
    for entry in CORPUS {
        let _ = fingerprint(entry);
    }
}

#[test]
fn results_are_bit_for_bit_what_they_were() {
    if std::env::var("BI_GOLDEN").is_ok() {
        for entry in CORPUS {
            println!(
                "        (\"{}\", 0x{:016x}),",
                entry.name,
                fingerprint(entry)
            );
        }
        return;
    }
    assert_eq!(
        GOLDEN.len(),
        CORPUS.len(),
        "a corpus shader has no recorded fingerprint"
    );
    for (entry, (name, expected)) in CORPUS.iter().zip(GOLDEN) {
        assert_eq!(entry.name, *name);
        assert_eq!(fingerprint(entry), *expected, "{name}: a result changed");
    }
}

#[test]
fn a_run_is_deterministic() {
    for entry in CORPUS {
        assert_eq!(fingerprint(entry), fingerprint(entry), "{}", entry.name);
    }
}

/// A fingerprint only protects what actually varies: every shader's outputs must differ from
/// one input set to the next, and be finite.
#[test]
fn every_corpus_shader_produces_varying_finite_outputs() {
    use blueice_glsl::interp::Machine;
    use common::{compile, randomize_inputs, TestTextures};
    for entry in CORPUS {
        let program = compile(entry);
        let mut m = Machine::new(&program, &TestTextures).unwrap();
        let mut rng = common::Rng(0x1234_5678_9abc_def0);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..common::ROUNDS {
            randomize_inputs(&mut m, &mut rng);
            m.run_main().unwrap();
            let mut lanes = Vec::new();
            for out in ["o0", "o1"] {
                let values = m.get_by_name(out).unwrap().floats().unwrap();
                assert!(
                    values.iter().all(|x| x.is_finite()),
                    "{}: {out} = {values:?}",
                    entry.name
                );
                lanes.extend(values.iter().map(|x| x.to_bits()));
            }
            seen.insert(lanes);
        }
        assert!(
            seen.len() > common::ROUNDS / 2,
            "{}: only {} distinct outputs in {} runs",
            entry.name,
            seen.len(),
            common::ROUNDS
        );
    }
}
