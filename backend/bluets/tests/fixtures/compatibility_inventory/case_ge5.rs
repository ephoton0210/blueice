// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finite G-E5 source domains; repeated builds and Native/Blue executions are not cases.
use super::case_sources::{Case, Comparison};
use super::inventory::Suite;
use super::snapshot::{Result, Snapshot};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use syn::visit::{self, Visit};
use syn::{Expr, ItemFn, Lit};

const INPUT: &str = "backend/bluets/tests/fixtures/ge5_source_maps/cases.json";
const BLUE_MATRIX_TEST: &str =
    "all_frozen_declarations_match_native_and_javascript_maps_validate_twice";

pub fn validate_measured_tests(harness: &str, measured: &BTreeSet<String>) -> Result<()> {
    if harness.ends_with("/ge5_source_maps.rs") && !measured.contains(BLUE_MATRIX_TEST) {
        return Err(
            "G-E5 full matrix comparison requires the measured Blue declaration/bounds test".into(),
        );
    }
    Ok(())
}

pub fn primary(snapshot: &Snapshot, suite: &Suite) -> Result<Vec<Case>> {
    let input: Value = serde_json::from_str(snapshot.source(INPUT)?)?;
    if let Some(cases) = rules::support(snapshot, suite, &input)? {
        return Ok(cases);
    }
    let root_source = snapshot.source(&suite.harness)?;
    let root_ignored;
    let ignored = if suite.harness.ends_with("/ge5_source_maps.rs") {
        let derived = module_ignored(&suite.harness, root_source, &|path| {
            Ok(snapshot.source(path)?.to_string())
        })?;
        if derived != suite.ignored_tests.iter().cloned().collect()
            || derived.len() != suite.ignored_tests.len()
        {
            return Err("partial/unknown G-E5 module ignored-test selection".into());
        }
        // The ordinary measured Blue declaration/bounds test covers the full
        // input. Module role tests neither add rows nor turn replay into JS proof.
        root_ignored = super::rust_facts::parse(root_source)?.ignored_tests;
        &root_ignored
    } else {
        &suite.ignored_tests
    };
    let parents = RefCell::new(BTreeMap::<String, Rc<Value>>::new());
    select(&suite.harness, root_source, ignored, &input, &|row| {
        resolve_with(&input, row, &|suite| {
            let cached = parents.borrow().get(suite).cloned();
            if let Some(parent) = cached {
                return Ok(parent);
            }
            let parent: Rc<Value> = Rc::new(serde_json::from_str(
                snapshot.source(&format!("backend/bluets/tests/fixtures/{suite}/cases.json"))?,
            )?);
            parents.borrow_mut().insert(suite.into(), parent.clone());
            Ok(parent)
        })
    })
}

fn module_ignored(
    harness: &str,
    source: &str,
    load: &impl Fn(&str) -> Result<String>,
) -> Result<BTreeSet<String>> {
    let facts = super::rust_facts::parse(source)?;
    let mut ignored = facts.ignored_tests.into_iter().collect::<BTreeSet<_>>();
    let parent = harness.rsplit_once('/').ok_or("invalid G-E5 harness")?.0;
    for module in facts.modules {
        let path = module.path.ok_or("unknown G-E5 support module path")?;
        if !module.scope.is_empty() || path.split('/').any(|part| matches!(part, ".." | "")) {
            return Err("unknown G-E5 support module scope".into());
        }
        let child = super::rust_facts::parse(&load(&format!("{parent}/{path}"))?)?;
        for name in child.ignored_tests {
            if !ignored.insert(format!("{}::{name}", module.name)) {
                return Err("duplicate G-E5 support test".into());
            }
        }
    }
    Ok(ignored)
}

#[derive(Clone)]
pub struct Resolved {
    options: Value,
    crlf: bool,
}

fn resolve_with(
    input: &Value,
    row: &Value,
    load: &impl Fn(&str) -> Result<Rc<Value>>,
) -> Result<Resolved> {
    let origin = row
        .get("origin")
        .ok_or("G-E5 selected configuration has no parent origin")?;
    let suite = origin["suite"]
        .as_str()
        .ok_or("invalid G-E5 parent suite")?;
    let parent = load(suite)?;
    let rows = parent["cases"]
        .as_array()
        .ok_or("missing G-E5 parent cases")?;
    let selected: Vec<_> = rows
        .iter()
        .filter(|item| item["id"] == origin["id"])
        .collect();
    if selected.len() != 1 {
        return Err("ambiguous G-E5 parent configuration".into());
    }
    let mut options = selected[0]["options"]
        .as_object()
        .ok_or("missing parent options")?
        .clone();
    let profile = input["profiles"][row["profile"].as_str().ok_or("missing G-E5 profile")?]
        .as_object()
        .ok_or("unknown G-E5 profile")?;
    options.extend(profile.clone());
    let variant = row["variant"]
        .as_str()
        .ok_or("missing G-E5 coordinate variant")?;
    let crlf = if variant == "original" {
        selected[0]["files"][selected[0]["entry"]
            .as_str()
            .ok_or("missing parent entry")?]
        .as_str()
        .ok_or("missing parent entry source")?
        .contains("\r\n")
    } else {
        variant.starts_with("crlf")
    };
    Ok(Resolved {
        options: Value::Object(options),
        crlf,
    })
}

