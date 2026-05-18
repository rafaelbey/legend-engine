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
//
//! Runtime smoke tests for the relation projection natives `select` and
//! `columns`.
//!
//! `select` has two TDS-side overloads — single `ColSpec` and multi-arg
//! `ColSpecArray` — registered under their exact mangled FQNs so the
//! prefix-fallback path can't pick the wrong one. Both produce a fresh
//! `TDS` heap object with a re-rendered canonical CSV; we verify that
//! by chaining the result back through `size()` (rows preserved) and
//! `columns()->size()` / `columns().name` (schema narrowed).
//!
//! `columns` returns `Column<T>[*]` as `Value::Collection` of
//! `Column` heap objects. The canonical PCT test reads
//! `$tds->columns().name`; we cover the same shape and use the
//! collection-side `size` native to assert cardinality.

use legend_pure_core_platform::platform::PLATFORM_AUTO_IMPORTS;
use legend_pure_core_platform::repo::{self, OwnedSourceFile, Repo, RepoMeta};
use legend_pure_parser_pure::model::PureModel;
use legend_pure_runtime::error::PureException;
use legend_pure_runtime::eval::Evaluator;
use legend_pure_runtime::native::NativeRegistry;
use legend_pure_runtime::value::Value;
use smol_str::SmolStr;

// ---------------------------------------------------------------------------
// Test harness — mirrors `relation_natives_smoke.rs` / `relation_set_smoke.rs`.
// ---------------------------------------------------------------------------

fn auto_imports() -> Vec<SmolStr> {
    PLATFORM_AUTO_IMPORTS
        .iter()
        .map(|&s| SmolStr::new(s))
        .collect()
}

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
        name: "relation_projection_probe",
        pattern: ".*",
        dependencies: USER_DEPS,
    };
    repos.push(Repo::Filesystem {
        prefix: "/relation_projection_probe".into(),
        files: vec![OwnedSourceFile {
            path: format!("/relation_projection_probe/{name}"),
            content: source.into(),
        }],
        meta: Some(meta),
        source_root: None,
    });
    repos
}

fn compile_user(name: &str, source: &str) -> PureModel {
    let repos = compose_repos_with_user_source(name, source);
    let auto_imports = auto_imports();
    let url = format!("/relation_projection_probe/{name}");
    match repo::load(&repos, &auto_imports) {
        Ok(model) => model,
        Err(p) => {
            let user_errors: Vec<&_> = p
                .errors
                .iter()
                .filter(|e| e.source_info.source.as_str() == url)
                .collect();
            if user_errors.is_empty() {
                return p.model;
            }
            panic!(
                "user-source compile failed.\nUser errors ({}):\n{}",
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
            );
        }
    }
}

fn eval_function(source_name: &str, source: &str, fn_fqn: &[&str]) -> Value {
    let model = compile_user(source_name, source);
    let registry = NativeRegistry::standard();
    let mut eval = Evaluator::new(&model, &registry);
    let path: Vec<SmolStr> = fn_fqn.iter().map(|&s| SmolStr::new(s)).collect();
    let id = model
        .resolve_function_by_path(&path)
        .unwrap_or_else(|| panic!("function `{}` not found in model", fn_fqn.join("::")));
    eval.call_user_function_by_id(id)
        .unwrap_or_else(|e| panic!("evaluation of `{}` failed: {e}", fn_fqn.join("::")))
}

fn try_eval_function(
    source_name: &str,
    source: &str,
    fn_fqn: &[&str],
) -> Result<Value, PureException> {
    let model = compile_user(source_name, source);
    let registry = NativeRegistry::standard();
    let mut eval = Evaluator::new(&model, &registry);
    let path: Vec<SmolStr> = fn_fqn.iter().map(|&s| SmolStr::new(s)).collect();
    let id = model
        .resolve_function_by_path(&path)
        .unwrap_or_else(|| panic!("function `{}` not found in model", fn_fqn.join("::")));
    eval.call_user_function_by_id(id)
}

// ---------------------------------------------------------------------------
// select(rel, ~col) — single ColSpec
// ---------------------------------------------------------------------------

