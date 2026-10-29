// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Recursive projection retains single reads, defaults and iterator closing.
use super::*;
use crate::parser::{parse_variable_pattern, BindingPattern, VariableBindingPattern};

pub(super) fn lower(
    emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    lower_at_scope(emitted, module, options, false)
}

pub(in crate::emitter) fn lower_system(
    emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    lower_at_scope(emitted, module, options, true)
}

fn lower_at_scope(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
    top_level_only: bool,
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
        .map_err(|mut d| d.remove(0))?;
        remaining = remaining.saturating_sub(tokens.len());
        let mut depth = 0usize;
        let candidate = tokens.iter().enumerate().find_map(|(index, token)| {
            let at_top_level = depth == 0;
            match token.text.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth = depth.saturating_sub(1),
                _ => {}
            }
            if top_level_only && !at_top_level {
                return None;
            }
            if !matches!(tokens.get(index + 1)?.text.as_str(), "{" | "[") {
                return None;
            }
            if !(matches!(token.text.as_str(), "var" | "let" | "const")
                || token.is(",") && in_declaration(&tokens, index))
            {
                return None;
            }
            let (pattern, close) = parse_variable_pattern(
                &tokens,
                index + 1,
                &module.id,
                options.limits.parser.max_type_depth,
            )?;
            tokens
                .get(close + 1)?
                .is("=")
                .then_some((index + 1, close, pattern))
        });
        let Some((open, close, pattern)) = candidate else {
            return Ok(emitted);
        };
        let end = assignment_end(&tokens, close + 2);
        if end <= close + 2 {
            return Err(unsupported(
                module,
                &SourceSpan::new(&module.id, 0, 0),
                "binding initializer was not retained",
            ));
        }
        let source = &emitted.javascript;
        let external = crate::emitter::helper_selection::external(options);
        let mut projector = Projector {
            external,
            source,
            sequence: &mut sequence,
            iterate: options.downlevel_iteration,
            require: if external {
                format!(
                    "({})",
                    include_str!("../es5_helpers.v1.js")
                        .replace("function __blueice_target_require_object", "function")
                )
            } else {
                unused_name(source, "require_object", &mut 0)
            },
            read: if external {
                crate::emitter::helper_selection::reference(source, "__read")
            } else {
                unused_name(source, "binding_read", &mut 0)
            },
            iterator: unused_name(source, "binding_iterator", &mut 0),
            step: unused_name(source, "binding_step", &mut 0),
            rest: if external {
                crate::emitter::helper_selection::reference(source, "__rest")
            } else {
                unused_name(source, "binding_rest", &mut 0)
            },
            objects: false,
            arrays: false,
            rests: false,
            parts: Vec::new(),
        };
        let value = &source[tokens[close + 2].start..tokens[end - 1].end];
        projector
            .project(&pattern, value, options.limits.parser.max_type_depth)
            .ok_or_else(|| {
                Diagnostic::error(
                    DiagnosticCode::ResourceLimit,
                    SourceSpan::new(&module.id, 0, 0),
                    "nested binding lowering exceeds the depth limit",
                )
            })?;
        let replacement = projector.parts.join(", ");
        let mut helper = String::new();
        if projector.objects && !external {
            helper.push_str(
                &include_str!("../es5_helpers.v1.js")
                    .replace("__blueice_target_require_object", &projector.require),
            );
        }
        if projector.arrays && !external {
            helper.push_str(
                &include_str!("../binding_helpers.v1.js")
                    .replace("__blueice_target_binding_read", &projector.read)
                    .replace("__blueice_target_binding_iterator", &projector.iterator)
                    .replace("__blueice_target_binding_step", &projector.step),
            );
            helper.push_str(
                &include_str!("../iteration_helpers.v1.js")
                    .replace("__blueice_target_iterator", &projector.iterator)
                    .replace("__blueice_target_step_result", &projector.step),
            );
        }
        if projector.rests && !external {
            helper.push_str(
                &object_spread::HELPER
                    .replace("__blueice_target_object_properties", &projector.rest),
            );
        }
        let insertion = directive_end(&tokens);
        emitted = mapped_edits(
            emitted,
            vec![
                TextEdit {
                    start: tokens[open].start,
                    end: tokens[end - 1].end,
                    replacement,
                },
                TextEdit {
                    start: insertion,
                    end: insertion,
                    replacement: format!("\n{helper}\n"),
                },
            ],
        );
    }
}

