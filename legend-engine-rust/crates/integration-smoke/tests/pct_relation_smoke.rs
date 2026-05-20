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
// PCT tests — `filter`
// ---------------------------------------------------------------------------

#[test]
fn pct_filter_testSimpleFilterShared() {
    run_pct_test("meta::pure::functions::relation::tests::filter::testSimpleFilterShared");
}

#[test]
fn pct_filter_testSimpleFilter_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::filter::testSimpleFilter_MultipleExpressions",
    );
}

// ---------------------------------------------------------------------------
// PCT tests — `sort`
// ---------------------------------------------------------------------------

/// Currently fails: `$res->map(x|$x.id)` dispatches to
/// `meta::pure::functions::collection::map<T,V|m>(col:T[m],…)` instead
/// of `meta::pure::functions::relation::map<T,V>(rel:Relation<T>[1],…)`.
/// See `pct_size_testGroupBySize` and friends — same pure-side
/// narrowing-tiebreak gap (Named-vs-Generic param0 dominance).
#[ignore = "blocked on pure-side narrower preferring relation::map over collection::map for Relation-typed receiver"]
#[test]
fn pct_sort_testSimpleSortShared() {
    run_pct_test("meta::pure::functions::relation::tests::sort::testSimpleSortShared");
}

#[ignore = "uses $res->map(x|$x.col), same narrowing gap as pct_sort_testSimpleSortShared"]
#[test]
fn pct_sort_testSimpleSort_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::sort::testSimpleSort_MultipleExpressions",
    );
}

// ---------------------------------------------------------------------------
// PCT tests — `distinct`
// ---------------------------------------------------------------------------

#[test]
fn pct_distinct_testDistinctSingle() {
    run_pct_test("meta::pure::functions::relation::tests::distinct::testDistinctSingle");
}

#[test]
fn pct_distinct_testDistinctSingle_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::distinct::testDistinctSingle_MultipleExpressions",
    );
}

#[test]
fn pct_distinct_testDistinctMultiple() {
    run_pct_test("meta::pure::functions::relation::tests::distinct::testDistinctMultiple");
}

#[test]
fn pct_distinct_testDistinctMultiple_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::distinct::testDistinctMultiple_MultipleExpressions",
    );
}

#[test]
fn pct_distinct_testDistinctAll() {
    run_pct_test("meta::pure::functions::relation::tests::distinct::testDistinctAll");
}

#[test]
fn pct_distinct_testDistinctAll_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::distinct::testDistinctAll_MultipleExpressions",
    );
}

// ---------------------------------------------------------------------------
// PCT tests — `concatenate`
// ---------------------------------------------------------------------------

#[test]
fn pct_concatenate_testSimpleConcatenateShared() {
    run_pct_test(
        "meta::pure::functions::relation::tests::concatenate::testSimpleConcatenateShared",
    );
}

#[test]
fn pct_concatenate_testSimpleConcatenate_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::concatenate::testSimpleConcatenate_MultipleExpressions",
    );
}

// ---------------------------------------------------------------------------
// PCT tests — `rename`
// ---------------------------------------------------------------------------

#[test]
fn pct_rename_testSimpleRenameShared() {
    run_pct_test("meta::pure::functions::relation::tests::rename::testSimpleRenameShared");
}

#[test]
fn pct_rename_testSimpleRename_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::rename::testSimpleRename_MultipleExpressions",
    );
}

// ---------------------------------------------------------------------------
// PCT tests — `limit`
// ---------------------------------------------------------------------------

#[test]
fn pct_limit_testSimpleLimitShared() {
    run_pct_test("meta::pure::functions::relation::tests::limit::testSimpleLimitShared");
}

#[test]
fn pct_limit_testSimpleLimit_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::limit::testSimpleLimit_MultipleExpressions",
    );
}

// ---------------------------------------------------------------------------
// PCT tests — `drop`
// ---------------------------------------------------------------------------

#[test]
fn pct_drop_testSimpleDropShared() {
    run_pct_test("meta::pure::functions::relation::tests::drop::testSimpleDropShared");
}

#[test]
fn pct_drop_testSimpleDrop_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::drop::testSimpleDrop_MultipleExpressions",
    );
}

// ---------------------------------------------------------------------------
// PCT tests — `select`
// ---------------------------------------------------------------------------

#[test]
fn pct_select_testSingleColSelectShared() {
    run_pct_test("meta::pure::functions::relation::tests::select::testSingleColSelectShared");
}

#[test]
fn pct_select_testSingleColSelect_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::select::testSingleColSelect_MultipleExpressions",
    );
}

#[test]
fn pct_select_testMultiColsSelectShared() {
    run_pct_test("meta::pure::functions::relation::tests::select::testMultiColsSelectShared");
}

#[test]
fn pct_select_testMultiColsSelect_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::select::testMultiColsSelect_MultipleExpressions",
    );
}

