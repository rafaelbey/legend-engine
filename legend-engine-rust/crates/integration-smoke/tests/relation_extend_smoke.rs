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

//! Runtime smoke tests for
//! `extend(Relation<T>[1], FuncColSpec<{T[1]->Any[0..1]},Z>[1])
//!   :Relation<T+Z>[1]`.
//!
//! Exercises the FuncColSpec lambda-preservation chain end-to-end:
//!
//! 1. Lowering captures the `~name:lam` init lambda in
//!    `RelationColumnLowered.init_lambda`.
//! 2. `eval` materialises the lambda as a `Value::Function` (capturing
//!    enclosing scope) and stores it on the runtime `FuncColSpec` heap
//!    object's `function` slot.
//! 3. The `extend` native re-reads the slot, invokes the lambda per
//!    source row (binding `$x` to a row-tuple object), and writes the
//!    return values into the appended column.
//!
//! Coverage asserts via downstream `size` and `columns` natives
//! (no `toString` round-trip) — the goal is to lock that the new
//! column shows up and the row count is preserved.

use legend_pure_core_platform::platform::PLATFORM_AUTO_IMPORTS;
use legend_pure_core_platform::repo::{self, OwnedSourceFile, Repo, RepoMeta};
use legend_pure_parser_pure::model::PureModel;
use legend_pure_runtime::eval::Evaluator;
use legend_pure_runtime::native::NativeRegistry;
use legend_pure_runtime::value::Value;
use smol_str::SmolStr;

// ---------------------------------------------------------------------------
// Test harness — mirrors `relation_filter_smoke.rs`.
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
        name: "relation_extend_probe",
        pattern: ".*",
        dependencies: USER_DEPS,
    };
    repos.push(Repo::Filesystem {
        prefix: "/relation_extend_probe".into(),
        files: vec![OwnedSourceFile {
            path: format!("/relation_extend_probe/{name}"),
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
    let url = format!("/relation_extend_probe/{name}");

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
    let registry = NativeRegistry::with_extensions(&[&legend_engine_rust_natives_functions_relation::RelationFunctionsExtension]);
    let mut eval = Evaluator::new(&model, &registry);
    let path: Vec<SmolStr> = fn_fqn.iter().map(|&s| SmolStr::new(s)).collect();
    let id = model
        .resolve_function_by_path(&path)
        .unwrap_or_else(|| panic!("function `{}` not found in model", fn_fqn.join("::")));
    eval.call_user_function_by_id(id)
        .unwrap_or_else(|e| panic!("evaluation of `{}` failed: {e}", fn_fqn.join("::")))
}

// ---------------------------------------------------------------------------
// extend(Relation, FuncColSpec) → Relation
// ---------------------------------------------------------------------------

#[test]
fn extend_preserves_row_count() {
    // Extending an n-row relation produces an n-row relation. Locks
    // the basic shape — the init lambda runs once per row, no rows
    // are added or dropped.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_extend_probe::extendSizeIs3(): Integer[1]
{
    let r = #TDS
       val
       1
       2
       3
    #;
    $r->extend(~doubled:c|$c.val->toOne() * 2)->size()
}";
    let result = eval_function(
        "extend_size.pure",
        source,
        &["relation_extend_probe", "extendSizeIs3"],
    );
    assert!(
        matches!(result, Value::Integer(3)),
        "expected Integer(3), got {result:?}",
    );
}

#[test]
fn extend_adds_one_column() {
    // After extending, `columns()` reports the original columns plus
    // the appended one. Two rows in, two-column TDS out (val + name).
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_extend_probe::extendAppendsColumn(): Integer[1]
{
    let r = #TDS
       val
       1
       2
    #;
    $r->extend(~name:c|'tag')->columns()->size()
}";
    let result = eval_function(
        "extend_appends_column.pure",
        source,
        &["relation_extend_probe", "extendAppendsColumn"],
    );
    assert!(
        matches!(result, Value::Integer(2)),
        "expected Integer(2) (val + name), got {result:?}",
    );
}

#[test]
fn extend_lambda_reads_source_row() {
    // The init lambda receives `$c` bound to the row-tuple, so
    // `$c.val` returns the value of `val` for that row. We assert
    // post-extend filter on the *new* column derived from `val`
    // returns the expected row count.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_extend_probe::extendThenFilterOnNewCol(): Integer[1]
{
    let r = #TDS
       val
       1
       2
       3
       4
    #;
    let e = $r->extend(~doubled:c|$c.val->toOne() * 2);
    $e->filter(x|$x.doubled > 4)->size()
}";
    let result = eval_function(
        "extend_then_filter.pure",
        source,
        &["relation_extend_probe", "extendThenFilterOnNewCol"],
    );
    // doubled = [2,4,6,8]; > 4 keeps {6,8} → 2 rows.
    assert!(
        matches!(result, Value::Integer(2)),
        "expected Integer(2), got {result:?}",
    );
}
