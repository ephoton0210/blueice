// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class completion survives the following generated module publication.
use super::*;

#[derive(Clone, Copy)]
enum LegacyPhase {
    None,
    MemberOnly,
    ConstructorReplacement { origin: usize, metadata: bool },
}

#[derive(Clone, Copy)]
pub(super) struct Original {
    start: usize,
    end: usize,
    close: usize,
    declaration: bool,
    pub(super) legacy_decorated: bool,
    standard_expression: bool,
    legacy_invocation: Option<usize>,
    legacy_phase: LegacyPhase,
    heritage: Option<(usize, usize)>,
    factory_start: Option<usize>,
    factory_end: Option<usize>,
    pub(super) generated_factory: bool,
    pub(super) standard_factory: bool,
    standard_class_decorated: bool,
    plain_public_static: bool,
}

/// The fallback constructor has no original constructor node. Its body close
/// and whole-callable endpoint belong to the exact original class instead.
pub(super) fn implicit_constructor_close(
    emitted: &mut EmittedJavaScript,
    module: &Module,
    close: usize,
    original: Option<Original>,
) {
    let Some(original) = original.filter(|_| emitted.segment_limit.is_some()) else {
        return;
    };
    point(emitted, module, close, Some(original.close));
    point(emitted, module, close + 1, Some(original.end));
}

