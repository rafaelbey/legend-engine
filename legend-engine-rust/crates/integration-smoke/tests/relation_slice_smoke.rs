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
//! Runtime smoke tests for relation row-slice natives `limit` and
//! `drop`. Both produce a fresh `TDS` heap object whose canonical
//! `csv` slot reparses to the expected row count via
//! `meta::pure::functions::relation::size`. Assertions live on the
//! integer size of the result so the tests stay decoupled from the
//! (still-evolving) CSV renderer formatting.

use legend_pure_core_platform::platform::PLATFORM_AUTO_IMPORTS;
use legend_pure_core_platform::repo::{self, OwnedSourceFile, Repo, RepoMeta};
use legend_pure_parser_pure::model::PureModel;
use legend_pure_runtime::eval::Evaluator;
use legend_pure_runtime::native::NativeRegistry;
use legend_pure_runtime::value::Value;
use smol_str::SmolStr;

// ---------------------------------------------------------------------------
// Test harness — mirrors `relation_natives_smoke.rs`
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
        name: "relation_slice_probe",
        pattern: ".*",
        dependencies: USER_DEPS,
    };
    repos.push(Repo::Filesystem {
        prefix: "/relation_slice_probe".into(),
        files: vec![OwnedSourceFile {
            path: format!("/relation_slice_probe/{name}"),
            content: source.into(),
        }],
        meta: Some(meta),
    });
    repos
}

fn compile_user(name: &str, source: &str) -> PureModel {
    let repos = compose_repos_with_user_source(name, source);
    let auto_imports = auto_imports();
    let url = format!("/relation_slice_probe/{name}");

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

fn assert_size(result: Value, expected: i64) {
    match result {
        Value::Integer(n) if n == expected => {}
        other => panic!("expected Integer({expected}), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// limit(rel, n) — first n rows
// ---------------------------------------------------------------------------

#[test]
fn limit_to_two_returns_two_rows() {
    // Three-row TDS, limit to 2 — result size must be 2.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_slice_probe::limitTwo(): Integer[1]
{
    let r = #TDS
       val
       1
       2
       3
    #;
    $r->limit(2)->size()
}";
    let result = eval_function(
        "limit_two.pure",
        source,
        &["relation_slice_probe", "limitTwo"],
    );
    assert_size(result, 2);
}

#[test]
fn limit_clamps_when_count_exceeds_rows() {
    // limit > size — clamps to row count, returns all rows.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_slice_probe::limitClamped(): Integer[1]
{
    let r = #TDS
       val
       1
       2
       3
    #;
    $r->limit(99)->size()
}";
    let result = eval_function(
        "limit_clamped.pure",
        source,
        &["relation_slice_probe", "limitClamped"],
    );
    assert_size(result, 3);
}

#[test]
fn limit_zero_returns_empty() {
    // limit 0 — header preserved, zero rows.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_slice_probe::limitZero(): Integer[1]
{
    let r = #TDS
       val
       1
       2
       3
    #;
    $r->limit(0)->size()
}";
    let result = eval_function(
        "limit_zero.pure",
        source,
        &["relation_slice_probe", "limitZero"],
    );
    assert_size(result, 0);
}

#[test]
fn limit_of_empty_tds_is_empty() {
    // Empty TDS, limit any positive count — result is still empty.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_slice_probe::limitOfEmpty(): Integer[1]
{
    let r = #TDS
       val
    #;
    $r->limit(5)->size()
}";
    let result = eval_function(
        "limit_of_empty.pure",
        source,
        &["relation_slice_probe", "limitOfEmpty"],
    );
    assert_size(result, 0);
}

// ---------------------------------------------------------------------------
// drop(rel, n) — skip first n rows
// ---------------------------------------------------------------------------

#[test]
fn drop_two_leaves_one() {
    // Three-row TDS, drop 2 — result size 1.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_slice_probe::dropTwo(): Integer[1]
{
    let r = #TDS
       val
       1
       2
       3
    #;
    $r->drop(2)->size()
}";
    let result = eval_function(
        "drop_two.pure",
        source,
        &["relation_slice_probe", "dropTwo"],
    );
    assert_size(result, 1);
}

#[test]
fn drop_more_than_rows_is_empty() {
    // drop > size — result is empty.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_slice_probe::dropTooMany(): Integer[1]
{
    let r = #TDS
       val
       1
       2
       3
    #;
    $r->drop(10)->size()
}";
    let result = eval_function(
        "drop_too_many.pure",
        source,
        &["relation_slice_probe", "dropTooMany"],
    );
    assert_size(result, 0);
}

#[test]
fn drop_zero_is_unchanged() {
    // drop 0 — keeps every row.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_slice_probe::dropZero(): Integer[1]
{
    let r = #TDS
       val
       1
       2
       3
    #;
    $r->drop(0)->size()
}";
    let result = eval_function(
        "drop_zero.pure",
        source,
        &["relation_slice_probe", "dropZero"],
    );
    assert_size(result, 3);
}

// ---------------------------------------------------------------------------
// Composition — limit / drop chained, multi-column TDS, string columns
// ---------------------------------------------------------------------------

#[test]
fn limit_then_drop_composes() {
    // Five-row TDS, limit(4) then drop(2) — leaves 2 rows.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_slice_probe::limitThenDrop(): Integer[1]
{
    let r = #TDS
       val
       1
       2
       3
       4
       5
    #;
    $r->limit(4)->drop(2)->size()
}";
    let result = eval_function(
        "limit_then_drop.pure",
        source,
        &["relation_slice_probe", "limitThenDrop"],
    );
    assert_size(result, 2);
}

#[test]
fn drop_then_limit_on_multi_column_tds() {
    // Two-column TDS (Integer + String) — exercises the round-trip
    // through `render_csv_from_columns_and_rows` (string quoting + comma
    // separator) for both natives chained together.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_slice_probe::dropThenLimit(): Integer[1]
{
    let r = #TDS
       val, str
       1, a
       2, ewe
       3, qw
       4, wwe
       5, weq
    #;
    $r->drop(1)->limit(3)->size()
}";
    let result = eval_function(
        "drop_then_limit.pure",
        source,
        &["relation_slice_probe", "dropThenLimit"],
    );
    assert_size(result, 3);
}
