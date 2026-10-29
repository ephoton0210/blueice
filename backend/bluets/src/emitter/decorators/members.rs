// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Member identities, fresh helper names and descriptor plans.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Method,
    Getter,
    Setter,
    Field,
    Accessor,
}

impl Kind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Method => "method",
            Self::Getter => "getter",
            Self::Setter => "setter",
            Self::Field => "field",
            Self::Accessor => "accessor",
        }
    }

    pub(super) fn is_field(self) -> bool {
        matches!(self, Self::Field)
    }
}

/// One member of the class, as the lowering sees it.
pub(super) struct Plan<'a> {
    pub(super) shell: &'a ClassMemberShell,
    pub(super) kind: Kind,
    pub(super) is_static: bool,
    pub(super) private: bool,
    /// `x` or `#p`.
    pub(super) name: String,
    /// The helper variable's stem: `_static_get_x`, `_private_p`.
    pub(super) var: String,
    /// Captured property key, evaluated once at class definition.
    pub(super) runtime_key: Option<String>,
    /// Literal auto-accessors need an identifier independent of their property text.
    pub(super) literal_storage: Option<String>,
}

impl Plan<'_> {
    pub(super) fn decorated(&self) -> bool {
        !self.shell.decorators.is_empty()
    }

    pub(super) fn decorators_var(&self) -> String {
        format!("{}_decorators", self.var)
    }

    pub(super) fn initializers_var(&self) -> String {
        format!("{}_initializers", self.var)
    }

    pub(super) fn extra_initializers_var(&self) -> String {
        format!("{}_extraInitializers", self.var)
    }

    /// The private name of an auto-accessor's backing field.
    pub(super) fn storage(&self) -> String {
        self.literal_storage
            .clone()
            .unwrap_or_else(|| format!("#{}_accessor_storage", self.name.trim_start_matches('#')))
    }

    pub(super) fn literal(&self) -> bool {
        self.shell
            .key
            .first()
            .is_some_and(|key| matches!(key.kind, TokenKind::String | TokenKind::Number))
    }
}

pub(super) struct Names {
    pub(super) used: BTreeSet<String>,
}

impl Names {
    pub(super) fn unique(&mut self, wanted: &str) -> String {
        let mut candidate = wanted.to_string();
        let mut count = 0;
        while self.used.contains(&candidate) {
            count += 1;
            candidate = format!("{wanted}_{count}");
        }
        self.used.insert(candidate.clone());
        candidate
    }
}

impl Lowerer<'_, '_> {
    pub(super) fn plans<'c>(
        &mut self,
        class: &'c ClassDeclaration,
    ) -> Result<Vec<Plan<'c>>, Diagnostic> {
        let mut plans = Vec::new();
        for shell in &class.members {
            let literal = shell
                .key
                .first()
                .is_some_and(|key| matches!(key.kind, TokenKind::String | TokenKind::Number));
            let (kind, is_static, name) = match shell.kind {
                ClassMemberKind::Method => {
                    let Some(method) = &shell.method else {
                        if shell.decorators.is_empty() {
                            continue;
                        }
                        return Err(unsupported(
                            &shell.span,
                            "this decorated method shape is not lowered",
                        ));
                    };
                    if method.body.is_none() {
                        if let Some(decorator) = shell.decorators.first() {
                            return Err(unsupported(
                                &decorator.span,
                                "a decorator can only decorate a method implementation, not an overload",
                            ));
                        }
                        continue;
                    }
                    (Kind::Method, method.is_static, method.name.clone())
                }
                ClassMemberKind::Accessor => {
                    let Some(accessor) = &shell.accessor else {
                        continue;
                    };
                    (
                        if accessor.getter {
                            Kind::Getter
                        } else {
                            Kind::Setter
                        },
                        accessor.is_static,
                        accessor.name.clone(),
                    )
                }
                ClassMemberKind::Field => {
                    let Some(field) = &shell.field else {
                        continue;
                    };
                    (
                        if field.accessor {
                            Kind::Accessor
                        } else {
                            Kind::Field
                        },
                        field.is_static,
                        field.name.clone(),
                    )
                }
                ClassMemberKind::Constructor
                | ClassMemberKind::StaticBlock
                | ClassMemberKind::IndexSignature => {
                    if let Some(decorator) = shell.decorators.first() {
                        return Err(unsupported(
                            &decorator.span,
                            "decorators are not valid here",
                        ));
                    }
                    continue;
                }
                ClassMemberKind::Opaque => {
                    if let Some(decorator) = shell.decorators.first() {
                        return Err(unsupported(
                            &decorator.span,
                            "this decorated member shape (computed or literal name, or a form the class parser does not structure) is not lowered",
                        ));
                    }
                    continue;
                }
            };
            let private = name.starts_with('#');
            let mut stem = String::from("_");
            if is_static {
                stem.push_str("static_");
            }
            match kind {
                Kind::Getter => stem.push_str("get_"),
                Kind::Setter => stem.push_str("set_"),
                _ => {}
            }
            if private {
                stem.push_str("private_");
            }
            let computed = shell.key.first().is_some_and(|key| key.is("["));
            stem.push_str(if computed || literal {
                "member"
            } else {
                name.trim_start_matches('#')
            });
            // Only decorated members get helper variables; keep their stems unique.
            let var = if shell.decorators.is_empty() {
                stem
            } else {
                self.names.unique(&stem)
            };
            let runtime_key = computed.then(|| self.names.unique("_computedKey"));
            let literal_storage = (literal && kind == Kind::Accessor)
                .then(|| format!("#{}", self.names.unique("_accessor_storage")));
            plans.push(Plan {
                literal_storage,
                runtime_key,
                shell,
                kind,
                is_static,
                private,
                name,
                var,
            });
        }
        Ok(plans)
    }
}
