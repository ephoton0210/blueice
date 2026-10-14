// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Checked heap-payload accounting for a retained BlueJS syntax tree.
//!
//! Vector capacities and string buffers are counted at their owning nodes.
//! Box allocations are counted once at their parent. SourceText values from
//! one parse share an Arc, whose text payload is counted only once. Allocator
//! bookkeeping and spare capacity inside num-bigint are outside this metric.

use super::*;
use std::collections::HashSet;

#[derive(Default)]
struct CountContext {
    source_texts: HashSet<usize>,
}

trait HeapPayload {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize>;
}

/// Account for a child payload without conflating allocation size with the
/// sum of separately owned allocations. Both unavailable children and real
/// usize overflow retain the public None result.
fn add_payload(total: usize, child: Option<usize>) -> Option<usize> {
    total.checked_add(child?)
}

macro_rules! sum {
    ($($part:expr),* $(,)?) => {{
        let mut total = 0usize;
        $(total = add_payload(total, $part)?;)*
        Some(total)
    }};
}

impl HeapPayload for String {
    fn heap_payload(&self, _: &mut CountContext) -> Option<usize> {
        Some(self.capacity())
    }
}

impl HeapPayload for JsString {
    fn heap_payload(&self, _: &mut CountContext) -> Option<usize> {
        self.owned_heap_payload_bytes()
    }
}

impl<T: HeapPayload> HeapPayload for Option<T> {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        self.as_ref()
            .map_or(Some(0), |value| value.heap_payload(context))
    }
}

impl<T: HeapPayload> HeapPayload for Box<T> {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        add_payload(std::mem::size_of::<T>(), (**self).heap_payload(context))
    }
}

impl<T: HeapPayload> HeapPayload for Vec<T> {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        // Vec's allocation layout bounds this product by isize::MAX. For
        // zero-sized elements the product is zero, including capacity MAX.
        let mut bytes = self
            .capacity()
            .checked_mul(std::mem::size_of::<T>())
            .expect("an allocated Vec has a representable byte capacity");
        for item in self {
            bytes = add_payload(bytes, item.heap_payload(context))?;
        }
        Some(bytes)
    }
}

impl HeapPayload for BigInt {
    fn heap_payload(&self, _: &mut CountContext) -> Option<usize> {
        // The pinned num-bigint digit iterator reports the exact logical
        // u32 limb count, without copying. Its byte length cannot exceed
        // the allocated native-digit buffer, including zero and sign.
        Some(
            self.iter_u32_digits()
                .len()
                .checked_mul(std::mem::size_of::<u32>())
                .expect("logical BigInt limbs fit their allocated digit buffer"),
        )
    }
}

impl HeapPayload for SourceText {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        let Some(text) = &self.text else {
            return Some(0);
        };
        Some(if context.source_texts.insert(text.as_ptr() as usize) {
            text.len()
        } else {
            0
        })
    }
}

impl Program {
    pub fn owned_heap_payload_bytes(&self) -> Option<usize> {
        self.body.heap_payload(&mut CountContext::default())
    }
}

impl Module {
    pub fn owned_heap_payload_bytes(&self) -> Option<usize> {
        let mut context = CountContext::default();
        sum!(
            self.body.heap_payload(&mut context),
            self.imports.heap_payload(&mut context),
            self.exports.heap_payload(&mut context),
            self.requests.heap_payload(&mut context),
        )
    }
}

impl HeapPayload for RequestedModule {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        self.specifier.heap_payload(context)
    }
}

impl HeapPayload for ImportName {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        match self {
            Self::Named(name) => name.heap_payload(context),
            Self::Namespace | Self::DeferredNamespace | Self::Source => Some(0),
        }
    }
}

impl HeapPayload for ImportEntry {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        sum!(
            self.module_request.heap_payload(context),
            self.import_name.heap_payload(context),
            self.local_name.heap_payload(context),
        )
    }
}

