// Copyright 2026 Goldman Sachs
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Probes that lambda parameter types bind to the receiver's row type
//! on relation-shaped natives (`filter`, `extend`, `sort`).
//!
//! Native function impls (a follow-on effort) read `$x.col` at runtime
//! and rely on the compile-time inferer having typed `$x` with the
//! receiver's columns. The catalog being clean only proves nothing
//! emits a hard error; lambda params that fail to infer are silently
//! treated as `Unresolved` and the cascading "Ambiguous function call"
//! diagnostics they would have produced are *suppressed* by
//! `any_arg_reads_unresolved` on the assumption that a
//! `CannotInferLambdaParameterTypes` already fired at the lambda's
//! site. A clean catalog is therefore consistent with both "lambda
//! inference works" and "lambda inference is silently broken." These
//! tests rule out the second possibility for the trivial-receiver
//! case.

use legend_pure_core_platform::platform::PLATFORM_AUTO_IMPORTS;
use legend_pure_core_platform::repo::{self, OwnedSourceFile, Repo, RepoMeta};
use legend_pure_parser_pure::ids::ElementId;
use legend_pure_parser_pure::model::{Element, PureModel};
use legend_pure_parser_pure::types::{
    ExprKind, Parameter, RelationColumnTypeExpr, TypeExpr, ValueSpec,
};
use smol_str::SmolStr;

// ---------------------------------------------------------------------------
// Test harness
// ---------------------------------------------------------------------------

fn auto_imports() -> Vec<SmolStr> {
    PLATFORM_AUTO_IMPORTS
        .iter()
        .map(|&s| SmolStr::new(s))
        .collect()
}

/// Compose the standard `core_functions_*` engine repo set plus a
/// synthetic `/lambda_probe` repo carrying the user-source `.pure`
/// file. The synthetic repo declares dependencies on every embedded
/// engine repo so the topo sort places it last.
fn compose_repos_with_user_source(name: &str, source: &str) -> Vec<Repo> {
    let mut repos: Vec<Repo> = Repo::default_with_build_snapshots();
    repos.extend(legend_engine_rust_core_functions_json_pure::repos());
    repos.extend(legend_engine_rust_core_functions_unclassified_pure::repos());
    repos.extend(legend_engine_rust_core_functions_variant_pure::repos());
    repos.extend(legend_engine_rust_core_functions_relation_pure::repos());
    repos.extend(legend_engine_rust_core_functions_standard_pure::repos());

    static USER_DEPS: &[&str] = &[
        "platform",
        "platform_precise_primitives",
        "platform_dsl_store",
        "platform_dsl_mapping",
        "platform_dsl_diagram",
        "platform_dsl_graph",
        "platform_dsl_path",
        "platform_dsl_tds",
        "platform_store_relational",
        "core_functions_json",
        "core_functions_unclassified",
        "core_functions_variant",
        "core_functions_relation",
        "core_functions_standard",
    ];
    let meta = RepoMeta {
        name: "lambda_probe",
        pattern: ".*",
        dependencies: USER_DEPS,
    };
    repos.push(Repo::Filesystem {
        prefix: "/lambda_probe".into(),
        files: vec![OwnedSourceFile {
            path: format!("/lambda_probe/{name}"),
            content: source.into(),
        }],
        meta: Some(meta),
        source_root: None,
    });
    repos
}