#[path = "case_ge5_rules.rs"]
mod rules;

#[derive(Clone, Copy)]
enum Domain {
    Form(&'static str),
    Forms(&'static [&'static str], &'static str),
    Public(&'static str),
    Capture(&'static str),
    Adjacent,
    All,
    Providers,
    LegacyPhase,
}
struct Rule {
    name: &'static str,
    script: &'static str,
    domain: Domain,
    direct: bool,
}
const ADJACENT: [&str; 7] = [
    "ge1_downlevel/called-imported-heritage",
    "ge1_downlevel/indexed-heritage",
    "ge1_downlevel/computed-constructor-spread",
    "ge1_downlevel/constructor-and-super-spread",
    "ge1_downlevel/computed-super-call-order",
    "ge1_downlevel/super-write-receiver-order",
    "ge2_module_helpers/inherit-constructor-spread",
];
const LEGACY: &str = "ge2_module_helpers/legacy-parameter-decorator";
const SAFE_HOME: [&str; 4] = [
    "ge3_decorator_compositions/private-super-method-safe-home-object",
    "ge3_decorator_compositions/private-super-accessors-safe-home-object",
    "ge3_decorator_compositions/private-static-super-safe-home-object",
    "ge3_decorator_compositions/private-super-async-generator-safe-home-object",
];
const STATIC_SUPER: [&str; 6] = [
    "ge3_decorator_compositions/static-super-computed-reads",
    "ge3_decorator_compositions/static-super-simple-writes",
    "ge3_decorator_compositions/static-super-compound-logical",
    "ge3_decorator_compositions/static-super-prefix-postfix",
    "ge3_decorator_compositions/static-super-strict-failed-write",
    "ge3_decorator_compositions/static-super-arrows-nested-homeobjects",
];

#[derive(Default)]
struct SelectorFacts<'a> {
    scripts: BTreeSet<String>,
    form_literals: BTreeSet<String>,
    literal_arguments: BTreeSet<String>,
    predicates: Vec<&'a syn::ExprClosure>,
    class_predicates: Vec<&'a syn::ExprClosure>,
    calls: BTreeSet<String>,
    forbidden: bool,
}
impl<'ast> Visit<'ast> for SelectorFacts<'ast> {
    fn visit_expr_lit(&mut self, node: &'ast syn::ExprLit) {
        if let Lit::Str(value) = &node.lit {
            let value = value.value();
            if value.ends_with(".cjs") {
                self.scripts.insert(value.clone());
            }
            if value.starts_with("ge3_decorator_compositions/") {
                self.form_literals.insert(value.clone());
            }
            if matches!(
                value.as_str(),
                "--probe" | "--shard" | "BLUEICE_GE5_ROLE_PROBE_ONLY" | "BLUEICE_GE5_POSITIVE_ONLY"
            ) {
                self.forbidden = true;
            }
        }
        visit::visit_expr_lit(self, node);
    }
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if node.method == "arg" {
            if let Some(value) = node.args.first().and_then(string) {
                self.literal_arguments.insert(value);
            }
        }
        if (node.method == "take" && !node.args.is_empty())
            || matches!(
                node.method.to_string().as_str(),
                "skip"
                    | "step_by"
                    | "nth"
                    | "find"
                    | "filter_map"
                    | "take_while"
                    | "skip_while"
                    | "map_while"
            )
        {
            self.forbidden = true;
        }
        if node.method == "filter" {
            if let Some(Expr::Closure(closure)) = node.args.first() {
                self.predicates.push(closure);
            } else {
                self.forbidden = true;
            }
        }
        visit::visit_expr_method_call(self, node);
    }
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let Expr::Path(path) = node.func.as_ref() {
            let name = path
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            self.calls.insert(name.clone());
            if name == "class_cli::run" {
                if let Some(Expr::Closure(closure)) = node.args.first() {
                    self.class_predicates.push(closure);
                } else {
                    self.forbidden = true;
                }
            }
        }
        visit::visit_expr_call(self, node);
    }
}
fn unwrapped(expression: &Expr) -> &Expr {
    match expression {
        Expr::Paren(p) => unwrapped(&p.expr),
        Expr::Group(g) => unwrapped(&g.expr),
        Expr::Block(b) if b.block.stmts.len() == 1 => match &b.block.stmts[0] {
            syn::Stmt::Expr(e, None) => unwrapped(e),
            _ => expression,
        },
        _ => expression,
    }
}
fn string(expression: &Expr) -> Option<String> {
    match unwrapped(expression) {
        Expr::Lit(l) => match &l.lit {
            Lit::Str(s) => Some(s.value()),
            _ => None,
        },
        _ => None,
    }
}
fn path(expression: &Expr, name: &str) -> bool {
    matches!(unwrapped(expression),Expr::Path(p) if p.path.is_ident(name))
}
fn row_binding(closure: &syn::ExprClosure) -> Option<String> {
    if closure.inputs.len() != 1 {
        return None;
    }
    let binding = match closure.inputs.first()? {
        syn::Pat::Tuple(tuple)
            if tuple.elems.len() == 2 && matches!(tuple.elems.first(), Some(syn::Pat::Wild(_))) =>
        {
            tuple.elems.last()?
        }
        binding => binding,
    };
    match binding {
        syn::Pat::Ident(binding) if binding.subpat.is_none() => Some(binding.ident.to_string()),
        _ => None,
    }
}
fn form_index(expression: &Expr, row: &str) -> bool {
    matches!(unwrapped(expression), Expr::Index(index)
        if path(&index.expr, row) && string(&index.index).as_deref() == Some("form"))
}
fn form_predicate(closure: &syn::ExprClosure, form: &str) -> bool {
    let Some(row) = row_binding(closure) else {
        return false;
    };
    let Expr::Binary(equal) = unwrapped(&closure.body) else {
        return false;
    };
    matches!(equal.op, syn::BinOp::Eq(_))
        && ((form_index(&equal.left, &row) && string(&equal.right).as_deref() == Some(form))
            || (form_index(&equal.right, &row) && string(&equal.left).as_deref() == Some(form)))
}
fn adjacent_predicate(outer: &syn::ExprClosure) -> bool {
    let Some(row) = row_binding(outer) else {
        return false;
    };
    let Expr::MethodCall(some) = unwrapped(&outer.body) else {
        return false;
    };
    if some.method != "is_some_and" || some.args.len() != 1 {
        return false;
    }
    let Expr::MethodCall(as_str) = unwrapped(&some.receiver) else {
        return false;
    };
    if as_str.method != "as_str" || !as_str.args.is_empty() || !form_index(&as_str.receiver, &row) {
        return false;
    }
    let Some(Expr::Closure(closure)) = some.args.first() else {
        return false;
    };
    let Some(form) = row_binding(closure) else {
        return false;
    };
    let Expr::Binary(or) = unwrapped(&closure.body) else {
        return false;
    };
    if !matches!(or.op, syn::BinOp::Or(_)) {
        return false;
    }
    let Expr::MethodCall(contains) = unwrapped(&or.left) else {
        return false;
    };
    if contains.method != "contains"
        || !path(&contains.receiver, "class_forms")
        || contains.args.len() != 1
    {
        return false;
    }
    let Some(Expr::Reference(argument)) = contains.args.first() else {
        return false;
    };
    if !path(&argument.expr, &form) {
        return false;
    }
    matches!(unwrapped(&or.right), Expr::Binary(eq)
        if matches!(eq.op, syn::BinOp::Eq(_)) && path(&eq.left, &form)
            && string(&eq.right).as_deref() == Some(LEGACY))
}
fn validate_rule(rule: &Rule, function: &ItemFn, facts: &super::rust_facts::Facts) -> Result<()> {
    let mut parsed = SelectorFacts::default();
    parsed.visit_block(&function.block);
    if parsed.forbidden {
        return Err(format!("partial/unknown G-E5 selector in {}", rule.name).into());
    }
    if matches!(rule.domain, Domain::Forms(_, _)) {
        let permitted = [
            "--experimental-vm-modules".into(),
            rule.script.into(),
            format!(
                "../../development/browser_core/phase-18-bluets/tools/{}",
                rule.script
            ),
        ];
        if !parsed.form_literals.is_empty()
            || parsed
                .literal_arguments
                .iter()
                .any(|arg| !permitted.contains(arg))
        {
            return Err(format!(
                "partial/unknown G-E5 multi-family selector in {}",
                rule.name
            )
            .into());
        }
    }
    if rule.script.is_empty() {
        if !parsed.scripts.is_empty() {
            return Err("unexpected G-E5 script selector".into());
        }
    } else {
        let valid = [
            rule.script.to_string(),
            format!(
                "../../development/browser_core/phase-18-bluets/tools/{}",
                rule.script
            ),
        ];
        if parsed.scripts.len() != 1 || !parsed.scripts.iter().all(|script| valid.contains(script))
        {
            return Err(format!("unknown G-E5 script selector in {}", rule.name).into());
        }
    }
    match rule.domain {
        Domain::Adjacent => {
            let array = super::case_inline::array(facts, rule.name, "class_forms")?;
            if array
                .labels
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
                != ADJACENT.into_iter().collect()
                || parsed.predicates.len() != 1
                || !adjacent_predicate(parsed.predicates[0])
                || !parsed.class_predicates.is_empty()
            {
                return Err("partial/unknown adjacent G-E5 matrix selector".into());
            }
        }
        Domain::Form(form) if rule.direct => {
            let predicates = parsed
                .predicates
                .iter()
                .chain(&parsed.class_predicates)
                .copied()
                .collect::<Vec<_>>();
            if predicates.len() != 1 || !form_predicate(predicates[0], form) {
                return Err(format!("partial/unknown G-E5 form selector in {}", rule.name).into());
            }
        }
        _ => {
            if !parsed.predicates.is_empty() || !parsed.class_predicates.is_empty() {
                return Err(format!("unexpected G-E5 wrapper predicate in {}", rule.name).into());
            }
        }
    }
    Ok(())
}
fn supplemental(
    harness: &str,
    rule: &Rule,
    axes: &[(&str, Vec<String>)],
    comparison: Comparison,
    reason: &str,
) -> Vec<Case> {
    let mut rows = vec![String::new()];
    for (key, values) in axes {
        rows = rows
            .iter()
            .flat_map(|base| {
                values
                    .iter()
                    .map(move |value| format!("{base};{key}={value}"))
            })
            .collect();
    }
    rows.into_iter()
        .map(|axes| {
            let mut case = Case::new(
                harness,
                &format!("function:{}{axes}", rule.name),
                &format!("{}{}", rule.name, axes),
                "supplemental-program/configuration",
            );
            case.comparison = comparison.clone();
            case.reason = Some(reason.into());
            case
        })
        .collect()
}
fn bool_values() -> Vec<String> {
    [false, true].into_iter().map(|v| v.to_string()).collect()
}
fn values(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| (*s).to_string()).collect()
}

