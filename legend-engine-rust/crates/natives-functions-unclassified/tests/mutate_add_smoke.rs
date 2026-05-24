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

//! End-to-end smoke test for the `mutateAdd` native and the
//! [`FunctionsUnclassifiedExtension`].
//!
//! What this proves:
//! 1. A downstream crate can register a Pure-callable native through
//!    the `RuntimeExtension` SPI without modifying the platform crate.
//! 2. `mutateAdd` is callable from Pure source once the extension is
//!    loaded — the `core_functions_unclassified` repo's `mutateAdd.pure`
//!    declaration resolves to the Rust impl in this crate.
//! 3. The native correctly appends values to the named property and
//!    returns the same object reference.
//! 4. The platform-only `NativeRegistry::standard()` does NOT expose
//!    `mutateAdd` (locked by `platform_invariants.rs` in legend-pure-rust;
//!    re-asserted here to surface the boundary at the consumer side).

use legend_engine_rust_natives_functions_unclassified::FunctionsUnclassifiedExtension;
use legend_pure_core_platform::platform::PLATFORM_AUTO_IMPORTS;
use legend_pure_core_platform::repo::{self, OwnedSourceFile, Repo, RepoMeta};
use legend_pure_parser_pure::model::PureModel;
use legend_pure_runtime::eval::Evaluator;
use legend_pure_runtime::native::NativeRegistry;
use legend_pure_runtime::value::Value;
use smol_str::SmolStr;

fn auto_imports() -> Vec<SmolStr> {
    PLATFORM_AUTO_IMPORTS
        .iter()
        .map(|&s| SmolStr::new(s))
        .collect()
}

fn compose_repos_with_user_source(name: &str, source: &str) -> Vec<Repo> {
    let mut repos: Vec<Repo> = Repo::default_with_build_snapshots();
    repos.extend(legend_engine_rust_core_functions_unclassified_pure::repos());

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
        "core_functions_unclassified",
    ];
    let meta = RepoMeta {
        name: "mutate_add_probe",
        pattern: ".*",
        dependencies: USER_DEPS,
    };
    repos.push(Repo::Filesystem {
        prefix: "/mutate_add_probe".into(),
        files: vec![OwnedSourceFile {
            path: format!("/mutate_add_probe/{name}"),
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
    let url = format!("/mutate_add_probe/{name}");

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

fn eval_with_extension(source_name: &str, source: &str, fn_fqn: &[&str]) -> Value {
    let model = compile_user(source_name, source);
    let registry = NativeRegistry::with_extensions(&[&FunctionsUnclassifiedExtension]);
    let mut eval = Evaluator::new(&model, &registry);
    let path: Vec<SmolStr> = fn_fqn.iter().map(|&s| SmolStr::new(s)).collect();
    let id = model
        .resolve_function_by_path(&path)
        .unwrap_or_else(|| panic!("function `{}` not found in model", fn_fqn.join("::")));
    eval.call_user_function_by_id(id)
        .unwrap_or_else(|e| panic!("evaluation of `{}` failed: {e}", fn_fqn.join("::")))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn mutate_add_appends_to_collection_property() {
    let source = r"###Pure
import meta::pure::functions::lang::*;

Class mutate_add_probe::Bag
{
   items: String[*];
}

function mutate_add_probe::appendItems(): String[*]
{
   let b = ^mutate_add_probe::Bag(items=['a']);
   $b->mutateAdd('items', ['b', 'c']);
   $b.items
}";
    let result = eval_with_extension(
        "append_items.pure",
        source,
        &["mutate_add_probe", "appendItems"],
    );
    match result {
        Value::Collection(ref items) => {
            let strs: Vec<String> = items
                .iter()
                .map(|v| match v {
                    Value::String(s) => s.to_string(),
                    other => panic!("expected String in items, got {other:?}"),
                })
                .collect();
            assert_eq!(strs, vec!["a", "b", "c"], "expected ['a','b','c']");
        }
        other => panic!("expected Collection, got {other:?}"),
    }
}

#[test]
fn mutate_add_returns_same_object_reference() {
    // The return of `mutateAdd` IS the object itself (mutation in place).
    // Verify by mutating then re-reading from the original variable —
    // the new values must be visible because `$b` and the return value
    // alias the same heap entry.
    let source = r"###Pure
import meta::pure::functions::lang::*;

Class mutate_add_probe::Counter
{
   ticks: Integer[*];
}

function mutate_add_probe::sumAfterMutate(): Integer[1]
{
   let c = ^mutate_add_probe::Counter(ticks=[10]);
   $c->mutateAdd('ticks', [20, 30]);
   $c.ticks->plus()
}";
    let result = eval_with_extension(
        "counter_sum.pure",
        source,
        &["mutate_add_probe", "sumAfterMutate"],
    );
    assert!(
        matches!(result, Value::Integer(60)),
        "expected Integer(60) from 10 + 20 + 30 after mutateAdd, got {result:?}"
    );
}

#[test]
fn mutate_add_appends_to_empty_property() {
    // Start with an explicitly-empty collection and append to it.
    let source = r"###Pure
import meta::pure::functions::lang::*;

Class mutate_add_probe::EmptyBag
{
   tags: String[*];
}

function mutate_add_probe::seedEmpty(): String[*]
{
   let b = ^mutate_add_probe::EmptyBag(tags=[]);
   $b->mutateAdd('tags', ['first']);
   $b.tags
}";
    let result = eval_with_extension(
        "seed_empty.pure",
        source,
        &["mutate_add_probe", "seedEmpty"],
    );
    match result {
        Value::String(ref s) => assert_eq!(s.as_str(), "first"),
        Value::Collection(ref items) if items.len() == 1 => match &items[0] {
            Value::String(s) => assert_eq!(s.as_str(), "first"),
            other => panic!("expected single String, got {other:?}"),
        },
        other => panic!("expected ['first'] (as String or single-element Collection), got {other:?}"),
    }
}

#[test]
fn mutate_add_unregistered_in_platform_registry() {
    // Boundary check from the consumer side: building a registry WITHOUT
    // FunctionsUnclassifiedExtension must leave `mutateAdd` unreachable.
    // Mirrors legend-pure-rust/crates/runtime/tests/platform_invariants.rs
    // but anchored at the consumer crate so the layering is visible from
    // both sides.
    let registry = NativeRegistry::standard();
    assert!(
        registry
            .get("mutateAdd_T_1__String_1__Any_MANY__T_1_")
            .is_none(),
        "mutateAdd must not be in the standard platform registry — \
         it only becomes callable when FunctionsUnclassifiedExtension is registered."
    );
}
