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

//! Runtime smoke tests for `meta::pure::functions::relation::sort` and
//! its `ascending` / `descending` `SortInfo` constructors.
//!
//! Each test compiles a small synthetic Pure source against the embedded
//! `core_functions_*` corpus, evaluates a function that returns either
//! the sorted relation's row count (via `size`) or its canonical CSV
//! (via the `TDS.csv` slot read), and asserts on the returned value.
//!
//! The asserted CSV is what the runtime's
//! `render_csv_from_columns_and_rows` helper emits — `, `-separated
//! header and rows, single-`\n` row separator, no `#TDS\n…\n#`
//! wrapping. That format is *not* the platform's `toString` output;
//! comparing to the platform format would require porting Java-Pure's
//! TDS pretty-printer into the runtime, which is out of scope for sort.

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
        name: "relation_sort_probe",
        pattern: ".*",
        dependencies: USER_DEPS,
    };
    repos.push(Repo::Filesystem {
        prefix: "/relation_sort_probe".into(),
        files: vec![OwnedSourceFile {
            path: format!("/relation_sort_probe/{name}"),
            content: source.into(),
        }],
        meta: Some(meta),
    });
    repos
}

fn compile_user(name: &str, source: &str) -> PureModel {
    let repos = compose_repos_with_user_source(name, source);
    let auto_imports = auto_imports();
    let url = format!("/relation_sort_probe/{name}");

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

/// Pull a `String[1]` result out of a `Value` returned by `eval_function`.
fn as_string(v: &Value) -> &str {
    match v {
        Value::String(s) => s.as_str(),
        other => panic!("expected Value::String, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Row-count probes — sort preserves row count.
// ---------------------------------------------------------------------------

#[test]
fn sort_ascending_orders_rows_by_size() {
    // Sanity-check the size invariant before reading the CSV.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_sort_probe::sortAscSize(): Integer[1]
{
    let r = #TDS
       val
       3
       1
       2
    #;
    $r->sort(ascending(~val))->size()
}";
    let result = eval_function(
        "sort_asc_size.pure",
        source,
        &["relation_sort_probe", "sortAscSize"],
    );
    assert!(
        matches!(result, Value::Integer(3)),
        "expected Integer(3), got {result:?}",
    );
}

// ---------------------------------------------------------------------------
// Canonical-CSV probes — read `$result.csv` directly.
// ---------------------------------------------------------------------------

#[test]
fn sort_ascending_orders_rows() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_sort_probe::sortAscCsv(): String[1]
{
    let r = #TDS
       val
       3
       1
       2
    #;
    $r->sort(ascending(~val)).csv
}";
    let result = eval_function(
        "sort_asc_csv.pure",
        source,
        &["relation_sort_probe", "sortAscCsv"],
    );
    assert_eq!(as_string(&result), "val\n1\n2\n3");
}

#[test]
fn sort_descending_reverses() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_sort_probe::sortDescCsv(): String[1]
{
    let r = #TDS
       val
       3
       1
       2
    #;
    $r->sort(descending(~val)).csv
}";
    let result = eval_function(
        "sort_desc_csv.pure",
        source,
        &["relation_sort_probe", "sortDescCsv"],
    );
    assert_eq!(as_string(&result), "val\n3\n2\n1");
}

#[test]
fn sort_multi_key_asc_then_desc() {
    // Two-column TDS: id ascending as major key, name descending as
    // tiebreaker. With three id-1 rows ("Sachin", "Neema", "Anna"),
    // descending name order yields Sachin > Neema > Anna.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_sort_probe::sortMultiKeyCsv(): String[1]
{
    let r = #TDS
       id, name
       2, George
       1, Sachin
       1, Neema
       1, Anna
       2, Alex
    #;
    $r->sort([ascending(~id), descending(~name)]).csv
}";
    let result = eval_function(
        "sort_multi_key.pure",
        source,
        &["relation_sort_probe", "sortMultiKeyCsv"],
    );
    assert_eq!(
        as_string(&result),
        "id, name\n1, 'Sachin'\n1, 'Neema'\n1, 'Anna'\n2, 'George'\n2, 'Alex'",
    );
}

// ---------------------------------------------------------------------------
// PCT-corpus probe — body of `testSimpleSortShared`'s first assertion.
// Asserts the same column extraction the platform test does (id values
// after sort), via an indirect check: row-count + first-row id.
// ---------------------------------------------------------------------------

#[test]
fn sort_pct_corpus_probe_simple_sort_shared() {
    // Mirrors the body of
    //   meta::pure::functions::relation::tests::sort::testSimpleSortShared
    // (the multi-key descending(id) + ascending(name) variant). Asserts
    // on the runtime-canonical CSV form.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_sort_probe::pctSortShared(): String[1]
{
    let r = #TDS
       id, name
       2, George
       3, Pierre
       1, Sachin
       1, Neema
       5, David
       4, Alex
       2, Thierry
    #;
    $r->sort([descending(~id), ascending(~name)]).csv
}";
    let result = eval_function(
        "sort_pct_corpus.pure",
        source,
        &["relation_sort_probe", "pctSortShared"],
    );
    assert_eq!(
        as_string(&result),
        "id, name\n5, 'David'\n4, 'Alex'\n3, 'Pierre'\n2, 'George'\n2, 'Thierry'\n1, 'Neema'\n1, 'Sachin'",
    );
}