/// `~'other kind'` — quoted column name carrying special chars (the
/// space in this case). Runtime fails with
/// `select: column ''other kind'' not present in receiver;
///  have ["val", "str", "other kind"]` — the looked-up name carries
/// the surrounding quotes (`'other kind'`) while the TDS column was
/// parsed without them (`other kind`).
///
/// The mismatch lives in the ColSpec literal lower: `~'name'` should
/// strip surrounding `'` to materialise just `name` as the `name`
/// slot. Header-parsing for the TDS literal does this correctly
/// (column appears as `other kind`); the ColSpec lower doesn't.
#[ignore = "pure-side: ColSpec lower for ~'name' should strip surrounding quotes from the name slot"]
#[test]
fn pct_select_testSingleSelectWithQuotedColumn() {
    run_pct_test(
        "meta::pure::functions::relation::tests::select::testSingleSelectWithQuotedColumn",
    );
}

#[ignore = "same quoted-name lowering gap as pct_select_testSingleSelectWithQuotedColumn"]
#[test]
fn pct_select_testSingleSelectWithQuotedColumn_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::select::testSingleSelectWithQuotedColumn_MultipleExpressions",
    );
}

#[test]
fn pct_select_testSelectAll() {
    run_pct_test("meta::pure::functions::relation::tests::select::testSelectAll");
}

#[test]
fn pct_select_testSelectAll_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::select::testSelectAll_MultipleExpressions",
    );
}

// ---------------------------------------------------------------------------
// PCT tests — `extend` (FuncColSpec only; OLAP variants need `over`/`_Window`)
// ---------------------------------------------------------------------------

/// Currently fails with `plus_String_MANY__String_1_: Type mismatch:
/// expected Object, got Integer`. Lambda body is
/// `$c.str->toOne() + $c.val->toOne()->toString()` — looks like the
/// `->toString()` postfix isn't applied to `$c.val->toOne()` before
/// `plus_String` dispatches; the right-hand arg arrives as Integer,
/// not String. Numeric extend variants (`* 2`, etc.) pass cleanly,
/// so the issue is specific to the `String + Numeric->toString()`
/// chain at the lambda body lowering / operator-precedence layer
/// (pure-side).
#[ignore = "pure-side: $c.val->toOne()->toString() chain doesn't reduce to String before plus dispatch"]
#[test]
fn pct_extend_testSimpleExtendStrShared() {
    run_pct_test("meta::pure::functions::relation::tests::extend::testSimpleExtendStrShared");
}

#[ignore = "same chain-to-String issue as pct_extend_testSimpleExtendStrShared"]
#[test]
fn pct_extend_testSimpleExtendStr_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::extend::testSimpleExtendStr_MultipleExpressions",
    );
}

#[test]
fn pct_extend_testSimpleExtendInt() {
    run_pct_test("meta::pure::functions::relation::tests::extend::testSimpleExtendInt");
}

#[test]
fn pct_extend_testSimpleExtendInt_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::extend::testSimpleExtendInt_MultipleExpressions",
    );
}

#[test]
fn pct_extend_testSimpleExtendFloat() {
    run_pct_test("meta::pure::functions::relation::tests::extend::testSimpleExtendFloat");
}

#[test]
fn pct_extend_testSimpleExtendFloat_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::extend::testSimpleExtendFloat_MultipleExpressions",
    );
}

#[test]
fn pct_extend_testSimpleMultipleColumns() {
    run_pct_test("meta::pure::functions::relation::tests::extend::testSimpleMultipleColumns");
}

#[test]
fn pct_extend_testSimpleMultipleColumns_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::extend::testSimpleMultipleColumns_MultipleExpressions",
    );
}

// ---------------------------------------------------------------------------
// PCT tests — `size`
// ---------------------------------------------------------------------------

#[test]
fn pct_size_testSimpleSize() {
    run_pct_test("meta::pure::functions::relation::tests::size::testSimpleSize");
}

#[test]
fn pct_size_testSimpleSize_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::size::testSimpleSize_MultipleExpressions",
    );
}

#[test]
fn pct_size_testComparisonOperationAfterSize() {
    run_pct_test(
        "meta::pure::functions::relation::tests::size::testComparisonOperationAfterSize",
    );
}

#[test]
fn pct_size_testComparisonOperationAfterSize_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::size::testComparisonOperationAfterSize_MultipleExpressions",
    );
}

// ---------------------------------------------------------------------------
// PCT tests — `eval(ColSpec, row)`
// ---------------------------------------------------------------------------

/// `eval(ColSpec, row)` is Pure-defined in `eval.pure:18` and its body
/// walks `$col->genericType().typeArguments->at(0).rawType->toOne()
///   ->cast(@RelationType<Any>).columns->toOne()
///   ->cast(@Column<Nil,Z|0..1>)->eval($row)`.
///
/// Currently fails with
/// `The system is trying to get an element at offset 0 where the
///  collection is of size 0` — the reflection chain expects the
/// ColSpec's `classifierGenericType.typeArguments[0]` to have a
/// non-empty `columns` slot, but the heap-allocated ColSpec built by
/// `alloc_col_spec_literal` doesn't carry it (the inner
/// `RelationType` shape isn't materialised).
///
/// Either the platform reflection chain is too strict for our heap
/// shape, or our `alloc_col_spec_literal` needs to populate the
/// inner-RelationType.columns chain. Both are tractable but live
/// upstream / span pure+engine boundary.
#[ignore = "eval(ColSpec, row) reflection chain hits empty columns slot; needs ColSpec heap-shape work or Pure-body simplification"]
#[test]
fn pct_eval_testSimpleEval() {
    run_pct_test("meta::pure::functions::relation::tests::eval::testSimpleEval");
}
