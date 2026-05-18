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

//! Runtime smoke tests for relation natives that operate on TDS heap
//! instances. Each test compiles a small synthetic Pure source against
//! the embedded `core_functions_*` corpus, evaluates a function via
//! the standard runtime registry, and asserts on the returned value.
//!
//! The relation natives the runtime ships (`addColumns`, `stringToTDS`)
//! work over the metaclass shape of a relation. The new `size` native
//! is the first to read a `TDS<…>` heap object's row data — by
//! reparsing the canonical `csv` slot — and forms the template for
//! upcoming row-iterating natives (`filter`, `head`, `limit`, etc.).

use legend_pure_core_platform::platform::PLATFORM_AUTO_IMPORTS;
use legend_pure_core_platform::repo::{self, OwnedSourceFile, Repo, RepoMeta};
use legend_pure_parser_pure::model::PureModel;
use legend_pure_runtime::eval::Evaluator;
use legend_pure_runtime::native::NativeRegistry;
use legend_pure_runtime::value::Value;
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
/// synthetic `/relation_native_probe` repo carrying the user-source
/// `.pure` file with the expression we want to evaluate.
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
        name: "relation_native_probe",
        pattern: ".*",
        dependencies: USER_DEPS,
    };
    repos.push(Repo::Filesystem {
        prefix: "/relation_native_probe".into(),
        files: vec![OwnedSourceFile {
            path: format!("/relation_native_probe/{name}"),
            content: source.into(),
        }],
        meta: Some(meta),
        source_root: None,
    });
    repos
}

/// Compile, returning the model. Panics with focused output on
/// user-source errors.
fn compile_user(name: &str, source: &str) -> PureModel {
    let repos = compose_repos_with_user_source(name, source);
    let auto_imports = auto_imports();
    let url = format!("/relation_native_probe/{name}");

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

/// Compile, evaluate the named function, and return the result Value.
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
// size(Relation<T>) → Integer
// ---------------------------------------------------------------------------

#[test]
fn size_of_three_row_tds_is_three() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_native_probe::sizeOfThree(): Integer[1]
{
    let r = #TDS
       val
       1
       2
       3
    #;
    $r->size()
}";
    let result = eval_function(
        "size_three.pure",
        source,
        &["relation_native_probe", "sizeOfThree"],
    );
    assert!(
        matches!(result, Value::Integer(3)),
        "expected Integer(3), got {result:?}",
    );
}

#[test]
fn size_of_empty_tds_is_zero() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function relation_native_probe::sizeOfEmpty(): Integer[1]
{
    let r = #TDS
       val
    #;
    $r->size()
}";
    let result = eval_function(
        "size_empty.pure",
        source,
        &["relation_native_probe", "sizeOfEmpty"],
    );
    assert!(
        matches!(result, Value::Integer(0)),
        "expected Integer(0), got {result:?}",
    );
}

#[test]
fn size_via_string_to_tds_call() {
    // Same semantics as the literal but exercises the `stringToTDS`
    // surface form. The two paths share `parse_and_infer`, so
    // `size` must agree on row count.
    let source = r"###Pure
function relation_native_probe::sizeViaNative(): Integer[1]
{
    let r = meta::pure::metamodel::relation::stringToTDS('val\n1\n2\n');
    $r->meta::pure::functions::relation::size()
}";
    let result = eval_function(
        "size_via_native.pure",
        source,
        &["relation_native_probe", "sizeViaNative"],
    );
    assert!(
        matches!(result, Value::Integer(2)),
        "expected Integer(2), got {result:?}",
    );
}
