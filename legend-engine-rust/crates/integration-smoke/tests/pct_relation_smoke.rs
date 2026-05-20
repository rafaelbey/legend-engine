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

//! PCT (Pure Compatibility Tests) runner for `core_functions_relation`.
//!
//! Each `<<PCT.test>>`-annotated function in
//! `meta::pure::functions::relation::tests::**` takes a single
//! parameter `f:Function<{Function<{->T[m]}>[1]->T[m]}>[1]` — an
//! "eval-style" function that the platform PCT framework binds to a
//! specific store/adapter (Interpreted, H2, DuckDB, …). For the Rust
//! interpreted runtime, `f` is a passthrough closure `{g|$g->eval()}`:
//! it takes the test's no-arg expression-lambda and evaluates it.
//!
//! Each Rust test below compiles a thin no-arg wrapper:
//!
//! ```pure
//! function pct_run::N(): Boolean[1]
//! {
//!     meta::pure::functions::relation::tests::filter::testSimpleFilterShared(
//!         {g|$g->eval()}
//!     )
//! }
//! ```
//!
//! …then invokes it. The PCT body itself owns the assertion — it calls
//! `assertEquals(expected, actual)` against the result's `toString()`
//! shape. We assert `Value::Boolean(true)` (or surface the underlying
//! Pure-side panic).
//!
//! Most PCT tests in this corpus assert on `toString(Relation)` output.
//! That function is Pure-defined (`PCT.platformOnly`) and its body
//! chains through `map(Relation, Function)`, `eval(ColSpec, row)`,
//! `joinStrings`, and `s(Any, Type)`. Until those run cleanly through
//! our runtime (Phase B of the PCT wiring effort), most tests in this
//! file will fail at the assertion site with a `toString`-chain error.
//! That's the expected first-wave signal; each gap surfaces a
//! concrete native to implement or runtime path to validate.

use legend_pure_core_platform::platform::PLATFORM_AUTO_IMPORTS;
use legend_pure_core_platform::repo::{self, OwnedSourceFile, Repo, RepoMeta};
use legend_pure_parser_pure::model::PureModel;
use legend_pure_runtime::eval::Evaluator;
use legend_pure_runtime::native::NativeRegistry;
use legend_pure_runtime::value::Value;
use smol_str::SmolStr;

// ---------------------------------------------------------------------------
// Harness — synthesises a no-arg wrapper that calls the PCT test with
// an eval-passthrough `f`.
// ---------------------------------------------------------------------------

fn auto_imports() -> Vec<SmolStr> {
    PLATFORM_AUTO_IMPORTS
        .iter()
        .map(|&s| SmolStr::new(s))
        .collect()
}

fn compose(wrapper_source: &str) -> Vec<Repo> {
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
        name: "pct_run",
        pattern: ".*",
        dependencies: USER_DEPS,
    };
    repos.push(Repo::Filesystem {
        prefix: "/pct_run".into(),
        files: vec![OwnedSourceFile {
            path: "/pct_run/wrapper.pure".into(),
            content: wrapper_source.into(),
        }],
        meta: Some(meta),
        source_root: None,
    });
    repos
}

