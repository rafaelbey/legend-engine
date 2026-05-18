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
//! Runtime smoke tests for relation set-natives `distinct` and
//! `concatenate`. Both produce a fresh `TDS` heap object whose canonical
//! `csv` slot reparses to the expected row count via
//! `meta::pure::functions::relation::size`. We assert on the size value
//! rather than the rendered TDS text so the tests stay decoupled from
//! the (still-evolving) renderer formatting.

use legend_pure_core_platform::platform::PLATFORM_AUTO_IMPORTS;
use legend_pure_core_platform::repo::{self, OwnedSourceFile, Repo, RepoMeta};
use legend_pure_parser_pure::model::PureModel;
use legend_pure_runtime::error::PureException;
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
        name: "relation_set_probe",
        pattern: ".*",
        dependencies: USER_DEPS,
    };
    repos.push(Repo::Filesystem {
        prefix: "/relation_set_probe".into(),
        files: vec![OwnedSourceFile {
            path: format!("/relation_set_probe/{name}"),
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
    let url = format!("/relation_set_probe/{name}");
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

/// Compile + evaluate; panics with focused output on any failure.
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

/// Compile + evaluate, returning the raw evaluator result (not panicking
/// on `Err`). Used by tests that assert on the error path.
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
// distinct
// ---------------------------------------------------------------------------

#[test]
fn distinct_drops_duplicate_rows() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_set_probe::distinctSize(): Integer[1]
{
    let r = #TDS
       val
       1
       2
       1
       3
    #;
    $r->distinct()->size()
}";
    let result = eval_function(
        "distinct_dups.pure",
        source,
        &["relation_set_probe", "distinctSize"],
    );
    assert!(
        matches!(result, Value::Integer(3)),
        "expected Integer(3), got {result:?}",
    );
}

#[test]
fn distinct_on_already_unique_is_noop() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_set_probe::distinctUniqueSize(): Integer[1]
{
    let r = #TDS
       val
       1
       2
       3
    #;
    $r->distinct()->size()
}";
    let result = eval_function(
        "distinct_unique.pure",
        source,
        &["relation_set_probe", "distinctUniqueSize"],
    );
    assert!(
        matches!(result, Value::Integer(3)),
        "expected Integer(3), got {result:?}",
    );
}

#[test]
fn distinct_on_empty_is_empty() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_set_probe::distinctEmptySize(): Integer[1]
{
    let r = #TDS
       val
    #;
    $r->distinct()->size()
}";
    let result = eval_function(
        "distinct_empty.pure",
        source,
        &["relation_set_probe", "distinctEmptySize"],
    );
    assert!(
        matches!(result, Value::Integer(0)),
        "expected Integer(0), got {result:?}",
    );
}

// PCT-corpus probe: hand-translated `testDistinctAll` (uses bare
// `distinct()`, not the `~[col]` overload). Original asserts on
// `->sort(...)->toString()`; since sort/toString isn't wired yet, we
// assert on row count instead — input has 5 rows with one duplicate row
// (`5,weq` repeated), so size after distinct is 4.
#[test]
fn distinct_all_pct_probe_size_is_four() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_set_probe::distinctAllSize(): Integer[1]
{
    let r = #TDS
      val, str
      1, a
      3, ewe
      1, qw
      5, weq
      5, weq
    #;
    $r->distinct()->size()
}";
    let result = eval_function(
        "distinct_all_pct.pure",
        source,
        &["relation_set_probe", "distinctAllSize"],
    );
    assert!(
        matches!(result, Value::Integer(4)),
        "expected Integer(4), got {result:?}",
    );
}

// ---------------------------------------------------------------------------
// concatenate
// ---------------------------------------------------------------------------

#[test]
fn concatenate_two_three_row_tds_yields_six_rows() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_set_probe::concatSize(): Integer[1]
{
    let a = #TDS
       val
       1
       2
       3
    #;
    let b = #TDS
       val
       4
       5
       6
    #;
    $a->concatenate($b)->size()
}";
    let result = eval_function(
        "concat_basic.pure",
        source,
        &["relation_set_probe", "concatSize"],
    );
    assert!(
        matches!(result, Value::Integer(6)),
        "expected Integer(6), got {result:?}",
    );
}

#[test]
fn concatenate_with_schema_mismatch_errors() {
    // Different column names — should fail at runtime with our
    // `concatenate: column 0 name mismatch …` error.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_set_probe::concatMismatch(): Integer[1]
{
    let a = #TDS
       val
       1
       2
    #;
    let b = #TDS
       other
       3
       4
    #;
    $a->concatenate($b)->size()
}";
    let err = try_eval_function(
        "concat_mismatch.pure",
        source,
        &["relation_set_probe", "concatMismatch"],
    )
    .expect_err("expected concatenate to error on schema mismatch");
    let msg = format!("{err}");
    assert!(
        msg.contains("concatenate") && msg.contains("mismatch"),
        "expected schema-mismatch error to mention 'concatenate' and 'mismatch', got: {msg}",
    );
}
