// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Recursive projection retains single reads, defaults and iterator closing.
use super::*;
mod origins;
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
    let emitted = lower_at_scope(emitted, module, options, true)?;
    let emitted = origins::system_arrays(emitted, module, options)?;
    system_rest_completions(emitted, module, options)
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
            tokens: &tokens,
            objects: false,
            arrays: false,
            rests: false,
            parts: Vec::new(),
        };
        let value_start = tokens[close + 2].start;
        let value_end = tokens[end - 1].end;
        let value = crate::emitter::relocations::relocated_verbatim(value_start, value_end);
        let stable = end == close + 3 && module.declarations.iter().any(|declaration| matches!(declaration, Declaration::Variable(variable) if variable.kind == crate::parser::VariableKind::Const && variable.name == tokens[close + 2].text));
        projector
            .project(
                &pattern,
                &value,
                options.limits.parser.max_type_depth,
                Projection {
                    entry: tokens[open].start,
                    value_end,
                    pattern_end: tokens[close].end,
                    root: true,
                    stable,
                    inline: end == close + 3
                        || tokens[close + 2].is("{")
                            && exponentiation::matching_close(&tokens, close + 2) == Some(end - 1),
                },
            )
            .ok_or_else(|| {
                Diagnostic::error(
                    DiagnosticCode::ResourceLimit,
                    SourceSpan::new(&module.id, 0, 0),
                    "nested binding lowering exceeds the depth limit",
                )
            })?;
        let mut replacement = projector.parts.join(", ");
        let keyword = open - 1;
        let start = if matches!(tokens[keyword].text.as_str(), "var" | "let" | "const") {
            let entry = if stable && pattern.rest.is_some() {
                match &pattern.pattern {
                    BindingPattern::Object(bindings) if bindings.len() == 1 => {
                        bindings[0].span.start
                    }
                    _ => tokens[open].start,
                }
            } else {
                first_origin(&pattern, tokens[open].start)
            };
            let prefix = &emitted.javascript[..tokens[keyword].start];
            let key = (
                prefix.bytes().filter(|byte| *byte == b'\n').count(),
                prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
            );
            let point = emitted
                .provenance
                .partition_point(|point| (point.generated_line, point.generated_column) <= key);
            let keyword_anchor = point
                .checked_sub(1)
                .and_then(|index| emitted.provenance.get(index))
                .filter(|point| point.generated_line == key.0 && point.mapped)
                .map_or_else(
                    || crate::emitter::relocations::unmapped().to_string(),
                    |_| anchor(entry),
                );
            replacement = format!("{keyword_anchor}{} {replacement}", tokens[keyword].text);
            tokens[keyword].start
        } else {
            tokens[open].start
        };
        let endpoint = if let Some(rest) = pattern
            .rest
            .as_ref()
            .filter(|_| stable && matches!(pattern.pattern, BindingPattern::Object(_)))
        {
            rest.span.end
        } else if let BindingPattern::Array(bindings) = &pattern.pattern {
            if top_level_only {
                bindings
                    .iter()
                    .flatten()
                    .next_back()
                    .map_or(value_end, |binding| binding.span.end)
            } else {
                value_end
            }
        } else {
            final_origin(&pattern, tokens[close].end)
        };
        replacement.push_str(&anchor(endpoint));
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
        emitted = super::super::mapped_fragments::with_completion(
            emitted,
            vec![
                TextEdit {
                    start,
                    end: tokens[end - 1].end,
                    replacement,
                },
                TextEdit {
                    start: insertion,
                    end: insertion,
                    replacement: format!("{}\n{helper}\n", crate::emitter::relocations::unmapped()),
                },
            ],
            tokens[end - 1].end,
            endpoint,
        );
    }
}

