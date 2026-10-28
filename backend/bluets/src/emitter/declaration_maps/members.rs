// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class signatures and synthesized parameter properties retain source owners.
use super::*;
use crate::parser::ClassMemberKind;

impl Mapper<'_> {
    pub(super) fn class(&mut self, g: &ClassDeclaration, s: &ClassDeclaration) {
        self.range(&g.span, &s.span);
        self.range(&g.name_span, &s.name_span);
        self.type_parameters(&g.type_parameters, &s.type_parameters);
        if let Some((g, s)) = g
            .extends_type_span
            .as_ref()
            .zip(s.extends_type_span.as_ref())
        {
            self.ty(&self.generated(g), &self.source(s));
        }
        let mut used = BTreeSet::new();
        for gm in &g.members {
            let found = s.members.iter().enumerate().find(|(index, sm)| {
                !used.contains(index)
                    && gm.kind == sm.kind
                    && gm.name == sm.name
                    && gm.accessor.as_ref().map(|a| a.getter)
                        == sm.accessor.as_ref().map(|a| a.getter)
            });
            if let Some((index, sm)) = found {
                used.insert(index);
                self.member(gm, sm);
            } else if gm.field.is_some() {
                for constructor in s.members.iter().filter_map(|m| m.constructor.as_ref()) {
                    if let Some(parameter) = constructor
                        .parameter_properties
                        .iter()
                        .filter_map(|p| constructor.parameters.get(p.parameter_index))
                        .find(|p| Some(p.name.as_str()) == gm.name.as_deref())
                    {
                        let gt = self.generated(&gm.span);
                        let st = self.source(&parameter.span);
                        self.named(&gt, &st, &parameter.name);
                        self.ty(annotation(&gt), annotation(&st));
                    }
                }
            }
        }
    }
    fn member(&mut self, g: &ClassMemberShell, s: &ClassMemberShell) {
        let gt = self.generated(&g.span);
        let st = self.source(&s.span);
        if g.kind == ClassMemberKind::Constructor {
            if let Some((g, s)) = g.constructor.as_ref().zip(s.constructor.as_ref()) {
                self.parameters(&g.parameters, &s.parameters);
            }
            return;
        }
        self.point(g.span.start, s.span.start);
        let gn = g.key.first().map_or(g.span.start, |t| t.start);
        let sn = s.key.first().map_or(s.span.start, |t| t.start);
        self.modifiers(
            &gt.iter()
                .filter(|t| t.start < gn)
                .cloned()
                .collect::<Vec<_>>(),
            &st.iter()
                .filter(|t| t.start < sn)
                .cloned()
                .collect::<Vec<_>>(),
        );
        for (g, s) in g.key.iter().zip(&s.key) {
            self.pair(g, s);
        }
        if let Some((gf, sf)) = g.field.as_ref().zip(s.field.as_ref()) {
            if let Some((g, s)) = gf.annotation_span.as_ref().zip(sf.annotation_span.as_ref()) {
                self.ty(&self.generated(g), &self.source(s));
            }
            self.point(g.span.end, s.span.end);
        }
        if let Some((gm, sm)) = g.method.as_ref().zip(s.method.as_ref()) {
            self.type_parameters(&gm.type_parameters, &sm.type_parameters);
            self.parameters(&gm.parameters, &sm.parameters);
            if let Some((g, s)) = gm
                .return_type_span
                .as_ref()
                .zip(sm.return_type_span.as_ref())
            {
                self.ty(&self.generated(g), &self.source(s));
            }
        }
        if let Some((ga, sa)) = g.accessor.as_ref().zip(s.accessor.as_ref()) {
            self.parameters(&ga.parameters, &sa.parameters);
            if let Some((g, s)) = ga
                .return_type_span
                .as_ref()
                .zip(sa.return_type_span.as_ref())
            {
                self.ty(&self.generated(g), &self.source(s));
            }
            self.point(g.span.end, s.span.end);
        }
    }
}