fn compile_user(source: &str) -> PureModel {
    let repos = compose(source);
    match repo::load(&repos, &auto_imports()) {
        Ok(model) => model,
        Err(p) => {
            let user_errors: Vec<_> = p
                .errors
                .iter()
                .filter(|e| e.source_info.source.as_str() == "/pct_run/wrapper.pure")
                .collect();
            if user_errors.is_empty() {
                return p.model;
            }
            panic!(
                "wrapper compile failed.\nUser errors ({}):\n{}",
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

/// Build the wrapper source for a single PCT test FQN.
/// Returns a Pure source that defines `pct_run::wrapper` as a no-arg
/// function invoking the test with an eval-passthrough lambda.
fn wrapper_source_for(pct_test_fqn: &str) -> String {
    format!(
        r"###Pure
import meta::pure::functions::relation::*;

function pct_run::wrapper(): Boolean[1]
{{
    {pct_test_fqn}({{g|$g->eval()}})
}}",
    )
}

/// Compile a PCT-test wrapper and run it. Asserts `Value::Boolean(true)`.
fn run_pct_test(pct_test_fqn: &str) {
    let source = wrapper_source_for(pct_test_fqn);
    let model = compile_user(&source);
    let registry = NativeRegistry::with_extensions(&[
        &legend_engine_rust_natives_functions_relation::RelationFunctionsExtension,
    ]);
    let mut eval = Evaluator::new(&model, &registry);
    let path: Vec<SmolStr> = ["pct_run", "wrapper"]
        .iter()
        .map(|&s| SmolStr::new(s))
        .collect();
    let id = model
        .resolve_function_by_path(&path)
        .unwrap_or_else(|| panic!("wrapper function not found in model"));
    let result = eval
        .call_user_function_by_id(id)
        .unwrap_or_else(|e| panic!("PCT test `{pct_test_fqn}` evaluation failed: {e}"));
    assert!(
        matches!(result, Value::Boolean(true)),
        "PCT test `{pct_test_fqn}` did not return Boolean(true); got {result:?}",
    );
}

// ---------------------------------------------------------------------------
// First-wave PCT tests — `filter`
// ---------------------------------------------------------------------------

/// Locks the simplest filter PCT shape. Expects to fail at the
/// `assertEquals(...->toString())` site until `toString(Relation)` and
/// its dependency chain are wired through the runtime (Phase B).
#[test]
fn pct_filter_testSimpleFilterShared() {
    run_pct_test("meta::pure::functions::relation::tests::filter::testSimpleFilterShared");
}

// ---------------------------------------------------------------------------
// First-wave PCT tests — `sort`
// ---------------------------------------------------------------------------

/// Currently fails because `$res->map(x|$x.id)` dispatches to
/// `meta::pure::functions::collection::map<T,V|m>(col:T[m], …):V[m]`
/// instead of
/// `meta::pure::functions::relation::map<T,V>(rel:Relation<T>[1], …):V[*]`.
///
/// $res's resolved type is `Relation<{id:Integer, name:String}>[1]`
/// (the upstream Z-propagation fix binds it concretely). Both
/// overloads then survive Phase-1 of `narrow_candidates_by_type`:
///
/// - **`relation::map`** — param0=`Relation<T>[1]`. Direct nominal match.
/// - **`collection::map`** — param0=`T[m]` (Generic). The Generic
///   permissive arm of `is_type_compatible` returns true unconditionally,
///   so this overload also passes Phase-1.
///
/// Phase-2 ranks both candidates with the same score; Phase-3
/// most-specific-parameter elimination can't compare a Named param to
/// a Generic param, so neither dominates and the dispatcher picks the
/// wrong one (or it's order-dependent and we're losing).
///
/// **Concern for the pure-side expert** (no engine fix possible): the
/// narrower needs a tiebreak rule that prefers the overload whose
/// param0 is a concrete `Named` over the overload whose param0 is a
/// `Generic`, when the arg's resolved type is concretely `Named` and
/// the candidates' element ids would actually accept it. Similar shape
/// to the sort/extend narrowing fix that landed via the Z-prop
/// commits; this is the same root cause (Generic-vs-Named overload
/// dominance) one level deeper in the dispatch ranking.
///
/// Once the narrower lands, this test should turn green without any
/// engine change — `MapRelation` is wired and registered as
/// `map_Relation_1__Function_1__V_MANY_`.
#[ignore = "blocked on pure-side narrower preferring relation::map over collection::map for Relation-typed receiver"]
#[test]
fn pct_sort_testSimpleSortShared() {
    run_pct_test("meta::pure::functions::relation::tests::sort::testSimpleSortShared");
}

// ---------------------------------------------------------------------------
// First-wave PCT tests — `distinct`
// ---------------------------------------------------------------------------

#[test]
fn pct_distinct_testDistinctSingle() {
    run_pct_test("meta::pure::functions::relation::tests::distinct::testDistinctSingle");
}

// ---------------------------------------------------------------------------
// First-wave PCT tests — `size`
// ---------------------------------------------------------------------------

#[test]
fn pct_size_testSimpleSize() {
    run_pct_test("meta::pure::functions::relation::tests::size::testSimpleSize");
}
