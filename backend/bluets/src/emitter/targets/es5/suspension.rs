// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Continuations are compiled from the real parser's function-body records.

use super::*;
use crate::emitter::relocations;

mod graph;
mod native_abi;
pub(crate) const NATIVE_ABI_SOURCE: &str = native_abi::SOURCE;

#[derive(Clone, Copy)]
enum Kind {
    Generator,
    Async,
    AsyncGenerator,
}

pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    let source_tokens = crate::syntax::lex_with_limits(
        &module.id,
        &module.source,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut diagnostics| diagnostics.remove(0))?;
    let mut remaining = options.limits.parser.max_tokens;
    let mut sequence = 0;
    loop {
        let tokens = crate::syntax::lex_with_limits(
            &module.id,
            &emitted.javascript,
            options.limits.parser.max_source_bytes,
            remaining,
        )
        .map_err(|mut diagnostics| diagnostics.remove(0))?;
        remaining = remaining.saturating_sub(tokens.len());
        let candidate = (0..tokens.len()).rev().find_map(|start| {
            let (kind, keyword) =
                if tokens[start].is("async") && tokens.get(start + 1)?.is("function") {
                    (
                        if tokens.get(start + 2)?.is("*") {
                            Kind::AsyncGenerator
                        } else {
                            Kind::Async
                        },
                        start + 1,
                    )
                } else if tokens[start].is("function")
                    && tokens.get(start + 1)?.is("*")
                    && !(start > 0 && tokens[start - 1].is("async"))
                {
                    (Kind::Generator, start)
                } else {
                    return None;
                };
            let name = keyword + 1 + usize::from(tokens[keyword + 1].is("*"));
            if tokens.get(name)?.kind != TokenKind::Identifier || !tokens.get(name + 1)?.is("(") {
                return None;
            }
            let function = async_functions::function_at_parameters(&tokens, name + 1)?;
            let source = &emitted.javascript[tokens[start].start..tokens[function.body_end].end];
            Some((start, name, kind, function, source.to_string()))
        });
        let Some((start, name, kind, function, source)) = candidate else {
            return Ok(emitted);
        };
        let parsed =
            crate::parser::parse_emitted_module(&module.id, &source, &options.limits.parser)
                .map_err(|mut diagnostics| diagnostics.remove(0))?;
        let Some(Declaration::Function(declaration)) = parsed.declarations.first() else {
            return Err(unsupported(
                module,
                &SourceSpan::new(&module.id, 0, 0),
                "generator declaration was not retained",
            ));
        };
        let external_abi = crate::emitter::helper_selection::external(options);
        let helper = if external_abi {
            crate::emitter::helper_selection::reference(&emitted.javascript, "__generator")
        } else {
            unused_name(&emitted.javascript, "generator", &mut sequence)
        };
        let context = unused_name(&emitted.javascript, "generator_context", &mut sequence);
        let arguments = unused_name(&emitted.javascript, "generator_arguments", &mut sequence);
        let marker = matches!(kind, Kind::AsyncGenerator)
            .then(|| unused_name(&emitted.javascript, "generator_suspension", &mut sequence));
        let Some(graph) = graph::compile(
            (&parsed, tokens[start].start, &emitted),
            declaration,
            &context,
            &arguments,
            marker.as_deref(),
            external_abi,
            &options.limits.parser,
        ) else {
            return Err(unsupported(
                module,
                &SourceSpan::new(&module.id, 0, 0),
                "this suspension control-flow shape cannot be lowered to ES5",
            ));
        };
        let parameters = relocations::relocated_verbatim(
            tokens[function.parameters].end,
            tokens[function.parameters_end].start,
        );
        let parameter_list = relocations::relocated_verbatim(
            tokens[function.parameters].start,
            tokens[function.parameters_end].end,
        );
        let name_completion = if matches!(kind, Kind::Async) {
            format!(
                "{} ",
                async_functions::named_completion(&emitted, module, &source_tokens, &tokens[name])
            )
        } else {
            String::new()
        };
        let name = relocations::relocated_verbatim(tokens[name].start, tokens[name].end);
        let arrow = matches!(kind, Kind::Async)
            && async_functions::arrow_at(&emitted, module, tokens[start].start);
        let plain_arrow =
            arrow && async_functions::plain_arrow_formals(&emitted, module, tokens[start].start);
        let parameters_anchor = if plain_arrow {
            relocations::source_anchor(tokens[start].start)
        } else if arrow {
            relocations::unmapped().to_string()
        } else {
            relocations::source_anchor(tokens[function.parameters].start)
        };
        let signature_anchor = relocations::source_anchor(tokens[function.parameters_end].start);
        let body_anchor = relocations::source_anchor(tokens[function.body].start);
        let generated = relocations::unmapped();
        let body = if external_abi {
            native_abi::body(
                &graph,
                &context,
                &arguments,
                &helper,
                matches!(kind, Kind::AsyncGenerator),
                &emitted.javascript,
                &mut sequence,
            )
        } else {
            format!(
            "var {arguments} = arguments;\n{}\
             return {helper}(this, {}, function ({context}) {{\nwhile (true) {{ switch ({context}.label) {{\n{}\
             default: throw new Error(\"Invalid generator continuation\");\n}} }}\n}});",
            graph.declarations,graph.entry,graph.cases,
        )
        };
        let (replacement, driver_source) = match kind {
            Kind::Generator => (
                format!("function {name}{parameter_list} {{\n{generated}{body}\n{generated}}}"),
                String::new(),
            ),
            Kind::Async | Kind::AsyncGenerator => {
                let count = async_functions::parameter_length(
                    &tokens[function.parameters + 1..function.parameters_end],
                );
                let external = (0..count)
                    .map(|_| unused_name(&emitted.javascript, "async_argument", &mut sequence))
                    .collect::<Vec<_>>()
                    .join(", ");
                let driver = if external_abi {
                    crate::emitter::helper_selection::reference(
                        &emitted.javascript,
                        if matches!(kind, Kind::AsyncGenerator) {
                            "__asyncGenerator"
                        } else {
                            "__awaiter"
                        },
                    )
                } else {
                    unused_name(&emitted.javascript, "async_driver", &mut sequence)
                };
                let driver_source = if external_abi {
                    String::new()
                } else if let Some(marker) = &marker {
                    include_str!("../async_generator_helpers.v1.js")
                        .replace("__blueice_target_async_generator", &driver)
                        .replace("__blueice_target_generator_suspension", marker)
                } else {
                    include_str!("../async_helpers.v1.js")
                        .replace("__blueice_target_async", &driver)
                };
                let original_generator = matches!(kind, Kind::AsyncGenerator);
                let driver_anchor = if plain_arrow || original_generator {
                    signature_anchor.as_str()
                } else {
                    ""
                };
                let callback_anchor = if plain_arrow || original_generator {
                    signature_anchor.as_str()
                } else {
                    generated
                };
                let return_anchor = if original_generator {
                    signature_anchor.as_str()
                } else {
                    ""
                };
                let outer_parameters_anchor = if original_generator && external.is_empty() {
                    signature_anchor.as_str()
                } else {
                    generated
                };
                (format!("function {name}{name_completion}{outer_parameters_anchor}({external}) {body_anchor}{{{generated}\n{return_anchor}return {driver_anchor}{driver}(this, arguments, {}{callback_anchor}function {parameters_anchor}({parameters}{signature_anchor}) {{{generated}\n{body}\n{generated}}});\n{generated}}}",if external_abi&&matches!(kind,Kind::Async) {"void 0, "}else{""}), driver_source)
            }
        };
        let iterator = unused_name(&emitted.javascript, "delegate_iterator", &mut sequence);
        let step = unused_name(&emitted.javascript, "delegate_step", &mut sequence);
        let helper_source = if external_abi {
            String::new()
        } else {
            include_str!("../generator_helpers.v1.js")
                .replace("__blueice_target_generator", &helper)
                .replace("__blueice_target_iterator", &iterator)
                .replace("__blueice_target_step_result", &step)
        };
        let method = matches!(kind, Kind::Async)
            && async_functions::method_end_at(&emitted, module, tokens[function.body_end].start)
                .is_some();
        let declaration_end = if method {
            tokens
                .get(function.body_end + 1)
                .filter(|next| {
                    next.is(")")
                        && emitted.javascript[tokens[function.body_end].end..next.start]
                            .chars()
                            .all(char::is_whitespace)
                })
                .map_or(tokens[function.body_end].end, |next| next.start)
        } else if matches!(kind, Kind::Async) {
            async_functions::async_callable_end(&emitted.javascript, &tokens, &function)
        } else {
            tokens[function.body_end].end
        };
        let iterator_source = if external_abi {
            String::new()
        } else {
            include_str!("../generator_delegation_helpers.v1.js")
                .replace("__blueice_target_iterator", &iterator)
                .replace("__blueice_target_step_result", &step)
        };
        let insertion = directive_end(&tokens);
        emitted = mapped_edits(
            emitted,
            vec![
                TextEdit {
                    start: insertion,
                    end: insertion,
                    replacement: format!("\n{helper_source}\n{iterator_source}\n{driver_source}\n"),
                },
                TextEdit {
                    start: tokens[start].start,
                    end: declaration_end,
                    replacement,
                },
            ],
        );
    }
}