impl HeapPayload for ExportEntry {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        match self {
            Self::Local {
                export_name,
                local_name,
            } => sum!(
                export_name.heap_payload(context),
                local_name.heap_payload(context)
            ),
            Self::Indirect {
                export_name,
                module_request,
                import_name,
                ..
            } => sum!(
                export_name.heap_payload(context),
                module_request.heap_payload(context),
                import_name.heap_payload(context),
            ),
            Self::Star { module_request, .. } => module_request.heap_payload(context),
            Self::Namespace {
                export_name,
                module_request,
                ..
            } => sum!(
                export_name.heap_payload(context),
                module_request.heap_payload(context),
            ),
        }
    }
}

impl HeapPayload for Function {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        sum!(
            self.name.heap_payload(context),
            self.params.heap_payload(context),
            self.body.heap_payload(context),
            self.source_text.heap_payload(context),
        )
    }
}

impl HeapPayload for Class {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        sum!(
            self.name.heap_payload(context),
            self.extends.heap_payload(context),
            self.elements.heap_payload(context),
            self.decorators.heap_payload(context),
            self.source_text.heap_payload(context),
        )
    }
}

impl HeapPayload for ClassElement {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        match self {
            Self::Method {
                key,
                function,
                decorators,
                ..
            }
            | Self::Accessor {
                key,
                function,
                decorators,
                ..
            } => sum!(
                key.heap_payload(context),
                function.heap_payload(context),
                decorators.heap_payload(context),
            ),
            Self::Field {
                key,
                initializer,
                decorators,
                ..
            } => sum!(
                key.heap_payload(context),
                initializer.heap_payload(context),
                decorators.heap_payload(context),
            ),
            Self::StaticBlock(body) => body.heap_payload(context),
        }
    }
}

impl HeapPayload for Param {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        sum!(
            self.pattern.heap_payload(context),
            self.default.heap_payload(context),
        )
    }
}

impl HeapPayload for Pattern {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        match self {
            Self::Identifier(name) => name.heap_payload(context),
            Self::Array(elements) => elements.heap_payload(context),
            Self::Object(properties) => properties.heap_payload(context),
        }
    }
}

impl HeapPayload for AssignmentPattern {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        match self {
            Self::Target(target) => target.heap_payload(context),
            Self::Array(elements) => elements.heap_payload(context),
            Self::Object(properties) => properties.heap_payload(context),
        }
    }
}

impl HeapPayload for AssignmentPatternElement {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        sum!(
            self.pattern.heap_payload(context),
            self.default.heap_payload(context),
        )
    }
}

impl HeapPayload for AssignmentPatternProp {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        match self {
            Self::KeyValue {
                key,
                value,
                default,
            } => sum!(
                key.heap_payload(context),
                value.heap_payload(context),
                default.heap_payload(context),
            ),
            Self::Rest(value) => value.heap_payload(context),
        }
    }
}

impl HeapPayload for ArrayPatternElement {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        sum!(
            self.pattern.heap_payload(context),
            self.default.heap_payload(context),
        )
    }
}

impl HeapPayload for ObjectPatternProp {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        match self {
            Self::KeyValue {
                key,
                value,
                default,
            } => sum!(
                key.heap_payload(context),
                value.heap_payload(context),
                default.heap_payload(context),
            ),
            Self::Rest(value) => value.heap_payload(context),
        }
    }
}

impl HeapPayload for PropertyKey {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        match self {
            Self::Identifier(name) => name.heap_payload(context),
            Self::String(value) => value.heap_payload(context),
            Self::Computed(expression) => expression.heap_payload(context),
            Self::Number(_) => Some(0),
        }
    }
}

impl HeapPayload for VarDeclarator {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        sum!(
            self.pattern.heap_payload(context),
            self.init.heap_payload(context),
        )
    }
}

impl HeapPayload for SwitchCase {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        sum!(
            self.test.heap_payload(context),
            self.consequent.heap_payload(context),
        )
    }
}

impl HeapPayload for CatchClause {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        sum!(
            self.param.heap_payload(context),
            self.body.heap_payload(context),
        )
    }
}

impl HeapPayload for ForHead {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        match self {
            Self::Decl(_, pattern) => pattern.heap_payload(context),
            Self::AnnexBVarInit(pattern, expression) => sum!(
                pattern.heap_payload(context),
                expression.heap_payload(context),
            ),
            Self::Assignment(pattern) => pattern.heap_payload(context),
            Self::Expr(expression) => expression.heap_payload(context),
        }
    }
}