struct Projector<'a> {
    external: bool,
    source: &'a str,
    sequence: &'a mut usize,
    iterate: bool,
    require: String,
    read: String,
    iterator: String,
    step: String,
    rest: String,
    objects: bool,
    arrays: bool,
    rests: bool,
    parts: Vec<String>,
}
impl Projector<'_> {
    fn temporary(&mut self, kind: &str) -> String {
        unused_name(self.source, kind, self.sequence)
    }
    fn project(
        &mut self,
        pattern: &VariableBindingPattern,
        value: &str,
        depth: usize,
    ) -> Option<()> {
        let depth = depth.checked_sub(1)?;
        let source = self.temporary("pattern_source");
        match &pattern.pattern {
            BindingPattern::Object(bindings) => {
                self.objects = true;
                self.parts
                    .push(format!("{source}={}({value})", self.require));
                for binding in bindings {
                    let key = serde_json::to_string(&binding.key).ok()?;
                    let read = format!("{source}[{key}]");
                    self.binding(
                        &binding.name,
                        binding.nested.as_deref(),
                        binding.default.as_deref(),
                        &read,
                        depth,
                    )?;
                }
                if let Some(rest) = &pattern.rest {
                    self.rests = true;
                    let excluded =
                        serde_json::to_string(&bindings.iter().map(|b| &b.key).collect::<Vec<_>>())
                            .ok()?;
                    self.parts.push(if self.external {
                        format!("{}={}({source},{excluded})", rest.name, self.rest)
                    } else {
                        format!("{}={}({{}},{source},true,{excluded})", rest.name, self.rest)
                    });
                }
            }
            BindingPattern::Array(bindings) => {
                let value = if self.iterate {
                    self.arrays = true;
                    let count = if pattern.rest.is_some() {
                        if self.external {
                            "void 0".to_string()
                        } else {
                            "null".to_string()
                        }
                    } else {
                        bindings.len().to_string()
                    };
                    format!("{}({value},{count})", self.read)
                } else {
                    value.to_string()
                };
                self.parts.push(format!("{source}={value}"));
                for (index, binding) in bindings.iter().enumerate() {
                    if let Some(binding) = binding {
                        self.binding(
                            &binding.name,
                            binding.nested.as_deref(),
                            binding.default.as_deref(),
                            &format!("{source}[{index}]"),
                            depth,
                        )?;
                    }
                }
                if let Some(rest) = &pattern.rest {
                    self.parts
                        .push(format!("{}={source}.slice({})", rest.name, bindings.len()));
                }
            }
        }
        Some(())
    }
    fn binding(
        &mut self,
        name: &str,
        nested: Option<&VariableBindingPattern>,
        default: Option<&[Token]>,
        read: &str,
        depth: usize,
    ) -> Option<()> {
        let value = if let Some((first, last)) =
            default.and_then(|tokens| Some((tokens.first()?, tokens.last()?)))
        {
            let value = self.temporary("pattern_value");
            self.parts.push(format!("{value}={read}"));
            format!(
                "{value}===void 0?({}):{value}",
                &self.source[first.start..last.end]
            )
        } else {
            read.to_string()
        };
        if let Some(nested) = nested {
            self.project(nested, &format!("({value})"), depth)
        } else {
            self.parts.push(format!("{name}={value}"));
            Some(())
        }
    }
}
fn in_declaration(tokens: &[Token], before: usize) -> bool {
    let mut depth = 0;
    for token in tokens[..before].iter().rev() {
        match token.text.as_str() {
            ")" | "]" | "}" => depth += 1,
            "(" | "[" | "{" if depth > 0 => depth -= 1,
            "(" | "[" | "{" | ";" if depth == 0 => return false,
            "var" | "let" | "const" if depth == 0 => return true,
            _ => {}
        }
    }
    false
}