pub(super) fn original_span(
    emitted: &EmittedJavaScript,
    module: &Module,
    tokens: &[Token],
    start: usize,
    options: &CompilerOptions,
) -> Option<Original> {
    emitted.segment_limit?;
    let name = &tokens[start + 1].text;
    let candidates = runtime_declarations(&module.declarations)
        .into_iter()
        .filter_map(|(declaration, _)| match declaration {
            Declaration::Class(class)
                if &class.name == name
                    || !options.experimental_decorators
                        && !class.decorators.is_empty()
                        && (class.extends_span.is_some()
                            || !class.members.is_empty()
                                && class.members.iter().all(|member| {
                                    !member.decorators.is_empty()
                                        && member.field.as_ref().is_some_and(|field| {
                                            !field.accessor && !field.name.starts_with('#')
                                        })
                                })) =>
            {
                Some(class)
            }
            _ => None,
        })
        .chain(
            module
                .class_expressions
                .values()
                .map(|expression| &expression.class),
        )
        .collect::<Vec<_>>();
    let source_position = |offset| {
        let prefix = &module.source[..offset];
        (
            prefix.bytes().filter(|byte| *byte == b'\n').count(),
            prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
        )
    };
    let origin = |offset| {
        let prefix = &emitted.javascript[..offset];
        let key = (
            prefix.bytes().filter(|byte| *byte == b'\n').count(),
            prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
        );
        let index = emitted
            .provenance
            .partition_point(|point| (point.generated_line, point.generated_column) <= key);
        index
            .checked_sub(1)
            .and_then(|index| emitted.provenance.get(index))
            .filter(|point| point.mapped && point.generated_line == key.0)
            .map(|point| (point.source_line, point.source_column))
    };
    let name_origin = origin(tokens[start + 1].start);
    let keyword_origin = origin(tokens[start].start);
    let matched = candidates
        .iter()
        .filter(|class| {
            name_origin == Some(source_position(class.name_span.start))
                || keyword_origin == Some(source_position(class.span.start))
                || keyword_origin
                    == Some(source_position(
                        crate::emitter::decorators::class_keyword_start(class, &module.source),
                    ))
                || !options.experimental_decorators
                    && !class.decorators.is_empty()
                    && tokens
                        .get(start + 2)
                        .is_some_and(|token| token.is("extends"))
                    && class.extends_span.as_ref().is_some_and(|span| {
                        origin(tokens[start + 3].start) == Some(source_position(span.start))
                            && origin(tokens[start + 3].end) == Some(source_position(span.end))
                    })
        })
        .copied()
        .collect::<Vec<_>>();
    // Standard lowering erases the generated class header's public roles.
    // A surviving member token still identifies its exact original class AST.
    // Match both its spelling and its original UTF-16 provenance; generated
    // constructor names never participate in source identity.
    let body_matches = if matched.is_empty() && !options.experimental_decorators {
        let open = start
            + if tokens
                .get(start + 2)
                .is_some_and(|token| token.is("extends"))
            {
                4
            } else {
                2
            };
        exponentiation::matching_close(tokens, open).map_or_else(Vec::new, |close| {
            candidates
                .iter()
                .copied()
                .filter(|class| {
                    (module.class_expressions.values().any(|expression| {
                        expression.class.span == class.span && !expression.statement
                    }) || !class.decorators.is_empty()
                        && !class.members.is_empty()
                        && class.members.iter().all(|member| {
                            !member.decorators.is_empty()
                                && member.field.as_ref().is_some_and(|field| {
                                    !field.accessor && !field.name.starts_with('#')
                                })
                        }))
                        && crate::emitter::decorators::lowers_class(class, options)
                        && tokens[open + 1..close].iter().any(|token| {
                            let position = origin(token.start);
                            class.body.iter().any(|original| {
                                original.text == token.text
                                    && position == Some(source_position(original.start))
                            })
                        })
                })
                .collect::<Vec<_>>()
        })
    } else {
        Vec::new()
    };
    let selected = if matched.len() == 1 {
        Some(matched[0])
    } else if body_matches.len() == 1 {
        Some(body_matches[0])
    } else if candidates.len() == 1
        && keyword_origin.is_some_and(|point| {
            source_position(candidates[0].span.start) <= point
                && point <= source_position(candidates[0].name_span.end)
        })
    {
        Some(candidates[0])
    } else {
        None
    };
    selected.map(|class| {
        let legacy_decorated = options.experimental_decorators && (
            crate::emitter::legacy_decorators::lowers_class(class)
            || class.members.iter().any(|member| !member.decorators.is_empty()
                || member.method.as_ref().is_some_and(|method| method.parameters.iter().any(|parameter| !parameter.decorators.is_empty())))
        );
        let standard_factory = !options.experimental_decorators
            && runtime_declarations(&module.declarations).into_iter().any(|(declaration, _)| matches!(declaration, Declaration::Class(original) if original.span == class.span))
            && crate::emitter::decorators::lowers_class(class, options);
        let start = crate::syntax::lex(
            "<class-origin>",
            &module.source[class.span.start..class.name_span.start],
        )
        .ok()
        .and_then(|tokens| {
            tokens.into_iter().find(|token| {
                !class.decorators.iter().any(|decorator| {
                    decorator.span.start <= class.span.start + token.start
                        && class.span.start + token.end <= decorator.span.end
                })
            })
        })
        .map_or(class.span.start, |token| class.span.start + token.start);
        let legacy_phase = if !legacy_decorated {
            LegacyPhase::None
        } else if crate::emitter::legacy_decorators::lowers_class(class) {
            // Constructor replacement metadata exists only for an explicit
            // source constructor. An implicit constructor emits no such phase.
            let metadata = options.emit_decorator_metadata && class.members.iter().any(|member| {
                member.constructor.as_ref().is_some_and(|constructor| constructor.body.is_some())
            });
            LegacyPhase::ConstructorReplacement {
                origin: if metadata { class.span.start } else { start },
                metadata,
            }
        } else {
            LegacyPhase::MemberOnly
        };
        Original {
        start,
        legacy_phase,
        legacy_invocation: (options.experimental_decorators
            && options.emit_decorator_metadata
            && class
                .members
                .iter()
                .filter_map(|member| member.constructor.as_ref())
                .any(|constructor| {
                    constructor
                        .parameters
                        .iter()
                        .any(|parameter| !parameter.decorators.is_empty())
                }))
        .then_some(class.span.start),
        end: class.span.end,
        close: class.body_span.end - 1,
        legacy_decorated,
        standard_expression: !options.experimental_decorators
            && crate::emitter::decorators::lowers_class(class, options)
            && module.class_expressions.values().any(|expression| expression.class.span == class.span && !expression.statement),
        heritage: class.extends_span.as_ref().map(|span| (span.start, span.end)),
        factory_start: module.class_expressions.values().find(|expression| expression.class.span == class.span && !crate::emitter::decorators::lowers_class(class, options)).and_then(|expression| {
            crate::lex(&module.id, &module.source).ok().and_then(|tokens| crate::emitter::class_lowering::expressions::names::evaluation_prefix_origin(expression, &tokens))
        }),
        factory_end: module.class_expressions.values().find(|expression| expression.class.span == class.span && !expression.statement && expression.class.members.iter().any(|member|member.field.as_ref().is_some_and(|field|field.is_static&&!field.declared))).and_then(|expression|expression.class.extends_span.as_ref().map(|span|span.end)).or_else(||standard_factory.then_some(class.span.end)),
        standard_factory,
        standard_class_decorated: !options.experimental_decorators && !class.decorators.is_empty(),
        plain_public_static: class.decorators.is_empty()
            && (runtime_declarations(&module.declarations).into_iter().any(|(declaration, _)| matches!(declaration, Declaration::Class(original) if original.span == class.span))
                || module.class_expressions.values().any(|expression| expression.statement && expression.class.span == class.span))
            && class.members.iter().all(|member| member.decorators.is_empty()
                && member.field.as_ref().is_none_or(|field| !field.accessor && !field.name.starts_with('#')))
            && class.members.iter().any(|member| member.field.as_ref().is_some_and(|field| field.is_static && !field.declared)),
        generated_factory: legacy_decorated && options.module_kind == crate::ModuleKind::System
            || !options.experimental_decorators
                && runtime_declarations(&module.declarations).into_iter().any(|(declaration, _)| matches!(declaration, Declaration::Class(original) if original.span == class.span))
                && crate::emitter::decorators::lowers_class(class, options),
        declaration: runtime_declarations(&module.declarations).into_iter().any(|(declaration,_)| matches!(declaration, Declaration::Class(original) if original.span==class.span)),
    }})
}