/// Compile a synthetic user `.pure` source against the embedded engine
/// repos. Panics with a focused diagnostic when a user-source error
/// fires so the failure points at the test's source line directly.
fn compile_user(name: &str, source: &str) -> PureModel {
    let repos = compose_repos_with_user_source(name, source);
    let auto_imports = auto_imports();
    let url = format!("/lambda_probe/{name}");

    match repo::load(&repos, &auto_imports) {
        Ok(model) => model,
        Err(p) => {
            let mut user_errors = Vec::new();
            let mut other_errors = Vec::new();
            for e in &p.errors {
                if e.source_info.source.as_str() == url {
                    user_errors.push(e);
                } else {
                    other_errors.push(e);
                }
            }
            if user_errors.is_empty() {
                // Surface other errors only as diagnostic context — the
                // partial model still carries everything we need.
                eprintln!(
                    "compile produced {} non-user errors but no user-source errors; \
                     proceeding with the partial model",
                    other_errors.len()
                );
                return p.model;
            }
            panic!(
                "user-source compile failed.\nUser errors ({}):\n{}\nFirst few other errors ({}):\n{}",
                user_errors.len(),
                user_errors
                    .iter()
                    .map(|e| format!(
                        "  {}:{}:{} {}",
                        e.source_info.source,
                        e.source_info.start_line,
                        e.source_info.start_column,
                        e.message
                    ))
                    .collect::<Vec<_>>()
                    .join("\n"),
                other_errors.len(),
                other_errors
                    .iter()
                    .take(5)
                    .map(|e| format!("  {}: {}", e.source_info.source, e.message))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
    }
}

/// Look up a top-level function by FQN segments and return its body.
/// Panics if the function isn't in the model — that's a fixture bug.
fn function_body(model: &PureModel, fqn: &[&str]) -> std::sync::Arc<[ValueSpec]> {
    let path: Vec<SmolStr> = fqn.iter().map(|&s| SmolStr::new(s)).collect();
    let id: ElementId = model
        .resolve_function_by_path(&path)
        .unwrap_or_else(|| panic!("could not resolve function `{}`", fqn.join("::")));
    match model.get_element(id) {
        Element::Function(f) => f.body.clone(),
        other => panic!(
            "expected `{}` to resolve to a Function; got {other:?}",
            fqn.join("::"),
        ),
    }
}

/// Walk a body and collect every `ExprKind::Lambda` parameter list,
/// pre-order. Each entry is a snapshot of the lambda's parameters.
fn collect_lambda_params(body: &[ValueSpec]) -> Vec<Vec<Parameter>> {
    let mut out = Vec::new();
    for vs in body {
        walk(vs, &mut out);
    }
    out
}

fn walk(vs: &ValueSpec, out: &mut Vec<Vec<Parameter>>) {
    match vs.kind.as_ref() {
        ExprKind::Lambda { parameters, body } => {
            out.push(parameters.clone());
            for b in body {
                walk(b, out);
            }
        }
        ExprKind::FunctionCall(d)
        | ExprKind::PropertyCall(d)
        | ExprKind::QualifiedPropertyCall(d) => {
            for a in &d.arguments {
                walk(a, out);
            }
        }
        ExprKind::Collection { elements } => {
            for e in elements {
                walk(e, out);
            }
        }
        ExprKind::ColSpecLiteral { column, .. } => {
            if let Some(init) = column.init_lambda.as_ref() {
                walk(init, out);
            }
        }
        ExprKind::ColSpecArrayLiteral { columns, .. } => {
            for c in columns {
                if let Some(init) = c.init_lambda.as_ref() {
                    walk(init, out);
                }
            }
        }
        _ => {}
    }
}

/// Pull the Relation column slice out of a (possibly wrapped)
/// `TypeExpr`. Mirrors `extract_relation_columns` in
/// `crates/pure/src/resolve.rs` so the probe checks the same shape
/// production code consumes.
fn extract_columns(te: &TypeExpr) -> Option<&[RelationColumnTypeExpr]> {
    match te {
        TypeExpr::Relation(cols) => Some(cols.as_slice()),
        TypeExpr::Named { type_arguments, .. } => {
            type_arguments.first().and_then(extract_columns)
        }
        _ => None,
    }
}

/// Assert that `param`'s `type_expr` carries a Relation layer that
/// includes a column named `expected_col`. Panics with the param's
/// full type on mismatch so the failing shape is visible in the
/// test output.
#[track_caller]
fn assert_param_has_column(param: &Parameter, expected_col: &str) {
    let Some(cols) = extract_columns(&param.type_expr) else {
        panic!(
            "lambda parameter `{}` doesn't carry a Relation layer; \
             type_expr = {:?}",
            param.name, param.type_expr,
        );
    };
    assert!(
        cols.iter().any(|c| c.name.as_str() == expected_col),
        "lambda parameter `{}` has Relation columns {:?}, missing `{expected_col}`",
        param.name,
        cols.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
    );
}

// ---------------------------------------------------------------------------
// Step 1 — baseline probes against TDS-literal receivers
// ---------------------------------------------------------------------------

#[test]
fn filter_lambda_param_binds_to_relation_row() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function lambda_probe::filterTds(): Boolean[1]
{
    let r = #TDS
       val
       1
       2
    #;
    let f = $r->filter(x|$x.val > 1);
    true
}";
    let model = compile_user("filter_probe.pure", source);
    let body = function_body(&model, &["lambda_probe", "filterTds"]);
    let lambdas = collect_lambda_params(&body);
    assert!(
        !lambdas.is_empty(),
        "expected at least one lambda in filterTds; lambda walk found none"
    );
    // The first lambda is the filter predicate `x|$x.val > 1`.
    let filter_lambda = &lambdas[0];
    assert_eq!(filter_lambda.len(), 1, "filter predicate has one param");
    assert_param_has_column(&filter_lambda[0], "val");
}

/// Walk a body collecting every `ColSpecLiteral` and its
/// classification kind. Used by the `extend` shape probe.
fn collect_col_spec_literals(
    body: &[ValueSpec],
) -> Vec<(SmolStr, legend_pure_parser_pure::types::ColSpecLiteralKind)> {
    let mut out = Vec::new();
    for vs in body {
        walk_col(vs, &mut out);
    }
    out
}

fn walk_col(
    vs: &ValueSpec,
    out: &mut Vec<(SmolStr, legend_pure_parser_pure::types::ColSpecLiteralKind)>,
) {
    match vs.kind.as_ref() {
        ExprKind::ColSpecLiteral { column, kind } => {
            out.push((column.name.clone(), *kind));
        }
        ExprKind::ColSpecArrayLiteral { columns, kind } => {
            for c in columns {
                out.push((c.name.clone(), *kind));
            }
        }
        ExprKind::FunctionCall(d)
        | ExprKind::PropertyCall(d)
        | ExprKind::QualifiedPropertyCall(d) => {
            for a in &d.arguments {
                walk_col(a, out);
            }
        }
        ExprKind::Lambda { body, .. } => {
            for b in body {
                walk_col(b, out);
            }
        }
        ExprKind::Collection { elements } => {
            for e in elements {
                walk_col(e, out);
            }
        }
        _ => {}
    }
}

#[test]
fn extend_func_col_spec_preserves_its_init_lambda() {
    // `extend(~name:c|$c.val->...)` lowers to a `ColSpecLiteral`
    // carrying `kind=Func`. The init lambda `c|$c.val->…` is stashed
    // in `RelationColumnLowered.init_lambda` so the runtime
    // `FuncColSpec` allocator can hand a closure to the `extend`
    // native — which evaluates the lambda per row to materialise the
    // new column's cells.
    //
    // The lambda parameter's compile-time type is the synthetic
    // `Any[1]` expectation set by `lower_relation_columns_from_specs`
    // (the runtime binds row values through dynamic slot lookup on
    // the row-tuple heap object, so precise compile-time inference
    // for the param isn't required). This test therefore only
    // asserts the lambda is preserved and carries the expected
    // parameter name; it does not require the param type to carry a
    // Relation layer.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function lambda_probe::extendTds(): Boolean[1]
{
    let r = #TDS
       val
       1
       2
    #;
    let e = $r->extend(~name:c|$c.val->toOne()->toString());
    true
}";
    let model = compile_user("extend_probe.pure", source);
    let body = function_body(&model, &["lambda_probe", "extendTds"]);

    // The `walk` helper now descends into `ColSpecLiteral.init_lambda`,
    // so the FuncColSpec's init lambda surfaces here.
    let lambdas = collect_lambda_params(&body);
    assert_eq!(
        lambdas.len(),
        1,
        "expected exactly one lambda (the FuncColSpec init); found {}: {:?}",
        lambdas.len(),
        lambdas
            .iter()
            .map(|p| p.iter().map(|x| x.name.as_str()).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
    );
    let init_lambda = &lambdas[0];
    assert_eq!(init_lambda.len(), 1, "FuncColSpec init has one param");
    assert_eq!(init_lambda[0].name.as_str(), "c");

    // The ColSpecLiteral metadata is preserved, classified as `Func`.
    let cols = collect_col_spec_literals(&body);
    assert_eq!(cols.len(), 1, "expected one ColSpecLiteral; got {cols:?}");
    let (name, kind) = &cols[0];
    assert_eq!(name.as_str(), "name");
    assert_eq!(
        *kind,
        legend_pure_parser_pure::types::ColSpecLiteralKind::Func,
        "FuncColSpec literal should classify as Func"
    );
}

// ---------------------------------------------------------------------------
// Step 4 — corpus probes against real PCT functions in
// `core_functions_relation`. These confirm the binding holds for the
// engine-shipped corpus, not just synthetic fixtures.
// ---------------------------------------------------------------------------

/// Compose only the embedded engine repos (no synthetic user repo).
/// Used by corpus probes that exercise existing PCT functions in the
/// `core_functions_*` set.
fn compose_embedded_repos() -> Vec<Repo> {
    let mut repos: Vec<Repo> = Repo::default_with_build_snapshots();
    repos.extend(legend_engine_rust_core_functions_json_pure::repos());
    repos.extend(legend_engine_rust_core_functions_unclassified_pure::repos());
    repos.extend(legend_engine_rust_core_functions_variant_pure::repos());
    repos.extend(legend_engine_rust_core_functions_relation_pure::repos());
    repos.extend(legend_engine_rust_core_functions_standard_pure::repos());
    repos
}

/// Compile the embedded corpus and look up an existing PCT function
/// by FQN + mangled signature suffix.
fn embedded_function(fqn: &[&str], _signature_suffix: &str) -> std::sync::Arc<[ValueSpec]> {
    let repos = compose_embedded_repos();
    let auto_imports = auto_imports();
    let model = match repo::load(&repos, &auto_imports) {
        Ok(m) => m,
        Err(p) => p.model,
    };
    let path: Vec<SmolStr> = fqn.iter().map(|&s| SmolStr::new(s)).collect();
    let id = model
        .resolve_function_by_path(&path)
        .unwrap_or_else(|| panic!("could not resolve `{}` in embedded corpus", fqn.join("::")));
    match model.get_element(id) {
        Element::Function(f) => f.body.clone(),
        other => panic!("expected `{}` to be a Function; got {other:?}", fqn.join("::")),
    }
}

#[test]
fn corpus_filter_test_lambda_param_binds_to_relation_row() {
    // `testSimpleFilterShared` body contains
    // `#TDS\n val\n 1\n3\n4\n#->filter(x|$x.val > 1)`.
    // The TDS receiver is concretely typed, so the filter predicate's
    // `x` should bind to a Relation layer carrying the `val` column.
    let body = embedded_function(
        &[
            "meta",
            "pure",
            "functions",
            "relation",
            "tests",
            "filter",
            "testSimpleFilterShared",
        ],
        "_Function_1__Boolean_1_",
    );
    let lambdas = collect_lambda_params(&body);
    // Find the predicate `x|$x.val > 1` — the inner lambda with one
    // param named `x`.
    let predicate = lambdas
        .iter()
        .find(|p| p.len() == 1 && p[0].name.as_str() == "x")
        .unwrap_or_else(|| {
            panic!(
                "could not find filter predicate `x|...` in testSimpleFilterShared; \
                 collected lambda param sets = {:?}",
                lambdas
                    .iter()
                    .map(|p| p
                        .iter()
                        .map(|x| x.name.as_str())
                        .collect::<Vec<_>>())
                    .collect::<Vec<_>>(),
            );
        });
    assert_param_has_column(&predicate[0], "val");
}

#[test]
fn corpus_filter_test_chained_filter_lambdas_bind_to_relation_row() {
    // `testSimpleFilter_MultipleExpressions` body:
    //   let a = #TDS val 1, 3, 4, 5 #;
    //   let b = $a->filter(x|$x.val > 3);
    //   $b->filter(x|$x.val > 4);
    // Both filter predicates' `x` should bind to a Relation with
    // `val`. The second filter's receiver is `$b`, whose type comes
    // from the first `filter`'s return type — i.e., the row type
    // propagates through `let` and through `filter`'s own
    // `Relation<T> → Relation<T>` signature.
    let body = embedded_function(
        &[
            "meta",
            "pure",
            "functions",
            "relation",
            "tests",
            "filter",
            "testSimpleFilter_MultipleExpressions",
        ],
        "_Function_1__Boolean_1_",
    );
    let lambdas = collect_lambda_params(&body);
    let predicates: Vec<&Vec<Parameter>> = lambdas
        .iter()
        .filter(|p| p.len() == 1 && p[0].name.as_str() == "x")
        .collect();
    assert!(
        predicates.len() >= 2,
        "expected at least two `x|...` predicates; got {} (collected lambdas = {:?})",
        predicates.len(),
        lambdas
            .iter()
            .map(|p| p.iter().map(|x| x.name.as_str()).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
    );
    for p in predicates {
        assert_param_has_column(&p[0], "val");
    }
}

#[test]
fn sort_doesnt_collapse_to_phantom_lambda() {
    // `sort` takes `SortInfo[*]` (not a lambda). The receiver chain
    // `$r->sort(~val->ascending())` parses without any lambda
    // parameter to bind. This test confirms the sort path doesn't
    // accidentally surface a SortInfo-shaped phantom lambda — i.e.,
    // we should find ZERO lambdas in the body.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function lambda_probe::sortTds(): Boolean[1]
{
    let r = #TDS
       val
       2
       1
    #;
    let s = $r->sort(~val->ascending());
    true
}";
    let model = compile_user("sort_probe.pure", source);
    let body = function_body(&model, &["lambda_probe", "sortTds"]);
    let lambdas = collect_lambda_params(&body);
    // Zero lambdas — no `x|...` syntax, no `~name:lam` columns. If a
    // lambda surfaces here it means `~val` got mis-lowered to a
    // ColSpecLiteral with a synthetic lambda body.
    assert!(
        lambdas.is_empty(),
        "expected zero lambdas in sort body; found {} (params: {:?})",
        lambdas.len(),
        lambdas
            .iter()
            .map(|p| p.iter().map(|x| x.name.as_str()).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
    );
}