pub fn select(
    harness: &str,
    source: &str,
    ignored: &[String],
    input: &Value,
    resolve: &impl Fn(&Value) -> Result<Resolved>,
) -> Result<Vec<Case>> {
    let stem = harness
        .rsplit('/')
        .next()
        .and_then(|s| s.strip_suffix(".rs"))
        .ok_or("invalid G-E5 harness")?;
    let contracts =
        rules::rules(stem).ok_or_else(|| format!("unknown G-E5 harness selector: {stem}"))?;
    let facts = super::rust_facts::parse(source)?;
    let expected = contracts
        .iter()
        .map(|r| r.name.to_string())
        .collect::<BTreeSet<_>>();
    if ignored.iter().cloned().collect::<BTreeSet<_>>() != expected
        || ignored.len() != expected.len()
        || facts.ignored_tests.iter().cloned().collect::<BTreeSet<_>>() != expected
        || facts.ignored_tests.len() != expected.len()
    {
        return Err(format!("partial/unknown G-E5 ignored-test selection: {stem}").into());
    }
    let file = syn::parse_file(source)?;
    let functions = file
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Fn(f) => Some((f.sig.ident.to_string(), f)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let text = serde_json::to_string(input)?;
    let original =
        super::case_sources::json_cases(INPUT, &text, "/cases", "program/configuration")?;
    let specifications = input["cases"].as_array().ok_or("missing G-E5 case rows")?;
    let mut cases = Vec::new();
    let mut unique = BTreeSet::new();
    for rule in contracts {
        let function = functions
            .get(rule.name)
            .ok_or("missing G-E5 ignored function")?;
        validate_rule(rule, function, &facts)?;
        let mut selected = Vec::new();
        match rule.domain {
            Domain::Providers => {
                selected=supplemental(harness,rule,&[("module",values(&["CommonJS","ESNext"])),("emitted-shape",values(&["native","blue"])),("provider",values(&["private","ambient","import"]))],Comparison::BoundaryControl,"Handwritten emitted helper shapes exercise the actual pinned tslib loader; no compiler agreement or frozen matrix row is counted.");
            }
            Domain::LegacyPhase => {
                selected=supplemental(harness,rule,&[("class_decorated",bool_values()),("define",bool_values()),("module",values(&["CommonJS","AMD","UMD","System"])),("variant",values(&["original","crlfAstral"])),("source_map",bool_values())],Comparison::Compared,"Supplemental two-file legacy initialization program; source files, Native/Blue executions, repeat builds and UMD runtime routes do not multiply cases.");
            }
            Domain::Capture(form) => {
                let mut profiles = BTreeSet::new();
                for row in specifications.iter().filter(|row| row["form"] == form) {
                    let resolved = resolve(row)?;
                    let options = resolved.options;
                    let module = options["module"].as_str().ok_or("missing capture module")?;
                    let imported = options["importHelpers"].as_bool().unwrap_or(false);
                    let ambient = options["noEmitHelpers"].as_bool().unwrap_or(false);
                    if !["CommonJS", "ESNext"].contains(&module) || imported && ambient {
                        continue;
                    }
                    let key = format!(
                        "{}:{module}:{imported}:{ambient}:{}",
                        options["target"].as_str().ok_or("missing capture target")?,
                        resolved.crlf
                    );
                    if profiles.insert(key.clone()) {
                        let mut case = Case::new(
                            harness,
                            &format!("function:{};profile={key}", rule.name),
                            &format!("{}:{key}", rule.name),
                            "supplemental-program/configuration",
                        );
                        case.reason=Some("Generated captured-this/arguments caller replaces the original main.ts; each target/module/helper/coordinate profile is selected once, not every frozen publication profile.".into());
                        selected.push(case);
                    }
                }
            }
            domain => {
                if matches!(domain, Domain::All) {
                    let function = functions.get(BLUE_MATRIX_TEST).ok_or(
                        "G-E5 full input adapter requires the Blue declaration/bounds test",
                    )?;
                    let mut body = SelectorFacts::default();
                    body.visit_block(&function.block);
                    if !function
                        .attrs
                        .iter()
                        .any(|attr| attr.path().is_ident("test"))
                        || function
                            .attrs
                            .iter()
                            .any(|attr| attr.path().is_ident("ignore"))
                        || !body.calls.contains("declarations::run")
                        || body.forbidden
                    {
                        return Err("missing G-E5 Blue declaration/bounds assertion".into());
                    }
                }
                for (index, row) in specifications.iter().enumerate() {
                    let include = match domain {
                        Domain::All => true,
                        Domain::Form(form) => row["form"] == form,
                        Domain::Forms(forms, _) => row["form"]
                            .as_str()
                            .is_some_and(|form| forms.contains(&form)),
                        Domain::Adjacent => row["form"]
                            .as_str()
                            .is_some_and(|form| ADJACENT.contains(&form) || form == LEGACY),
                        Domain::Public(form) => {
                            if row["form"] != form
                                || !["original", "crlfAstral"]
                                    .contains(&row["variant"].as_str().unwrap_or(""))
                                || row["profile"] != "external"
                            {
                                false
                            } else {
                                let options = resolve(row)?.options;
                                ["CommonJS", "ESNext", "System"]
                                    .contains(&options["module"].as_str().unwrap_or(""))
                                    && !options["importHelpers"].as_bool().unwrap_or(false)
                                    && !options["noEmitHelpers"].as_bool().unwrap_or(false)
                            }
                        }
                        _ => unreachable!(),
                    };
                    if include {
                        let mut case = original[index].clone();
                        case.reason = Some(match domain {
                            Domain::All => "Raw declaration/declaration-map agreement and JavaScript metadata/bounds across the complete matrix; Native authority replay is not a whole-JavaScript-role claim.".into(),
                            Domain::Forms(_, scope) => scope.into(),
                            _ => format!("Exact finite G-E5 configuration selection exercised by {}; controls/repeat builds do not add matrix rows.",rule.name),
                        });
                        selected.push(case);
                    }
                }
            }
        }
        if let Domain::Forms(forms, _) = rule.domain {
            if forms
                .iter()
                .any(|form| !specifications.iter().any(|row| row["form"] == *form))
            {
                return Err(
                    format!("incomplete G-E5 multi-family source domain: {}", rule.name).into(),
                );
            }
        }
        if selected.is_empty() {
            return Err(format!("empty or unknown G-E5 source selector: {}", rule.name).into());
        }
        for case in selected {
            if unique.insert(case.id.clone()) {
                cases.push(case);
            }
        }
    }
    Ok(cases)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn input() -> Value {
        json!({"typescript":"5.9.3","cases":[{"id":"first","form":"ge3_decorator_compositions/literal-replacement-order"},{"id":"other","form":"ge3_decorator_compositions/literal-fields"},{"id":"last","form":"ge3_decorator_compositions/literal-replacement-order"}]})
    }
    fn source(predicate: &str) -> String {
        format!(
            r#"#[test] #[ignore] fn literal_replacement_order_preserves_every_complete_module_role() {{ let selection = rows.iter().filter(|row| {predicate}).collect::<Vec<_>>(); Command::new("node").arg("verify_ge5_decorator_replacement_roles.cjs"); }}"#
        )
    }
    #[test]
    fn exact_frozen_form_uses_only_its_original_json_rows() {
        let rows = select(
            "backend/bluets/tests/ge5_literal_replacement_module_maps.rs",
            &source("row[\"form\"] == \"ge3_decorator_compositions/literal-replacement-order\""),
            &["literal_replacement_order_preserves_every_complete_module_role".into()],
            &input(),
            &|_| unreachable!(),
        )
        .unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| row.selector.as_str())
                .collect::<Vec<_>>(),
            ["/cases/0", "/cases/2"]
        );
        assert!(rows
            .iter()
            .all(|row| row.source == INPUT && row.comparison == Comparison::Compared));
    }
    #[test]
    fn partial_unknown_and_changed_script_selectors_are_rejected() {
        let harness = "backend/bluets/tests/ge5_literal_replacement_module_maps.rs";
        let ignored = ["literal_replacement_order_preserves_every_complete_module_role".into()];
        for predicate in [
            "row[\"form\"] == \"unknown\"",
            "other[\"form\"] == \"ge3_decorator_compositions/literal-replacement-order\"",
            "row[\"form\"] == \"ge3_decorator_compositions/literal-replacement-order\" && row[\"variant\"] == \"original\"",
        ] {
            assert!(select(harness, &source(predicate), &ignored, &input(), &|_| unreachable!()).is_err());
        }
        let full =
            source("row[\"form\"] == \"ge3_decorator_compositions/literal-replacement-order\"");
        assert!(select(
            harness,
            &full.replace(".collect", ".take(1).collect"),
            &ignored,
            &input(),
            &|_| unreachable!()
        )
        .is_err());
        assert!(select(
            harness,
            &full.replace(".collect", ".filter_map(|row| Some(row)).collect"),
            &ignored,
            &input(),
            &|_| unreachable!()
        )
        .is_err());
        assert!(select(
            harness,
            &full.replace(
                "verify_ge5_decorator_replacement_roles.cjs",
                "verify_ge5_unknown.cjs"
            ),
            &ignored,
            &input(),
            &|_| unreachable!()
        )
        .is_err());
        assert!(select(harness, &full, &[], &input(), &|_| unreachable!()).is_err());
        assert!(select(
            "backend/bluets/tests/ge5_unknown.rs",
            &full,
            &ignored,
            &input(),
            &|_| unreachable!()
        )
        .is_err());
    }
    #[test]
    fn historical_double_accessor_name_is_not_a_refusal() {
        let source = r#"#[test] #[ignore] fn double_decorated_accessors_all_frozen_products(){Command::new("node").arg("verify_ge5_double_accessor_roles.cjs");}"#;
        let input = json!({"typescript":"5.9.3","cases":[{"id":"rejected-historical-label","form":"ge3_decorator_compositions/rejected-double-decorated-accessors"},{"id":"other","form":"other"}]});
        let rows = select(
            "backend/bluets/tests/ge5_double_accessor_family.rs",
            source,
            &["double_decorated_accessors_all_frozen_products".into()],
            &input,
            &|_| unreachable!(),
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].comparison, Comparison::Compared);
    }
    #[test]
    fn safe_and_static_wrappers_select_only_their_original_frozen_forms() {
        let safe = [
            "private-super-method-safe-home-object",
            "private-super-accessors-safe-home-object",
            "private-static-super-safe-home-object",
            "private-super-async-generator-safe-home-object",
        ];
        let static_super = [
            "static-super-computed-reads",
            "static-super-simple-writes",
            "static-super-compound-logical",
            "static-super-prefix-postfix",
            "static-super-strict-failed-write",
            "static-super-arrows-nested-homeobjects",
        ];
        let input = json!({"typescript":"5.9.3","cases":safe.iter().chain(&static_super)
            .chain(["unrelated"].iter()).enumerate().map(|(index,form)|
                json!({"id":index.to_string(),"form":format!("ge3_decorator_compositions/{form}")}))
            .collect::<Vec<_>>()});
        for (harness, name, script, count, scope) in [
            (
                "ge5_safe_home_object",
                "safe_home_object_families_keep_complete_modules_and_logical_ends",
                "verify_ge5_safe_home_roles.cjs",
                4,
                "complete module",
            ),
            (
                "ge5_static_super_complete_family",
                "static_super_all_frozen_products",
                "verify_ge5_static_super_complete.cjs",
                6,
                "complete module",
            ),
            (
                "ge5_static_super_family",
                "static_super_public_cli_reference_phases",
                "verify_ge5_static_super_public.cjs",
                6,
                "not complete module",
            ),
        ] {
            let source = format!(
                r#"#[test] #[ignore] fn {name}(){{Command::new("node").arg("{script}");}}"#
            );
            let path = format!("backend/bluets/tests/{harness}.rs");
            let ignored = [name.into()];
            let rows = select(&path, &source, &ignored, &input, &|_| unreachable!()).unwrap();
            assert_eq!(rows.len(), count);
            assert!(rows.iter().all(|row| row.source == INPUT
                && row.comparison == Comparison::Compared
                && row.reason.as_deref().unwrap().contains(scope)));
            for partial in [
                source.replace("Command::new", "let _ = \"ge3_decorator_compositions/static-super-computed-reads\"; Command::new"),
                source.replace(".arg(\"", ".arg(\"--shard\").arg(\""),
                source.replace(".arg(\"", ".arg(\"unknown-selector\").arg(\""),
                source.replace(script, "verify_ge5_unknown.cjs"),
                source.replace("#[ignore]", ""),
            ] {
                assert!(select(&path, &partial, &ignored, &input, &|_| unreachable!()).is_err());
            }
            let mut incomplete = input.clone();
            incomplete["cases"]
                .as_array_mut()
                .unwrap()
                .remove(if count == 4 { 0 } else { 4 });
            assert!(select(&path, &source, &ignored, &incomplete, &|_| unreachable!()).is_err());
        }
    }
    #[test]
    fn supplemental_project_and_loader_axes_have_their_own_units() {
        for (harness, name, script, count, comparison) in [
            (
                "ge5_legacy_class_phase",
                "native_cycle_products_preserve_all_class_initialization_phases",
                "verify_ge5_decorator_class_phase.cjs",
                64,
                Comparison::Compared,
            ),
            (
                "ge5_helper_provider_globals",
                "private_ambient_and_imported_helpers_keep_their_own_bindings",
                "verify_ge5_helper_provider_globals.cjs",
                12,
                Comparison::BoundaryControl,
            ),
        ] {
            let source = format!(
                r#"#[test] #[ignore] fn {name}(){{Command::new("node").arg("{script}");}}"#
            );
            let rows = select(
                &format!("backend/bluets/tests/{harness}.rs"),
                &source,
                &[name.into()],
                &input(),
                &|_| unreachable!(),
            )
            .unwrap();
            assert_eq!(rows.len(), count);
            assert_eq!(
                rows.iter()
                    .map(|row| &row.id)
                    .collect::<BTreeSet<_>>()
                    .len(),
                count
            );
            assert!(rows
                .iter()
                .all(|row| row.unit == "supplemental-program/configuration"
                    && row.comparison == comparison));
        }
    }
    #[test]
    fn current_worktree_g5_adapters_have_exact_finite_sources() {
        let root = std::env::var_os("BLUEICE_GE5_INVENTORY_SOURCE_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
        let input: Value =
            serde_json::from_slice(&std::fs::read(root.join(INPUT)).unwrap()).unwrap();
        let mut report = Vec::new();
        let mut errors = Vec::new();
        let parents = RefCell::new(BTreeMap::<String, Rc<Value>>::new());
        for entry in std::fs::read_dir(root.join("backend/bluets/tests")).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap();
            if !name.starts_with("ge5_") || !name.ends_with(".rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let facts = super::super::rust_facts::parse(&source).unwrap();
            if facts.ignored_tests.is_empty() {
                continue;
            }
            let harness = format!("backend/bluets/tests/{name}");
            let rows = select(&harness, &source, &facts.ignored_tests, &input, &|row| {
                resolve_with(&input, row, &|suite| {
                    let cached = parents.borrow().get(suite).cloned();
                    if let Some(parent) = cached {
                        return Ok(parent);
                    }
                    let parent: Rc<Value> = Rc::new(serde_json::from_slice(&std::fs::read(
                        root.join(format!("backend/bluets/tests/fixtures/{suite}/cases.json")),
                    )?)?);
                    parents.borrow_mut().insert(suite.into(), parent.clone());
                    Ok(parent)
                })
            });
            match rows {
                Ok(rows) => {
                    let frozen = rows.iter().filter(|row| row.source == INPUT).count();
                    report.push(json!({
                        "harness":harness,
                        "sourceSha256":super::super::case_report::digest(source.as_bytes()),
                        "ignoredFunctions":facts.ignored_tests,
                        "cases":rows.len(),
                        "frozenConfigurations":frozen,
                        "supplementalConfigurations":rows.len()-frozen,
                        "compared":rows.iter().filter(|r|r.comparison==Comparison::Compared).count(),
                        "boundaryControls":rows.iter().filter(|r|r.comparison==Comparison::BoundaryControl).count(),
                        "sourceSelectors":rows.iter().map(|row| (&row.source, &row.selector)).collect::<Vec<_>>()
                    }));
                }
                Err(error) => errors.push(format!("{harness}: {error}")),
            }
        }
        report.sort_by_key(|row| row["harness"].as_str().unwrap().to_string());
        if let Some(path) = std::env::var_os("BLUEICE_GE5_INVENTORY_AUDIT_REPORT") {
            std::fs::write(
                path,
                serde_json::to_vec_pretty(&json!({
                    "scope":"Working-tree adapter audit only; no measurement or pass rate is published",
                    "harnesses":report,
                    "errors":errors
                }))
                .unwrap(),
            )
            .unwrap();
        }
        assert!(!report.is_empty());
        assert!(errors.is_empty(), "{}", errors.join("\n"));
    }
    #[test]
    fn public_subsets_and_generated_capture_profiles_do_not_multiply_the_matrix() {
        let source = r#"#[test] #[ignore] fn private_accessor_public_cli_phases(){run("verify_ge5_private_accessor_public.cjs");} #[test] #[ignore] fn private_accessor_all_frozen_products(){run("verify_ge5_private_accessor_roles.cjs");}"#;
        let mut input = input();
        input["cases"] = json!([
            {"id":"imported","form":"ge2_module_helpers/private-accessor","variant":"original","profile":"external","options":{"module":"CommonJS","importHelpers":true}},
            {"id":"public","form":"ge2_module_helpers/private-accessor","variant":"crlfAstral","profile":"external","options":{"module":"CommonJS"}},
            {"id":"inline","form":"ge2_module_helpers/private-accessor","variant":"original","profile":"inline","options":{"module":"ESNext"}}
        ]);
        let rows = select(
            "backend/bluets/tests/ge5_private_accessor_family.rs",
            source,
            &[
                "private_accessor_public_cli_phases".into(),
                "private_accessor_all_frozen_products".into(),
            ],
            &input,
            &|row| {
                Ok(Resolved {
                    options: row["options"].clone(),
                    crlf: row["variant"] == "crlfAstral",
                })
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter()
                .map(|row| row.selector.as_str())
                .collect::<Vec<_>>(),
            ["/cases/1", "/cases/0", "/cases/2"]
        );
        let source = r#"#[test] #[ignore] fn exported_async_generator_endpoints_all_frozen_products(){run("verify_ge5_async_generator_g2_endpoints.cjs");} #[test] #[ignore] fn borrowed_async_generator_methods_match_every_native_target_profile(){run("verify_ge5_async_generator_g2_borrowed.cjs");} #[test] #[ignore] fn borrowed_methods_preserve_source_receiver_and_arguments(){run("verify_ge5_async_generator_g2_capture.cjs");}"#;
        for (index, row) in input["cases"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            row["form"] = json!("ge2_module_helpers/async-generator-finally");
            row["options"] = json!({"target":"ES2015","module":"CommonJS"});
            row["variant"] = json!(if index == 2 { "crlfAstral" } else { "original" });
        }
        let rows = select(
            "backend/bluets/tests/ge5_async_generator_g2_endpoints.rs",
            source,
            &[
                "exported_async_generator_endpoints_all_frozen_products".into(),
                "borrowed_async_generator_methods_match_every_native_target_profile".into(),
                "borrowed_methods_preserve_source_receiver_and_arguments".into(),
            ],
            &input,
            &|row| {
                Ok(Resolved {
                    options: row["options"].clone(),
                    crlf: row["variant"] == "crlfAstral",
                })
            },
        )
        .unwrap();
        assert_eq!(rows.iter().filter(|row| row.source == INPUT).count(), 3);
        assert_eq!(rows.iter().filter(|row| row.source != INPUT).count(), 2);
    }
    #[test]
    fn all_matrix_source_requires_the_blue_assertion_not_only_native_authority() {
        let source = r#"#[test] #[ignore] fn complete_native_map_authority_is_reproducible(){run("../../development/browser_core/phase-18-bluets/tools/record_ge5_maps.cjs");}"#;
        let ignored = ["complete_native_map_authority_is_reproducible".into()];
        let harness = "backend/bluets/tests/ge5_source_maps.rs";
        assert!(select(harness, source, &ignored, &input(), &|_| unreachable!()).is_err());
        let without_test = format!("{source} fn {BLUE_MATRIX_TEST}(){{declarations::run();}}");
        assert!(select(
            harness,
            &without_test,
            &ignored,
            &input(),
            &|_| unreachable!()
        )
        .is_err());
        let source = format!("{source} #[test] fn {BLUE_MATRIX_TEST}(){{declarations::run();}}");
        let rows = select(harness, &source, &ignored, &input(), &|_| unreachable!()).unwrap();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|row| row
            .reason
            .as_deref()
            .unwrap()
            .contains("not a whole-JavaScript-role claim")));
        let mut measured = ignored.into_iter().collect::<BTreeSet<_>>();
        assert!(validate_measured_tests(harness, &measured).is_err());
        measured.insert(BLUE_MATRIX_TEST.into());
        validate_measured_tests(harness, &measured).unwrap();
    }
    #[test]
    fn source_matrix_ignored_modules_are_derived_without_counting_replay_as_blue_proof() {
        let source = r#"#[path="roles.rs"] mod roles;
            #[test] #[ignore] fn complete_native_map_authority_is_reproducible() {}
        "#;
        let ignored = module_ignored("backend/bluets/tests/ge5_source_maps.rs", source, &|path| {
            assert_eq!(path, "backend/bluets/tests/roles.rs");
            Ok("#[test] #[ignore] fn full_roles() {}".into())
        })
        .unwrap();
        assert_eq!(
            ignored,
            [
                "complete_native_map_authority_is_reproducible".to_string(),
                "roles::full_roles".to_string()
            ]
            .into_iter()
            .collect()
        );
        let mut measured = ignored;
        assert!(
            validate_measured_tests("backend/bluets/tests/ge5_source_maps.rs", &measured).is_err()
        );
        measured.insert(BLUE_MATRIX_TEST.into());
        validate_measured_tests("backend/bluets/tests/ge5_source_maps.rs", &measured).unwrap();
        assert!(
            module_ignored("backend/bluets/tests/ge5_source_maps.rs", source, &|_| Err(
                "missing module".into()
            ))
            .is_err()
        );
    }
}
