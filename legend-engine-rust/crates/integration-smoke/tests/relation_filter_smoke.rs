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
//! Runtime smoke tests for `meta::pure::functions::relation::filter`.
//!
//! `filter` is the first lambda-per-row relation native: the predicate
//! receives `$x` bound to a row-tuple object and returns a `Boolean`.
//! These probes drive the native end-to-end through the same
//! repo-composition harness used by `relation_natives_smoke.rs`,
//! asserting on row count via the already-wired `size` native.
//!
//! Coverage:
//! - `filter_keeps_matching_rows` — predicate filters down a 3-row TDS.
//! - `filter_returns_empty_when_predicate_never_matches` — empty result.
//! - `filter_returns_all_when_predicate_always_matches` — identity case.
//! - `filter_with_string_column` — string-equality predicate.
//! - `filter_pct_corpus_probe_simple_filter` — mirrors the body of
//!   `meta::pure::functions::relation::tests::filter::testSimpleFilterShared`,
//!   asserting via `size()` rather than the (not-yet-wired) `toString`
//!   round-trip.

use legend_pure_core_platform::platform::PLATFORM_AUTO_IMPORTS;
use legend_pure_core_platform::repo::{self, OwnedSourceFile, Repo, RepoMeta};
use legend_pure_parser_pure::model::PureModel;
use legend_pure_runtime::eval::Evaluator;
use legend_pure_runtime::native::NativeRegistry;
use legend_pure_runtime::value::Value;
use smol_str::SmolStr;

// ---------------------------------------------------------------------------
// Test harness — mirrors `relation_natives_smoke.rs`.
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
        name: "relation_filter_probe",
        pattern: ".*",
        dependencies: USER_DEPS,
    };
    repos.push(Repo::Filesystem {
        prefix: "/relation_filter_probe".into(),
        files: vec![OwnedSourceFile {
            path: format!("/relation_filter_probe/{name}"),
            content: source.into(),
        }],
        meta: Some(meta),
    });
    repos
}

fn compile_user(name: &str, source: &str) -> PureModel {
    let repos = compose_repos_with_user_source(name, source);
    let auto_imports = auto_imports();
    let url = format!("/relation_filter_probe/{name}");

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

// ---------------------------------------------------------------------------
// filter(Relation<T>, Function<{T->Boolean}>) → Relation<T>
// ---------------------------------------------------------------------------

#[test]
fn filter_keeps_matching_rows() {
    // 3-row Integer column; predicate `$x.val > 1` retains rows 2 and 3.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_filter_probe::filterGreaterThanOne(): Integer[1]
{
    let r = #TDS
       val
       1
       2
       3
    #;
    $r->filter(x|$x.val > 1)->size()
}";
    let result = eval_function(
        "filter_keeps_matching.pure",
        source,
        &["relation_filter_probe", "filterGreaterThanOne"],
    );
    assert!(
        matches!(result, Value::Integer(2)),
        "expected Integer(2), got {result:?}",
    );
}

#[test]
fn filter_returns_empty_when_predicate_never_matches() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_filter_probe::filterAllRejected(): Integer[1]
{
    let r = #TDS
       val
       1
       2
       3
    #;
    $r->filter(x|$x.val < 0)->size()
}";
    let result = eval_function(
        "filter_empty_result.pure",
        source,
        &["relation_filter_probe", "filterAllRejected"],
    );
    assert!(
        matches!(result, Value::Integer(0)),
        "expected Integer(0), got {result:?}",
    );
}

#[test]
fn filter_returns_all_when_predicate_always_matches() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_filter_probe::filterIdentity(): Integer[1]
{
    let r = #TDS
       val
       1
       2
       3
    #;
    $r->filter(x|true)->size()
}";
    let result = eval_function(
        "filter_identity.pure",
        source,
        &["relation_filter_probe", "filterIdentity"],
    );
    assert!(
        matches!(result, Value::Integer(3)),
        "expected Integer(3), got {result:?}",
    );
}

#[test]
fn filter_with_string_column() {
    // Two-column TDS with quoted strings; filter on string equality.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_filter_probe::filterByName(): Integer[1]
{
    let r = #TDS
       id, name
       1, 'alice'
       2, 'bob'
       3, 'alice'
    #;
    $r->filter(x|$x.name == 'alice')->size()
}";
    let result = eval_function(
        "filter_string.pure",
        source,
        &["relation_filter_probe", "filterByName"],
    );
    assert!(
        matches!(result, Value::Integer(2)),
        "expected Integer(2), got {result:?}",
    );
}

#[test]
fn filter_pct_corpus_probe_simple_filter() {
    // Mirrors the body of
    // `meta::pure::functions::relation::tests::filter::testSimpleFilterShared`:
    //     #TDS val\n 1\n 3\n 4\n# -> filter(x|$x.val > 1)
    // The PCT test asserts on a `toString()` round-trip; this probe
    // asserts on row count instead, since the goal is to lock the
    // corpus shape (TDS literal + identical predicate) — the
    // toString-based shape comparison is a separate native's
    // responsibility.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_filter_probe::pctSimpleFilterShared(): Integer[1]
{
    let r = #TDS
       val
       1
       3
       4
    #;
    $r->filter(x|$x.val > 1)->size()
}";
    let result = eval_function(
        "filter_pct_simple.pure",
        source,
        &["relation_filter_probe", "pctSimpleFilterShared"],
    );
    assert!(
        matches!(result, Value::Integer(2)),
        "expected Integer(2), got {result:?}",
    );
}