impl HeapPayload for ForInit {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        match self {
            Self::VarDecl(_, declarations) => declarations.heap_payload(context),
            Self::Expr(expression) => expression.heap_payload(context),
        }
    }
}

impl HeapPayload for Stmt {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        match self {
            Self::Empty => Some(0),
            Self::Expr(expression) | Self::Throw(expression) => expression.heap_payload(context),
            Self::Block(body) => body.heap_payload(context),
            Self::VarDecl(_, declarations) => declarations.heap_payload(context),
            Self::If {
                test,
                consequent,
                alternate,
            } => sum!(
                test.heap_payload(context),
                consequent.heap_payload(context),
                alternate.heap_payload(context),
            ),
            Self::For {
                init,
                test,
                update,
                body,
            } => sum!(
                init.heap_payload(context),
                test.heap_payload(context),
                update.heap_payload(context),
                body.heap_payload(context),
            ),
            Self::ForIn { left, right, body }
            | Self::ForOf {
                left, right, body, ..
            } => sum!(
                left.heap_payload(context),
                right.heap_payload(context),
                body.heap_payload(context),
            ),
            Self::While { test, body } | Self::DoWhile { body, test } => {
                sum!(test.heap_payload(context), body.heap_payload(context),)
            }
            Self::Switch {
                discriminant,
                cases,
            } => sum!(
                discriminant.heap_payload(context),
                cases.heap_payload(context),
            ),
            Self::Labelled { label, item } => {
                sum!(label.heap_payload(context), item.heap_payload(context))
            }
            Self::Break(label) | Self::Continue(label) => label.heap_payload(context),
            Self::Return(expression) => expression.heap_payload(context),
            Self::Try {
                block,
                handler,
                finalizer,
            } => sum!(
                block.heap_payload(context),
                handler.heap_payload(context),
                finalizer.heap_payload(context),
            ),
            Self::With { object, body } => {
                sum!(object.heap_payload(context), body.heap_payload(context),)
            }
            Self::FunctionDecl(function) => function.heap_payload(context),
            Self::ModuleDefaultFunction { function, binding } => sum!(
                function.heap_payload(context),
                binding.heap_payload(context),
            ),
            Self::ClassDecl(class) => class.heap_payload(context),
            Self::ClassField(field) => field.heap_payload(context),
            Self::ClassPrivateBrand(name) | Self::ClassExtraInitializers(name) => {
                name.heap_payload(context)
            }
            Self::ClassDecoratedField { field, record } => {
                sum!(field.heap_payload(context), record.heap_payload(context),)
            }
        }
    }
}

impl HeapPayload for ArrayElement {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        match self {
            Self::Normal(expression) | Self::Spread(expression) => expression.heap_payload(context),
        }
    }
}

impl HeapPayload for ObjectProp {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        match self {
            Self::KeyValue { key, value, .. } => {
                sum!(key.heap_payload(context), value.heap_payload(context),)
            }
            Self::Spread(expression) => expression.heap_payload(context),
            Self::Method { key, function } | Self::Accessor { key, function, .. } => {
                sum!(key.heap_payload(context), function.heap_payload(context),)
            }
        }
    }
}

impl HeapPayload for Argument {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        match self {
            Self::Normal(expression) | Self::Spread(expression) => expression.heap_payload(context),
        }
    }
}

impl HeapPayload for ArrowBody {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        match self {
            Self::Expr(expression) => expression.heap_payload(context),
            Self::Block(body) => body.heap_payload(context),
        }
    }
}