/// Only the generated factory prefix inherits the original expression context.
/// The constructor and inheritance operands retain their separate source roles.
pub(super) fn expression_factory_prefix(
    emitted: &mut EmittedJavaScript,
    module: &Module,
    start: usize,
    end: usize,
    original: Option<Original>,
) {
    let Some(source) = original.and_then(|original| original.factory_start) else {
        return;
    };
    let position = |offset| {
        let prefix = &emitted.javascript[..offset];
        (
            prefix.bytes().filter(|byte| *byte == b'\n').count(),
            prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
        )
    };
    let (start_key, end_key) = (position(start), position(end));
    emitted.provenance.retain(|segment| {
        let key = (segment.generated_line, segment.generated_column);
        key < start_key || key >= end_key
    });
    point(emitted, module, start, Some(source));
}

/// Moved static state completes after the factory's heritage argument rather
/// than restoring the enclosing source class span at this inner boundary.
pub(super) fn expression_factory_completion(
    emitted: &mut EmittedJavaScript,
    module: &Module,
    end: usize,
    original: Option<Original>,
) {
    if let Some(source) = original.and_then(|original| original.factory_end) {
        point(emitted, module, end, Some(source));
    }
}

/// A decorated expression's generated delimiters finish at the original class
/// endpoint, after its independently mapped factory closing brace.
pub(super) fn expression_factory_invocation(
    emitted: &mut EmittedJavaScript,
    module: &Module,
    start: usize,
    end: usize,
    original: Option<Original>,
) {
    let Some(original) = original.filter(|original| original.standard_expression) else {
        return;
    };
    let position = |offset| {
        let prefix = &emitted.javascript[..offset];
        (
            prefix.bytes().filter(|byte| *byte == b'\n').count(),
            prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
        )
    };
    let (start_key, end_key) = (position(start), position(end));
    emitted.provenance.retain(|segment| {
        let key = (segment.generated_line, segment.generated_column);
        key < start_key || key > end_key
    });
    point(emitted, module, start, Some(original.end));
    point(emitted, module, end, Some(original.end));
}