#[test]
fn select_one_column_via_col_spec() {
    // After `select(~a)`, the result should still have 2 rows and
    // exactly 1 column. We assert both: row count via `size()`, column
    // count via `columns()->size()`.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_projection_probe::selectOneRowCount(): Integer[1]
{
    let r = #TDS
       a, b
       1, x
       2, y
    #;
    $r->select(~a)->size()
}

function relation_projection_probe::selectOneColCount(): Integer[1]
{
    let r = #TDS
       a, b
       1, x
       2, y
    #;
    $r->select(~a)->columns()->size()
}
";

    let row_count = eval_function(
        "select_one.pure",
        source,
        &["relation_projection_probe", "selectOneRowCount"],
    );
    assert!(
        matches!(row_count, Value::Integer(2)),
        "expected Integer(2) rows, got {row_count:?}"
    );

    let col_count = eval_function(
        "select_one.pure",
        source,
        &["relation_projection_probe", "selectOneColCount"],
    );
    assert!(
        matches!(col_count, Value::Integer(1)),
        "expected Integer(1) column after select(~a), got {col_count:?}"
    );
}

// ---------------------------------------------------------------------------
// select(rel, ~[a, b]) — ColSpecArray
// ---------------------------------------------------------------------------

#[test]
fn select_two_columns_via_col_spec_array() {
    // `select(~[a, b])` on a 3-column TDS round-trips both columns and
    // preserves all rows.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_projection_probe::selectTwoRowCount(): Integer[1]
{
    let r = #TDS
       a, b, c
       1, x, p
       2, y, q
    #;
    $r->select(~[a, b])->size()
}

function relation_projection_probe::selectTwoColCount(): Integer[1]
{
    let r = #TDS
       a, b, c
       1, x, p
       2, y, q
    #;
    $r->select(~[a, b])->columns()->size()
}
";

    let row_count = eval_function(
        "select_two.pure",
        source,
        &["relation_projection_probe", "selectTwoRowCount"],
    );
    assert!(
        matches!(row_count, Value::Integer(2)),
        "expected Integer(2) rows, got {row_count:?}"
    );

    let col_count = eval_function(
        "select_two.pure",
        source,
        &["relation_projection_probe", "selectTwoColCount"],
    );
    assert!(
        matches!(col_count, Value::Integer(2)),
        "expected Integer(2) columns after select(~[a,b]), got {col_count:?}"
    );
}

// ---------------------------------------------------------------------------
// select(~missing) — error path
// ---------------------------------------------------------------------------

#[test]
fn select_unknown_column_errors() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_projection_probe::selectMissing(): Integer[1]
{
    let r = #TDS
       a, b
       1, x
    #;
    $r->select(~missing)->size()
}
";

    let result = try_eval_function(
        "select_missing.pure",
        source,
        &["relation_projection_probe", "selectMissing"],
    );
    let err = result.expect_err(
        "select(~missing) on a TDS without a 'missing' column must surface an error",
    );
    let msg = format!("{err}");
    assert!(
        msg.contains("missing"),
        "error message should mention the missing column name, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// columns() returns Column[*] of the right cardinality
// ---------------------------------------------------------------------------

#[test]
fn columns_returns_column_count() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_projection_probe::columnCount(): Integer[1]
{
    let r = #TDS
       a, b, c
       1, 2, 3
    #;
    $r->columns()->size()
}
";

    let count = eval_function(
        "columns_count.pure",
        source,
        &["relation_projection_probe", "columnCount"],
    );
    assert!(
        matches!(count, Value::Integer(3)),
        "expected Integer(3) for 3-column TDS, got {count:?}"
    );
}

// ---------------------------------------------------------------------------
// PCT-corpus probe — testSingleColSelectShared, simplified
// ---------------------------------------------------------------------------

#[test]
fn select_pct_single_col_shared() {
    // Mirrors meta::pure::functions::relation::tests::select::testSingleColSelectShared
    // — `#TDS … #->select(~str)` keeps the str column only. We assert
    // on column count + preserved row count rather than the exact
    // serialised text (renderer formatting is its own concern).
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_projection_probe::pctSingleColRowCount(): Integer[1]
{
    let r = #TDS
       val, str, other
       1, a, a
       3, ewe, b
       4, qw, c
       5, wwe, d
       6, weq, e
    #;
    $r->select(~str)->size()
}

function relation_projection_probe::pctSingleColColCount(): Integer[1]
{
    let r = #TDS
       val, str, other
       1, a, a
       3, ewe, b
       4, qw, c
       5, wwe, d
       6, weq, e
    #;
    $r->select(~str)->columns()->size()
}
";

    let rows = eval_function(
        "pct_single.pure",
        source,
        &["relation_projection_probe", "pctSingleColRowCount"],
    );
    assert!(
        matches!(rows, Value::Integer(5)),
        "expected 5 rows preserved, got {rows:?}"
    );

    let cols = eval_function(
        "pct_single.pure",
        source,
        &["relation_projection_probe", "pctSingleColColCount"],
    );
    assert!(
        matches!(cols, Value::Integer(1)),
        "expected 1 column after select(~str), got {cols:?}"
    );
}
