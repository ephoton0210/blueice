// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Capture the complete adjacent matrices before their independent Native role replay.
#[allow(dead_code)]
#[path = "oracle_support/ge5_cli.rs"]
mod cli;
#[allow(dead_code)]
#[path = "oracle_support/ge5_declarations.rs"]
mod declarations;
#[allow(dead_code)]
#[path = "oracle_support/gd2_maps.rs"]
mod decoded;
#[allow(dead_code)]
#[path = "oracle_support/ge5_inputs.rs"]
mod inputs;
#[allow(dead_code)]
#[path = "oracle_support/ge5_javascript_maps.rs"]
mod javascript_maps;
#[path = "oracle_support/project_paths.rs"]
mod project_paths;

#[test]
#[ignore = "complete class661/legacy363 public CLI collection; requires subsequent independent role verifiers"]
fn collect_both_complete_adjacent_matrices_with_two_public_builds() {
    inputs::verify_authorities();
    let directory = std::path::PathBuf::from(
        std::env::var_os("BLUEICE_GE5_ADJACENT_OBSERVATION_DIRECTORY")
            .expect("an explicit adjacent observation directory"),
    );
    std::fs::create_dir_all(&directory).unwrap();
    let temporary = cli::Temporary::new("decorator-adjacent-matrices");
    let compiler = inputs::sha(&std::fs::read(env!("CARGO_BIN_EXE_bluetsc")).unwrap());
    let specifications = inputs::input()["cases"].as_array().unwrap();
    let class_forms = [
        "ge1_downlevel/called-imported-heritage",
        "ge1_downlevel/indexed-heritage",
        "ge1_downlevel/computed-constructor-spread",
        "ge1_downlevel/constructor-and-super-spread",
        "ge1_downlevel/computed-super-call-order",
        "ge1_downlevel/super-write-receiver-order",
        "ge2_module_helpers/inherit-constructor-spread",
    ];
    let selection = specifications
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            row["form"].as_str().is_some_and(|form| {
                class_forms.contains(&form)
                    || form == "ge2_module_helpers/legacy-parameter-decorator"
            })
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(selection.len(), 1024);
    let mut rows = std::thread::scope(|scope| {
        let workers = (0..2).map(|worker| {
            let selection=&selection;let temporary=&temporary;let compiler=&compiler;
            scope.spawn(move || {
                let mut rows=Vec::new();
                for (position,&index) in selection.iter().enumerate() {
                    if position%2!=worker {continue;}
                    let specification=&specifications[index];let original=inputs::resolve(specification);
                    let actual=cli::observe_builds(temporary,index,&original,2);
                    let differences=declarations::compare(specification,&original,&inputs::reference()["cases"][index],&actual);
                    assert!(differences.is_empty(),"{}",differences.join("\n"));
                    rows.push(serde_json::json!({"index":index,"id":specification["id"],"compilerSha256":compiler,"repeatBuilds":2,"options":original["options"],"outputs":actual.outputs}));
                }
                rows
            })
        }).collect::<Vec<_>>();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    rows.sort_by_key(|row| row["index"].as_u64().unwrap());
    let (class, legacy): (Vec<_>, Vec<_>) = rows.into_iter().partition(|row| {
        specifications[row["index"].as_u64().unwrap() as usize]["form"]
            != "ge2_module_helpers/legacy-parameter-decorator"
    });
    assert_eq!(class.len(), 661);
    assert_eq!(legacy.len(), 363);
    for (name, rows) in [("class-super", class), ("decorator-legacy-whole", legacy)] {
        std::fs::write(
            directory.join(format!("{name}.json")),
            serde_json::to_vec(&rows).unwrap(),
        )
        .unwrap();
    }
}