/// Constructor-replacement metadata keeps its decorator-phase origin for the
/// generated invocation, independently of the non-decorator header.
pub(super) fn legacy_factory_invocation(
    emitted: &mut EmittedJavaScript,
    module: &Module,
    start: usize,
    end: usize,
    original: Option<Original>,
) {
    let Some(source) = original
        .filter(|original| original.heritage.is_none())
        .and_then(|original| {
            if original.generated_factory {
                if let LegacyPhase::ConstructorReplacement {
                    origin,
                    metadata: true,
                } = original.legacy_phase
                {
                    return Some(origin);
                }
            }
            original.legacy_invocation
        })
    else {
        return;
    };
    let position = |offset| {
        let prefix = &emitted.javascript[..offset];
        (
            prefix.bytes().filter(|byte| *byte == b'\n').count(),
            prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
        )
    };
    let (begin, finish) = (position(start), position(end));
    emitted.provenance.retain(|point| {
        let key = (point.generated_line, point.generated_column);
        key < begin || key >= finish
    });
    point(emitted, module, start, Some(source));
}

/// The generated heritage binding transports the original base expression's
/// source role into the inheritance helper, independently of the class header.
pub(super) fn retain_expression_heritage(
    emitted: &mut EmittedJavaScript,
    module: &Module,
    tokens: &[Token],
    start: usize,
    original: Option<Original>,
) {
    let Some((begin, end)) = original
        .filter(|original| original.standard_expression)
        .and_then(|original| original.heritage)
    else {
        return;
    };
    if tokens
        .get(start + 2)
        .is_some_and(|token| token.is("extends"))
    {
        point(emitted, module, tokens[start + 3].start, Some(begin));
        point(emitted, module, tokens[start + 3].end, Some(end));
    }
}

/// Standard declaration factories invoke the completed class after the class's
/// generated grouping. Prior U points at the old erased class close must not
/// interrupt the complete original class-end role of these delimiters.
pub(super) fn standard_factory_invocation(
    emitted: &mut EmittedJavaScript,
    module: &Module,
    start: usize,
    end: usize,
    original: Option<Original>,
) {
    let Some(original) = original.filter(|original| original.standard_factory) else {
        return;
    };
    let location = |offset| {
        let prefix = &emitted.javascript[..offset];
        (
            prefix.bytes().filter(|byte| *byte == b'\n').count(),
            prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
        )
    };
    let (first, last) = (location(start), location(end));
    emitted.provenance.retain(|segment| {
        let position = (segment.generated_line, segment.generated_column);
        position < first || position > last
    });
    point(emitted, module, start, Some(original.end));
    point(emitted, module, end, Some(original.end));
}

/// A member-only legacy declaration finishes its initial binding before the
/// separately relocated publication starts at its own original name range.
pub(super) fn legacy_binding_completion(
    emitted: &mut EmittedJavaScript,
    module: &Module,
    end: usize,
    original: Option<Original>,
) {
    if let Some(original) = original.filter(|original| {
        original.declaration && original.legacy_decorated && !original.generated_factory
    }) {
        point(emitted, module, end, Some(original.end));
    }
}

pub(super) fn completion(
    emitted: &mut EmittedJavaScript,
    module: &Module,
    name: &str,
    generated_offset: usize,
    original: Option<Original>,
) {
    let Some(original) = original else {
        return;
    };
    let (source_end, source_close) = (original.end, original.close);
    let return_start = generated_offset - format!("return {name}").len();
    point(emitted, module, return_start, Some(source_close));
    point(emitted, module, generated_offset, Some(source_end));
    let closing = generated_offset + 2;
    debug_assert_eq!(emitted.javascript.as_bytes().get(closing), Some(&b'}'));
    point(
        emitted,
        module,
        closing,
        (!original.legacy_decorated
            && (original.generated_factory || !original.plain_public_static))
            .then_some(source_close),
    );
}

