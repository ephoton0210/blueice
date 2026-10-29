// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Source continuations use the actual tslib iterator/exception protocol.
use super::*;
use std::collections::BTreeMap;

pub(crate) const SOURCE: &str = include_str!("native_abi.v1.js");

pub(super) fn body(
    graph: &graph::Graph,
    context: &str,
    arguments: &str,
    helper: &str,
    asynchronous: bool,
    source: &str,
    sequence: &mut usize,
) -> String {
    let blocks = graph.native.as_ref().expect("external ABI graph");
    let count = blocks.blocks.len();
    let native = unused_name(source, "native_generator_context", sequence);
    let action = unused_name(source, "native_generator_action", sequence);
    let exits = unused_name(source, "native_generator_exits", sequence);
    let pending = unused_name(source, "native_generator_pending_yield", sequence);
    let normalized = blocks
        .exits
        .iter()
        .map(|(leave, after)| {
            let mut target = *after;
            while let Some(after) = blocks.exits.get(&target) {
                target = *after;
            }
            (*leave, count - target)
        })
        .collect::<BTreeMap<_, _>>();
    let cases = blocks
        .blocks
        .iter()
        .enumerate()
        .map(|(label, code)| {
            let position = if blocks.exits.contains_key(&label) || blocks.finishes.contains(&label)
            {
                String::new()
            } else {
                format!("{native}.label = {}; ", count - label)
            };
            format!(
                "case {label}: {position}{}\n",
                code.replace(&format!("{context}.exception"), &format!("{native}.sent()"))
            )
        })
        .collect::<String>();
    let delegate = if blocks.delegation {
        let values = crate::emitter::helper_selection::reference(source, "__values");
        format!("case \"delegate\": {native}.label = {count} - {context}.label - 1; return [5,{values}({action}.value)];")
    } else {
        String::new()
    };
    let (pending_branch, yielding) = if asynchronous {
        let awaiting = crate::emitter::helper_selection::reference(source, "__await");
        (format!("if ({pending}) {{ {pending}=false; {native}.label--; return [4,{native}.sent()]; }}"),
            format!("{pending} = !{action}.value.awaiting; return [4,new {awaiting}({action}.value.value)];"))
    } else {
        (String::new(), format!("return [4,{action}.value];"))
    };
    let body = SOURCE
        .replace("__BLUEICE_PENDING_BRANCH__", &pending_branch)
        .replace("__BLUEICE_CASES__", &cases)
        .replace("__BLUEICE_DELEGATE__", &delegate)
        .replace("__BLUEICE_YIELD__", &yielding)
        .replace("__BLUEICE_SOURCE__", context)
        .replace("__BLUEICE_NATIVE__", &native)
        .replace("__BLUEICE_ACTION__", &action)
        .replace("__BLUEICE_EXITS__", &exits)
        .replace("__BLUEICE_COUNT__", &count.to_string())
        .replace("__BLUEICE_ENTRY__", &graph.entry.to_string());
    format!("var {arguments} = arguments;\n{}\nvar {pending}=false; var {exits}={};\nreturn {helper}(this,function({native}){{\n{body}\n}});",
        graph.declarations,serde_json::to_string(&normalized).expect("continuation exits are finite"))
}