impl HeapPayload for Expr {
    fn heap_payload(&self, context: &mut CountContext) -> Option<usize> {
        match self {
            Self::Number(_)
            | Self::Bool(_)
            | Self::Null
            | Self::This
            | Self::Super
            | Self::NewTarget
            | Self::ImportMeta => Some(0),
            Self::BigInt(value) => value.heap_payload(context),
            Self::String(value) => value.heap_payload(context),
            Self::Identifier(name) => name.heap_payload(context),
            Self::Parenthesized(expression)
            | Self::Await(expression)
            | Self::Unary {
                arg: expression, ..
            }
            | Self::Update {
                arg: expression, ..
            } => expression.heap_payload(context),
            Self::Template {
                quasis,
                expressions,
            } => sum!(
                quasis.heap_payload(context),
                expressions.heap_payload(context),
            ),
            Self::TaggedTemplate {
                tag,
                raw,
                cooked,
                expressions,
            } => sum!(
                tag.heap_payload(context),
                raw.heap_payload(context),
                cooked.heap_payload(context),
                expressions.heap_payload(context),
            ),
            Self::RegExp { pattern, flags } => {
                sum!(pattern.heap_payload(context), flags.heap_payload(context),)
            }
            Self::Array(elements) => elements.heap_payload(context),
            Self::Object(properties) => properties.heap_payload(context),
            Self::Function(function) => function.heap_payload(context),
            Self::Class(class) => class.heap_payload(context),
            Self::Yield { value, .. } => value.heap_payload(context),
            Self::DynamicImport {
                specifier, options, ..
            } => sum!(
                specifier.heap_payload(context),
                options.heap_payload(context),
            ),
            Self::Arrow {
                params,
                body,
                source_text,
                ..
            } => sum!(
                params.heap_payload(context),
                body.heap_payload(context),
                source_text.heap_payload(context),
            ),
            Self::Binary { left, right, .. }
            | Self::Logical { left, right, .. }
            | Self::Assign {
                target: left,
                value: right,
                ..
            }
            | Self::Member {
                object: left,
                property: right,
                ..
            }
            | Self::OptionalMember {
                object: left,
                property: right,
                ..
            } => sum!(left.heap_payload(context), right.heap_payload(context)),
            Self::Sequence(expressions) => expressions.heap_payload(context),
            Self::DestructureAssign { pattern, value } => {
                sum!(pattern.heap_payload(context), value.heap_payload(context),)
            }
            Self::Conditional {
                test,
                consequent,
                alternate,
            } => sum!(
                test.heap_payload(context),
                consequent.heap_payload(context),
                alternate.heap_payload(context),
            ),
            Self::Call { callee, args }
            | Self::OptionalCall { callee, args }
            | Self::New { callee, args } => {
                sum!(callee.heap_payload(context), args.heap_payload(context),)
            }
            Self::PrivateIn { name, object } => {
                sum!(name.heap_payload(context), object.heap_payload(context),)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_syntax_and_shared_function_source_have_distinct_payloads() {
        let small = crate::parse_module("export const value = 1;").unwrap();
        let nested = crate::parse_module(
            "export function outer(input) { const nested = (value) => `${value}-${input}`; return nested(input); }",
        )
        .unwrap();
        assert!(
            nested.owned_heap_payload_bytes().unwrap() > small.owned_heap_payload_bytes().unwrap()
        );

        let shared = Arc::<str>::from("function first() {} function second() {}");
        let one = SourceText::range(&shared, 0, 19);
        let two = SourceText::range(&shared, 20, shared.len());
        let mut context = CountContext::default();
        assert_eq!(one.heap_payload(&mut context), Some(shared.len()));
        assert_eq!(two.heap_payload(&mut context), Some(0));
    }

    #[test]
    fn a_source_text_range_with_an_unrepresentable_offset_has_no_text_payload() {
        // `SourceText::range` yields its default, textless value whenever
        // either offset doesn't fit in `u32` -- unreachable from a real
        // parse (no source file is anywhere near 4 GiB), but a real,
        // directly-checkable boundary of the crate's own `pub(crate)` API,
        // not a corrupted invariant.
        let text = Arc::<str>::from("short");
        let unrepresentable = SourceText::range(&text, usize::MAX, usize::MAX);
        let mut context = CountContext::default();
        assert_eq!(unrepresentable.heap_payload(&mut context), Some(0));
    }

    #[test]
    fn every_module_import_and_export_shape_has_a_computable_heap_payload() {
        let source = r#"
            import './side-effect.js';
            import def, { a as b, c } from './named.js';
            import * as ns from './ns.js';
            import defer * as dns from './deferred.js';
            import source src from './src.js';

            export { def, c };
            export { a as aa } from './named.js';
            export * from './star.js';
            export * as starNs from './starns.js';
            export default function namedDefault() { return 1; }
        "#;
        let module = crate::parse_module(source).unwrap();
        assert!(module.owned_heap_payload_bytes().unwrap() > 0);
    }

    #[test]
    fn every_reachable_statement_and_expression_shape_has_a_computable_heap_payload() {
        let source = r#"
            function dec(value, _context) { return value; }
            class Base {}

            export function kitchen(a, [x, y = 1, ...zs], { p, q: qq, ...rest } = {}, ...restArgs) {
                let num = 1;
                let big = 1n;
                let str = "s";
                let tpl = `t${a}`;
                let tag = String.raw`r${a}`;
                let re = /x/g;
                let spreadSrc = [1, 2];
                let arr = [1, ...spreadSrc];
                let computed = "k";
                let spreadObj = { k2: 2 };
                let obj = { k: 1, ...spreadObj, [computed]: 2, "str-key": 3, 4: "num-key", m() { return 1; }, get g() { return 1; }, set s(v) {} };
                let fn = function named() {};
                let arrow1 = (v) => v;
                let arrow2 = (v) => { return v; };
                let computed2 = "dyn";
                let cls = class Named extends Base {
                    static field = 1;
                    #priv = 2;
                    static {
                        void 0;
                    }
                    @dec method() {
                        return this.#priv;
                    }
                    get accessorGet() { return 1; }
                    set accessorSet(v) {}
                    accessor autoAcc = 3;
                    [computed2]() { return 1; }
                    hasPriv(o) { return #priv in o; }
                };

                label: for (let i = 0; i < 1; i++) {
                    if (i === 0) {
                        continue label;
                    } else {
                        break label;
                    }
                }
                for (const k in obj) {
                }
                for (const v of arr) {
                }
                while (false) {
                }
                do {
                } while (false);
                switch (num) {
                    case 1:
                        break;
                    default:
                        break;
                }
                try {
                    throw new Error("e");
                } catch (e) {
                } finally {
                }
                try {
                } catch {
                }
                block: {
                    let inner = 1;
                }
                ;

                const seq = (1, 2, 3);
                const cond = true ? 1 : 2;
                // `Expr::Parenthesized` is only retained when the group is
                // itself an assignment target (or wraps an optional chain);
                // an ordinary grouped read like `(a)` alone is unwrapped
                // back to the inner expression at parse time.
                (a) = 5;
                const logical = a && num;
                let plain = 1;
                plain = 2;
                for (plain = 0; plain < 1; plain++) {
                }
                for (plain in obj) {
                }
                [x, y] = [y, x];
                let restAssignTarget;
                ({ p: rest.p, ...restAssignTarget } = obj);
                const optChain = ({}).missing?.member;
                const optCall = fn.bind?.();
                const dynImport = () => import("./dynamic.js");
                const metaUrl = () => import.meta;
                const target = function () {
                    return new.target;
                };
                const call = new Base();
                const alsoCall = fn(a, ...restArgs);

                return new cls().hasPriv(new cls());
            }

            export function* gen() {
                yield 1;
            }

            // `export async function` (non-default) is not accepted by
            // this parser -- only `export default async function ...` is
            // -- so this is declared plainly and exported by name instead.
            async function asyncFn() {
                await Promise.resolve();
            }
            export { asyncFn };
        "#;
        let module = crate::parse_module(source).unwrap();
        assert!(module.owned_heap_payload_bytes().unwrap() > 0);
    }

    #[test]
    fn sloppy_annex_b_for_head_and_with_shapes_have_a_computable_heap_payload() {
        // `with`, the Annex B `var x = init in ...` for-in initializer, and
        // a call-expression for-in/for-of target are all sloppy-mode-only
        // relaxations -- unreachable from a module (always strict), so this
        // is a separate classic script with no directive prologue.
        let source = r#"
            function sink() { return {}; }
            with (sink()) {
            }
            for (var v = 1 in { a: 1 }) {
            }
            for (sink() in { b: 1 }) {
            }
        "#;
        let program = crate::parse(source).unwrap();
        assert!(program.owned_heap_payload_bytes().unwrap() > 0);
    }
}

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/payload_accounting_contracts.rs"]
mod accounting_contracts;