/// A source declaration completes at its own syntax boundary, before any
/// generated publication. Standard decorator factories have a separate ABI.
pub(super) fn declaration_completion(
    emitted: &mut EmittedJavaScript,
    module: &Module,
    end: usize,
    original: Option<Original>,
) {
    if let Some(original) = original.filter(|original| {
        original.declaration && (!original.generated_factory || original.legacy_decorated)
    }) {
        debug_assert_eq!(emitted.javascript.as_bytes().get(end - 1), Some(&b';'));
        let endpoint = if original.legacy_decorated && original.generated_factory {
            match original.legacy_phase {
                LegacyPhase::ConstructorReplacement { origin, .. } => origin,
                LegacyPhase::MemberOnly => original.start,
                LegacyPhase::None => original.end,
            }
        } else {
            original.end
        };
        point(emitted, module, end, Some(endpoint));
    }
}

/// The synthesized ESM export completes with no original source; the next
/// copied formatting gap must not restore the original class-body endpoint.
pub(super) fn publication_completion(emitted: &mut EmittedJavaScript, module: &Module, end: usize) {
    if emitted.segment_limit.is_some() {
        point(emitted, module, end, None);
    }
}

/// A member-only System class is hoisted into the execute binding phase.
/// Its generated assignment prefix does not inherit the source class header.
pub(super) fn generated_binding(original: Option<Original>) -> bool {
    original.is_some_and(|original| {
        original.generated_factory && matches!(original.legacy_phase, LegacyPhase::MemberOnly)
    })
}

pub(super) fn retain_class_start(
    emitted: &mut EmittedJavaScript,
    module: &Module,
    tokens: &[Token],
    start: usize,
    original: Option<Original>,
) {
    let Some(original) = original.filter(|original| original.declaration) else {
        return;
    };
    let binding = if start >= 2
        && tokens[start - 1].is("=")
        && tokens[start - 2].text == tokens[start + 1].text
    {
        if start >= 3 && matches!(tokens[start - 3].text.as_str(), "var" | "let" | "const") {
            start - 3
        } else {
            start - 2
        }
    } else {
        start
    };
    let location = |offset| {
        let prefix = &emitted.javascript[..offset];
        (
            prefix.bytes().filter(|byte| *byte == b'\n').count(),
            prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
        )
    };
    let begin = location(tokens[binding].start);
    let end = location(tokens[start + 1].start);
    emitted.provenance.retain(|point| {
        let key = (point.generated_line, point.generated_column);
        key < begin || key >= end
    });
    point(
        emitted,
        module,
        tokens[binding].start,
        (!(original.generated_factory && binding != start)).then_some(original.start),
    );
    if binding != start {
        point(emitted, module, tokens[start].start, Some(original.start));
    }
}

pub(super) fn point(
    emitted: &mut EmittedJavaScript,
    module: &Module,
    generated_offset: usize,
    original_offset: Option<usize>,
) {
    let prefix = &emitted.javascript[..generated_offset];
    let generated_line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let generated_column = prefix.rsplit('\n').next().unwrap().encode_utf16().count();
    let original = &module.source[..original_offset.unwrap_or(0)];
    let source_line = original.bytes().filter(|byte| *byte == b'\n').count();
    let source_column = original.rsplit('\n').next().unwrap().encode_utf16().count();
    let key = (generated_line, generated_column);
    let index = emitted
        .provenance
        .partition_point(|segment| (segment.generated_line, segment.generated_column) < key);
    let value = ProvenanceSegment {
        generated_line,
        generated_column,
        source_line,
        source_column,
        mapped: original_offset.is_some(),
        name_index: None,
    };
    if emitted
        .provenance
        .get(index)
        .is_some_and(|segment| (segment.generated_line, segment.generated_column) == key)
    {
        emitted.provenance[index] = value;
    } else if emitted
        .segment_limit
        .is_some_and(|limit| emitted.provenance.len() >= limit)
    {
        emitted.overflowed = true;
    } else {
        emitted.provenance.insert(index, value);
    }
}