struct Projection {
    entry: usize,
    value_end: usize,
    pattern_end: usize,
    root: bool,
    stable: bool,
    inline: bool,
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
    tokens: &'a [Token],
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
        origin: Projection,
    ) -> Option<()> {
        let Projection {
            entry,
            value_end,
            pattern_end,
            root,
            stable,
            inline,
        } = origin;
        let depth = depth.checked_sub(1)?;
        if let BindingPattern::Object(bindings) = &pattern.pattern {
            if let [binding] = bindings.as_slice() {
                if inline && (pattern.rest.is_none() || stable) && binding.default.is_none() {
                    let read = format!("{value}{}.{}", anchor(value_end), binding.key);
                    self.binding(
                        &binding.name,
                        binding.nested.as_deref(),
                        None,
                        &read,
                        depth,
                        (&binding.span, value_end),
                    )?;
                    if let Some(rest) = &pattern.rest {
                        self.rests = true;
                        self.parts.last_mut()?.push_str(&anchor(binding.span.end));
                        let name = self.tokens.iter().find(|token| {
                            token.start >= rest.span.start
                                && token.end <= rest.span.end
                                && token.is(&rest.name)
                        })?;
                        let keys = serde_json::to_string(&[&binding.key]).ok()?;
                        let generated = crate::emitter::relocations::unmapped();
                        let arguments = if self.external {
                            format!("{value}{}, {}{keys}", anchor(value_end), anchor(entry))
                        } else {
                            format!(
                                "{generated}{{}}, {value}{},{generated} true, {}{keys}",
                                anchor(value_end),
                                anchor(entry)
                            )
                        };
                        self.parts.push(format!(
                            "{}{}{}={}({arguments}{})",
                            anchor(name.start),
                            rest.name,
                            anchor(name.end),
                            self.rest,
                            anchor(pattern_end)
                        ));
                    }
                    return Some(());
                }
            }
        }
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
                        (&binding.span, value_end),
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
                    format!(
                        "{}{}({value}{},{count})",
                        anchor(entry),
                        self.read,
                        anchor(value_end)
                    )
                } else {
                    value.to_string()
                };
                self.parts.push(format!(
                    "{}{source}={value}{}",
                    anchor(entry),
                    anchor(if root { value_end } else { pattern_end })
                ));
                for (index, binding) in bindings.iter().enumerate() {
                    if let Some(binding) = binding {
                        self.binding(
                            &binding.name,
                            binding.nested.as_deref(),
                            binding.default.as_deref(),
                            &format!("{source}[{index}]"),
                            depth,
                            (
                                &binding.span,
                                if binding.nested.is_some() {
                                    binding.span.start
                                } else {
                                    binding.span.end
                                },
                            ),
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
        origin: (&SourceSpan, usize),
    ) -> Option<()> {
        let (span, read_origin) = origin;
        if let Some(nested) = nested {
            if !self.iterate && default.is_none() {
                if let BindingPattern::Array(bindings) = &nested.pattern {
                    if nested.rest.is_none() && bindings.iter().flatten().count() == 1 {
                        for (index, binding) in bindings.iter().enumerate() {
                            if let Some(binding) = binding {
                                self.binding(
                                    &binding.name,
                                    binding.nested.as_deref(),
                                    binding.default.as_deref(),
                                    &format!("{read}[{index}]"),
                                    depth.checked_sub(1)?,
                                    (&binding.span, binding.span.end),
                                )?;
                            }
                        }
                        return Some(());
                    }
                }
            }
        }
        let name_token = self
            .tokens
            .iter()
            .find(|token| token.start >= span.start && token.end <= span.end && token.is(name));
        let name_start = name_token.map_or(span.start, |token| token.start);
        let name_end = name_token.map_or(span.end, |token| token.end);
        let value = if let Some((first, last)) =
            default.and_then(|tokens| Some((tokens.first()?, tokens.last()?)))
        {
            let value = self.temporary("pattern_value");
            self.parts.push(format!(
                "{}{value}={read}{}",
                anchor(name_start),
                anchor(last.end)
            ));
            format!(
                "{value}===void 0?{}{}:{value}",
                crate::emitter::relocations::relocated_verbatim(first.start, last.end),
                anchor(last.end)
            )
        } else {
            read.to_string()
        };
        if let Some(nested) = nested {
            self.project(
                nested,
                &value,
                depth,
                Projection {
                    entry: span.start,
                    value_end: read_origin,
                    pattern_end: span.end,
                    root: false,
                    stable: false,
                    inline: default.is_none(),
                },
            )
        } else {
            let read_anchor = if default.is_some() {
                name_end
            } else {
                read_origin
            };
            self.parts.push(format!(
                "{}{name}{}={}{}",
                anchor(name_start),
                anchor(name_end),
                anchor(read_anchor),
                value
            ));
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

fn anchor(offset: usize) -> String {
    crate::emitter::relocations::source_anchor(offset)
}

fn first_origin(pattern: &VariableBindingPattern, fallback: usize) -> usize {
    match &pattern.pattern {
        BindingPattern::Object(bindings)
            if bindings.len() == 1 && pattern.rest.is_none() && bindings[0].default.is_none() =>
        {
            let binding = &bindings[0];
            binding
                .nested
                .as_ref()
                .map_or(binding.span.start, |nested| {
                    first_origin(nested, binding.span.start)
                })
        }
        _ => fallback,
    }
}

fn final_origin(pattern: &VariableBindingPattern, fallback: usize) -> usize {
    match &pattern.pattern {
        BindingPattern::Object(bindings)
            if bindings.len() == 1 && pattern.rest.is_none() && bindings[0].default.is_none() =>
        {
            let binding = &bindings[0];
            binding.nested.as_ref().map_or(binding.span.end, |nested| {
                final_origin(nested, binding.span.end)
            })
        }
        _ => fallback,
    }
}

/// System converts declaration completions into assignment completions after
/// the target-specific rest transform, retaining the latter phase's endpoint.
fn system_rest_completions(
    emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    let patterns = module
        .declarations
        .iter()
        .filter_map(|declaration| {
            let Declaration::Variable(variable) = declaration else {
                return None;
            };
            let pattern = variable.pattern.as_ref()?;
            let rest = pattern.rest.as_ref()?;
            let BindingPattern::Object(bindings) = &pattern.pattern else {
                return None;
            };
            let [binding] = bindings.as_slice() else {
                return None;
            };
            (binding.default.is_none() && binding.nested.is_none())
                .then_some((binding.name.as_str(), rest.name.as_str()))
        })
        .collect::<Vec<_>>();
    let spreads = module
        .declarations
        .iter()
        .filter_map(|declaration| {
            let Declaration::Variable(variable) = declaration else {
                return None;
            };
            if options.target >= crate::EcmaTarget::Es2018
                || variable.pattern.is_some()
                || !variable
                    .initializer
                    .first()
                    .is_some_and(|token| token.is("{"))
            {
                return None;
            }
            let mut depth = 0usize;
            let spread = variable.initializer.iter().any(|token| {
                let spread = depth == 1 && token.is("...");
                match token.text.as_str() {
                    "(" | "[" | "{" => depth += 1,
                    ")" | "]" | "}" => depth = depth.saturating_sub(1),
                    _ => {}
                }
                spread
            });
            spread.then_some(variable.name.as_str())
        })
        .collect::<Vec<_>>();
    if patterns.is_empty() && spreads.is_empty() {
        return Ok(emitted);
    }
    let tokens = crate::syntax::lex_with_limits(
        &module.id,
        &emitted.javascript,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut diagnostics| diagnostics.remove(0))?;
    let mut edits = Vec::new();
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        let top = depth == 0;
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            _ => {}
        }
        if !top || !tokens.get(index + 1).is_some_and(|token| token.is("=")) {
            continue;
        }
        let end = assignment_end(&tokens, index + 2);
        let Some(boundary) = tokens.get(end) else {
            continue;
        };
        let origin = if let Some((ordinary, rest)) = patterns
            .iter()
            .find(|(ordinary, rest)| token.is(ordinary) || token.is(rest))
        {
            if token.is(rest) && boundary.is(";") {
                if options.target < crate::EcmaTarget::Es2018 {
                    tokens[end - 1].start
                } else {
                    token.end
                }
            } else if token.is(ordinary)
                && boundary.is(",")
                && options.target == crate::EcmaTarget::Es5
            {
                tokens[end - 1].start
            } else {
                continue;
            }
        } else if spreads.iter().any(|name| token.is(name)) && boundary.is(";") {
            tokens[end - 1].start
        } else {
            continue;
        };
        let trailing = if boundary.is(",") {
            origin
        } else {
            boundary.end
        };
        edits.push((
            TextEdit {
                start: boundary.start,
                end: boundary.end,
                replacement: format!("{}{}{}", anchor(origin), boundary.text, anchor(trailing)),
            },
            trailing,
        ));
    }
    let mut emitted = emitted;
    for (edit, trailing) in edits {
        let boundary = edit.end;
        emitted = super::super::mapped_fragments::with_completion(
            emitted,
            vec![edit],
            boundary,
            trailing,
        );
    }
    Ok(emitted)
}
