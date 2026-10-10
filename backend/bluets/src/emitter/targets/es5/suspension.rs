// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Continuations are compiled from the real parser's function-body records.

use super::*;

mod graph;

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
        let helper = unused_name(&emitted.javascript, "generator", &mut sequence);
        let context = unused_name(&emitted.javascript, "generator_context", &mut sequence);
        let arguments = unused_name(&emitted.javascript, "generator_arguments", &mut sequence);
        let marker = matches!(kind, Kind::AsyncGenerator)
            .then(|| unused_name(&emitted.javascript, "generator_suspension", &mut sequence));
        let Some(graph) = graph::compile(
            &parsed,
            declaration,
            &context,
            &arguments,
            marker.as_deref(),
            &options.limits.parser,
        ) else {
            return Err(unsupported(
                module,
                &SourceSpan::new(&module.id, 0, 0),
                "this suspension control-flow shape cannot be lowered to ES5",
            ));
        };
        let parameters = &emitted.javascript
            [tokens[function.parameters].end..tokens[function.parameters_end].start];
        let name = &tokens[name].text;
        let body = format!(
            "var {arguments} = arguments;\n{}\
             return {helper}(this, {}, function ({context}) {{\nwhile (true) {{ switch ({context}.label) {{\n{}\
             default: throw new Error(\"Invalid generator continuation\");\n}} }}\n}});",
            graph.declarations,graph.entry,graph.cases,
        );
        let (replacement, driver_source) = match kind {
            Kind::Generator => (
                format!("function {name}({parameters}) {{\n{body}\n}}"),
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
                let driver = unused_name(&emitted.javascript, "async_driver", &mut sequence);
                let driver_source = if let Some(marker) = &marker {
                    include_str!("../async_generator_helpers.v1.js")
                        .replace("__blueice_target_async_generator", &driver)
                        .replace("__blueice_target_generator_suspension", marker)
                } else {
                    include_str!("../async_helpers.v1.js")
                        .replace("__blueice_target_async", &driver)
                };
                (format!("function {name}({external}) {{\nreturn {driver}(this, arguments, function ({parameters}) {{\n{body}\n}});\n}}"), driver_source)
            }
        };
        let iterator = unused_name(&emitted.javascript, "delegate_iterator", &mut sequence);
        let step = unused_name(&emitted.javascript, "delegate_step", &mut sequence);
        let helper_source = include_str!("../generator_helpers.v1.js")
            .replace("__blueice_target_generator", &helper)
            .replace("__blueice_target_iterator", &iterator)
            .replace("__blueice_target_step_result", &step);
        let iterator_source = include_str!("../iteration_helpers.v1.js")
            .replace("__blueice_target_iterator", &iterator)
            .replace("__blueice_target_step_result", &step);
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
                    end: tokens[function.body_end].end,
                    replacement,
                },
            ],
        );
    }
}