/// Record a checked original super call's whole-expression endpoint before
/// replacing its property/call transport. Empty calls otherwise retain only
/// the opening-parenthesis origin, which must not leak into the next operator.
pub(super) fn retain_super_calls(
    emitted: &mut EmittedJavaScript,
    module: &Module,
    tokens: &[Token],
    start: usize,
    close: usize,
    original: Option<Original>,
) {
    if emitted.segment_limit.is_none() {
        return;
    }
    let original = runtime_declarations(&module.declarations)
        .into_iter()
        .filter_map(|(declaration, _)| match declaration {
            Declaration::Class(class) => Some(class),
            _ => None,
        })
        .chain(
            module
                .class_expressions
                .values()
                .map(|expression| &expression.class),
        )
        .find(|class| {
            original.is_some_and(|original| {
                original.end == class.span.end && original.close == class.body_span.end - 1
            })
        });
    let Some(class) = original else {
        return;
    };
    for member in &class.members {
        if !matches!(
            member.kind,
            crate::parser::ClassMemberKind::Constructor
                | crate::parser::ClassMemberKind::Method
                | crate::parser::ClassMemberKind::Accessor
        ) {
            continue;
        }
        let original_close = class.body[member.token_start..member.token_end]
            .iter()
            .rfind(|token| token.is("}"));
        let Some(original_close) = original_close else {
            continue;
        };
        let prefix = &module.source[..original_close.start];
        let expected = (
            prefix.bytes().filter(|byte| *byte == b'\n').count(),
            prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
        );
        for token in &tokens[start..close] {
            if !token.is("}") {
                continue;
            }
            let prefix = &emitted.javascript[..token.start];
            let key = (
                prefix.bytes().filter(|byte| *byte == b'\n').count(),
                prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
            );
            let index = emitted
                .provenance
                .partition_point(|point| (point.generated_line, point.generated_column) <= key);
            let matches = index
                .checked_sub(1)
                .and_then(|index| emitted.provenance.get(index))
                .is_some_and(|point| {
                    point.mapped
                        && point.generated_line == key.0
                        && (point.source_line, point.source_column) == expected
                });
            if matches {
                point(emitted, module, token.end, Some(original_close.end));
            }
        }
    }
    for index in start..close {
        if !tokens[index].is("super")
            || !tokens
                .get(index + 1)
                .is_some_and(|token| token.is(".") || token.is("["))
        {
            continue;
        }
        let prefix = &emitted.javascript[..tokens[index].start];
        let key = (
            prefix.bytes().filter(|byte| *byte == b'\n').count(),
            prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
        );
        let mapping = emitted
            .provenance
            .iter()
            .rev()
            .find(|mapping| (mapping.generated_line, mapping.generated_column) <= key)
            .filter(|mapping| mapping.generated_line == key.0 && mapping.mapped);
        let Some(mapping) = mapping else {
            continue;
        };
        let original = class.body.iter().position(|token| {
            if !token.is("super") {
                return false;
            }
            let source = &module.source[..token.start];
            source.bytes().filter(|byte| *byte == b'\n').count() == mapping.source_line
                && source.rsplit('\n').next().unwrap().encode_utf16().count()
                    == mapping.source_column
        });
        let Some(original) = original else {
            continue;
        };
        let (Some(generated_end), Some(original_end)) = (
            super_call_end(tokens, index),
            super_call_end(&class.body, original),
        ) else {
            continue;
        };
        point(
            emitted,
            module,
            tokens[generated_end].end,
            Some(class.body[original_end].end),
        );
    }
}

fn super_call_end(tokens: &[Token], start: usize) -> Option<usize> {
    let next = start + 1;
    let open = if tokens.get(next)?.is(".") {
        start + 3
    } else if tokens[next].is("[") {
        exponentiation::matching_close(tokens, next)? + 1
    } else {
        next
    };
    tokens
        .get(open)?
        .is("(")
        .then(|| exponentiation::matching_close(tokens, open))?
}

/// A class decorator's initial constructor has no original constructor header.
/// Its independently retained class close still ends at the original class.
pub(super) fn implicit_header_is_generated(original: Option<Original>) -> bool {
    original.is_some_and(|original| original.standard_factory && original.standard_class_decorated)
}
