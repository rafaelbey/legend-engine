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
//! Runtime smoke tests for relation native `rename` — relabel a single
//! TDS column. Mirrors the harness used by `relation_set_smoke.rs` /
//! `relation_filter_smoke.rs`.
//!
//! Verification shape:
//!
//! - Happy path: rename `a -> newName` on a `(a, b)` TDS, then run a
//!   second pipeline stage that consumes the relation by the new
//!   column name (`filter` on `~newName` — only succeeds if `rename`
//!   actually replaced the header). We assert on the row count
//!   surviving the post-rename filter, which is decoupled from any
//!   `toString`/`columns()`/CSV-rendering contract.
//! - Error paths: unknown source column / collision with an existing
//!   column → evaluator returns `Err`; we assert via `try_eval_function`.

use legend_pure_core_platform::platform::PLATFORM_AUTO_IMPORTS;
use legend_pure_core_platform::repo::{self, OwnedSourceFile, Repo, RepoMeta};
use legend_pure_parser_pure::model::PureModel;
use legend_pure_runtime::error::PureException;
use legend_pure_runtime::eval::Evaluator;
use legend_pure_runtime::native::NativeRegistry;
use legend_pure_runtime::value::Value;
use smol_str::SmolStr;

// ---------------------------------------------------------------------------
// Test harness — mirrors `relation_set_smoke.rs`
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
        "platform_dsl_diagram",
        "platform_dsl_graph",
        "platform_dsl_path",
        "platform_dsl_tds",
        "core_functions_json",
        "core_functions_unclassified",
        "core_functions_variant",
        "core_functions_relation",
        "core_functions_standard",
    ];
    let meta = RepoMeta {
        name: "relation_rename_probe",
        pattern: ".*",
        dependencies: USER_DEPS,
    };
    repos.push(Repo::Filesystem {
        prefix: "/relation_rename_probe".into(),
        files: vec![OwnedSourceFile {
            path: format!("/relation_rename_probe/{name}"),
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
    let url = format!("/relation_rename_probe/{name}");
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
    let registry = NativeRegistry::with_extensions(&[&legend_engine_rust_natives_functions_relation::RelationFunctionsExtension]);
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
    let registry = NativeRegistry::with_extensions(&[&legend_engine_rust_natives_functions_relation::RelationFunctionsExtension]);
    let mut eval = Evaluator::new(&model, &registry);
    let path: Vec<SmolStr> = fn_fqn.iter().map(|&s| SmolStr::new(s)).collect();
    let id = model
        .resolve_function_by_path(&path)
        .unwrap_or_else(|| panic!("function `{}` not found in model", fn_fqn.join("::")));
    eval.call_user_function_by_id(id)
}

// ---------------------------------------------------------------------------
// Happy paths
// ---------------------------------------------------------------------------

/// Rename `val -> newVal` on a 5-row TDS, then verify post-rename row
/// count is preserved (cells survive untouched).
#[test]
fn rename_one_column_preserves_data() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_rename_probe::renameSize(): Integer[1]
{
    let r = #TDS
       val
       1
       2
       3
       4
       5
    #;
    $r->rename(~val, ~newVal)->size()
}";
    let result = eval_function(
        "rename_size.pure",
        source,
        &["relation_rename_probe", "renameSize"],
    );
    assert!(
        matches!(result, Value::Integer(5)),
        "expected Integer(5) (rows preserved across rename), got {result:?}",
    );
}

/// Stronger check: filter on the **renamed** column. This only succeeds
/// if `rename` actually relabelled the header — `filter`'s predicate
/// resolves `$x.newVal` against the post-rename schema. With the source
/// data `[1,2,3]` and predicate `$x.newVal > 1`, two rows survive.
#[test]
fn rename_then_filter_uses_new_column_name() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_rename_probe::renameThenFilter(): Integer[1]
{
    let r = #TDS
       val
       1
       2
       3
    #;
    $r->rename(~val, ~newVal)->filter(x|$x.newVal > 1)->size()
}";
    let result = eval_function(
        "rename_then_filter.pure",
        source,
        &["relation_rename_probe", "renameThenFilter"],
    );
    assert!(
        matches!(result, Value::Integer(2)),
        "expected Integer(2) (rows where renamed col > 1), got {result:?}",
    );
}

// PCT-corpus probe — hand-translated `testSimpleRenameShared` from
// `relation/functions/transformation/rename.pure`. The Java assertion is
// on `->sort(~val->ascending())->toString()` (not yet wired in our
// runtime); we assert on row count instead — rename must preserve all
// 5 rows of the source.
#[test]
fn rename_pct_simple_shared_size_is_five() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_rename_probe::renameSharedSize(): Integer[1]
{
    let r = #TDS
       val, str
       1, a
       3, ewe
       4, qw
       5, wwe
       6, weq
    #;
    $r->rename(~str, ~newStr)->size()
}";
    let result = eval_function(
        "rename_shared_pct.pure",
        source,
        &["relation_rename_probe", "renameSharedSize"],
    );
    assert!(
        matches!(result, Value::Integer(5)),
        "expected Integer(5) (PCT testSimpleRenameShared row count), got {result:?}",
    );
}

// ---------------------------------------------------------------------------
// Error paths
// ---------------------------------------------------------------------------

#[test]
fn rename_unknown_source_errors() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_rename_probe::renameUnknown(): Integer[1]
{
    let r = #TDS
       val
       1
    #;
    $r->rename(~missing, ~x)->size()
}";
    let err = try_eval_function(
        "rename_unknown.pure",
        source,
        &["relation_rename_probe", "renameUnknown"],
    )
    .expect_err("expected rename to error on unknown source column");
    let msg = format!("{err}");
    assert!(
        msg.contains("rename") && msg.contains("missing"),
        "expected error to mention 'rename' and the missing column name, got: {msg}",
    );
}

#[test]
fn rename_to_collision_errors() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_rename_probe::renameCollide(): Integer[1]
{
    let r = #TDS
       a, b
       1, 2
       3, 4
    #;
    $r->rename(~a, ~b)->size()
}";
    let err = try_eval_function(
        "rename_collision.pure",
        source,
        &["relation_rename_probe", "renameCollide"],
    )
    .expect_err("expected rename to error on target-name collision");
    let msg = format!("{err}");
    assert!(
        msg.contains("rename") && msg.contains("already exists"),
        "expected collision error mentioning 'rename' and 'already exists', got: {msg}",
    );
}
