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

#[test]
fn pct_sort_testSimpleSortShared() {
    run_pct_test("meta::pure::functions::relation::tests::sort::testSimpleSortShared");
}

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

#[test]
fn pct_select_testSingleSelectWithQuotedColumn() {
    run_pct_test(
        "meta::pure::functions::relation::tests::select::testSingleSelectWithQuotedColumn",
    );
}

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

#[test]
fn pct_extend_testSimpleExtendStrShared() {
    run_pct_test("meta::pure::functions::relation::tests::extend::testSimpleExtendStrShared");
}

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
// PCT tests — `composition` (multi-native compositions in tests/composition.pure)
// Limited to tests whose body only uses natives we've implemented:
// filter, sort, distinct, extend, rename, select, limit, drop,
// concatenate, columns, size, ascending/descending, map, toString.
// Tests touching groupBy, project, pivot, join, asofjoin, lateral,
// window/olap, variant are not included.
// ---------------------------------------------------------------------------

/// Lambda body `$x.str->toOne() + $x.val->toOne()->toString()` inside an
/// `extend(~newCol:x|…)`, then `filter(x|$x.newCol == …)`. The FuncColSpec
/// init lambda's row param `$x` is now typed from the source relation, so
/// `$x.val` resolves to its column type and `toString` dispatches on the
/// scalar (not the `Relation` overload).
#[test]
fn pct_composition_testExtendFilter() {
    run_pct_test("meta::pure::functions::relation::tests::composition::testExtendFilter");
}

#[test]
fn pct_composition_test_Distinct_Filter() {
    run_pct_test("meta::pure::functions::relation::tests::composition::test_Distinct_Filter");
}

#[test]
fn pct_composition_testMixColumnNamesRenameFilter() {
    run_pct_test(
        "meta::pure::functions::relation::tests::composition::testMixColumnNamesRenameFilter",
    );
}

/// `#TDS->rename(~a,~b)->…->extend(~newCol:c|$c.<renamed>->toOne()->toString() + …)`.
/// The ColSpec init-lambda row typing is fixed, but the extend's *source* is a
/// Chained `rename`→`extend`→`rename`→`filter`→`rename`→`select`. The
/// `rename<T,Z,K,V>(…):Relation<T-Z+V>` type-algebra now resolves (the
/// `(?:K)⊆T` / `Z=…` wildcard binding + `T-Z+V` Union/Difference collapse are
/// implemented), so the three column renames and the first `extend` lambda
/// (`$c.col_one_num->toOne()->toString()`) type-check and run. The remaining
/// blocker is `extend`'s OWN `Relation<T+Z>`: the `FuncColSpec<{T->Any},Z>`
/// arg doesn't surface its output column `Z` (the new `newCol`, typed by the
/// init lambda's return type), so `T+Z` stays generic and the *next*
/// `rename(~newCol,~_new_col)` can't resolve `_new_col`'s type — `$x._new_col
/// ->toString()` then mis-dispatches to `toString_Relation`.
///
/// Fixed: `extend`'s `FuncColSpec<{T->Any},Z>` arg now binds `Z` to a
/// single-column `Relation` built from the init lambda's return type
/// (`colspec_binding_type` in legend-pure-rust `resolve.rs`), so `T+Z`
/// collapses to the augmented schema and the downstream `rename` resolves.
#[test]
fn pct_composition_testMixColumnNamesRenameExtend() {
    run_pct_test(
        "meta::pure::functions::relation::tests::composition::testMixColumnNamesRenameExtend",
    );
}

/// `extend(over(~p), agg)->filter(...)` — the windowed sum is expected to be
/// computed over the POST-filter rows (SQL `SUM(i) OVER (PARTITION BY p)` after
/// `WHERE o IS NOT NULL`): p=0 → 20 (=10+10), not 50. That filter-before-window
/// behaviour is a SQL-backend-only semantic.
///
/// Java's OWN interpreted AND compiled relation engines do NOT implement it —
/// they produce the full-partition sum (p=0 → 50), identical to our row-by-row
/// `extend_olap.rs`, and EXCLUDE this test:
///   - Test_Interpreted_RelationFunctions_PCT.java  (expected 20 / actual 50)
///   - Test_Compiled_RelationFunctions_PCT.java     (expected 20 / actual 50)
/// SQL backends that lack window support also exclude it (e.g. Spanner:
/// "Window Columns not supported").
///
/// So our engine is already Java-parity-correct; this stays excluded as a
/// SQL-only semantic, NOT a bug to fix. (See pure-agent diagnosis in scratch_6.)
#[ignore = "java-parity: filter-before-window is SQL-backend-only; Java interpreted+compiled also produce the full-partition sum and exclude this test"]
#[test]
fn pct_composition_testExtendFilterOutNull() {
    run_pct_test(
        "meta::pure::functions::relation::tests::composition::testExtendFilterOutNull",
    );
}

#[test]
fn pct_composition_testExtendAddOnNull() {
    run_pct_test("meta::pure::functions::relation::tests::composition::testExtendAddOnNull");
}

#[test]
fn pct_composition_testExtendJoinStringOnNull() {
    run_pct_test(
        "meta::pure::functions::relation::tests::composition::testExtendJoinStringOnNull",
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
/// Fixed in legend-pure-rust: `genericType()` now reflects a ColSpec's stored
/// `classifierGenericType` (so `$col->genericType().typeArguments` resolves to
/// the inner `RelationType`), and a `Column` — which extends `Function` —
/// evaluates against the row tuple, so the final `Column.eval($row)` reads the
/// cell.
#[test]
fn pct_eval_testSimpleEval() {
    run_pct_test("meta::pure::functions::relation::tests::eval::testSimpleEval");
}

// ---------------------------------------------------------------------------
// PCT tests — OLAP `over(_, sortInfo, frame)` with extend(_,_Window,AggColSpec).
//
// Each test uses `extend(over(~p, [~o,~i], rows(M, N)), ~newCol:{...}:y|$y->plus())`,
// then string-compares the result via `assertEquals(expected_string,
// $res->sort(...)->toString())`. The assertion path is pure-string —
// no `columns().classifierGenericType` reflection, no `assertTdsEquivalent`,
// so neither the find_by_prefix collision nor the Column-reflection
// gap blocks these.
// ---------------------------------------------------------------------------

#[test]
fn pct_over_testRows_UnboundedPreceding_CurrentRow() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRows_UnboundedPreceding_CurrentRow",
    );
}

#[test]
fn pct_over_testRows_CurrentRow_UnboundedFollowing() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRows_CurrentRow_UnboundedFollowing",
    );
}

#[test]
fn pct_over_testRows_UnboundedPreceding_UnboundedFollowing() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRows_UnboundedPreceding_UnboundedFollowing",
    );
}

#[test]
fn pct_over_testRows_NPreceding_NPreceding() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRows_NPreceding_NPreceding",
    );
}

#[test]
fn pct_over_testRows_NPreceding_NFollowing() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRows_NPreceding_NFollowing",
    );
}

#[test]
fn pct_over_testRows_NFollowing_NFollowing() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRows_NFollowing_NFollowing",
    );
}

#[test]
fn pct_over_testRows_UnboundedPreceding_NPreceding() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRows_UnboundedPreceding_NPreceding",
    );
}

#[test]
fn pct_over_testRows_UnboundedPreceding_NFollowing() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRows_UnboundedPreceding_NFollowing",
    );
}

#[test]
fn pct_over_testRows_NPreceding_UnboundedFollowing() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRows_NPreceding_UnboundedFollowing",
    );
}

#[test]
fn pct_over_testRows_NFollowing_UnboundedFollowing() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRows_NFollowing_UnboundedFollowing",
    );
}

#[test]
fn pct_over_testRows_CurrentRow_CurrentRow() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRows_CurrentRow_CurrentRow",
    );
}

#[test]
fn pct_over_testRows_NPreceding_CurrentRow() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRows_NPreceding_CurrentRow",
    );
}

#[test]
fn pct_over_testRows_CurrentRow_NFollowing() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRows_CurrentRow_NFollowing",
    );
}

#[test]
fn pct_over_testRows_UnboundedPreceding_UnboundedFollowing_WithSinglePartition_WithoutOrderBy() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRows_UnboundedPreceding_UnboundedFollowing_WithSinglePartition_WithoutOrderBy",
    );
}

#[test]
fn pct_over_testRows_UnboundedPreceding_UnboundedFollowing_WithMultiplePartitions_WithoutOrderBy() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRows_UnboundedPreceding_UnboundedFollowing_WithMultiplePartitions_WithoutOrderBy",
    );
}

#[test]
fn pct_over_testRows_UnboundedPreceding_CurrentRow_WithMultiplePartitions_WithSingleOrderBy() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRows_UnboundedPreceding_CurrentRow_WithMultiplePartitions_WithSingleOrderBy",
    );
}

// ---------------------------------------------------------------------------
// PCT tests — OLAP `reduce` (standalone reduce inside extend(_,_Window,FuncColSpec))
//
// Each test uses `extend(over(~p, sortInfo, frame),
//     ~newCol:{p,w,r|reduce($p, $w, $r, mapFn, aggFn)})`,
// then `assertTdsEquivalent($expected, $res->sort(...), 0.00001)`.
// The path now works because:
//   * `find_by_prefix` prefers `map_T_*` over `map_Relation_*` (the
//     legend-pure-rust fix in commit 24df3f16a46), so the `.name`
//     auto-map inside tdsEquivalent dispatches to platform map.
//   * `Column.classifierGenericType` is populated by the refactored
//     `Columns` native using `legend_pure_runtime::relation::
//     alloc_multiplicity` + the canonical Column shape — the
//     reflection walk `$col.classifierGenericType.typeArguments
//     ->at(1).rawType->toOne()->subTypeOf(Number)` resolves.
// ---------------------------------------------------------------------------

#[test]
fn pct_reduce_testRows_UnboundedPreceding_CurrentRow() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRows_UnboundedPreceding_CurrentRow",
    );
}

#[test]
fn pct_reduce_testRows_CurrentRow_UnboundedFollowing() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRows_CurrentRow_UnboundedFollowing",
    );
}

#[test]
fn pct_reduce_testRows_UnboundedPreceding_UnboundedFollowing() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRows_UnboundedPreceding_UnboundedFollowing",
    );
}

#[test]
fn pct_reduce_testRows_NPreceding_NPreceding() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRows_NPreceding_NPreceding",
    );
}

#[test]
fn pct_reduce_testRows_NPreceding_NFollowing() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRows_NPreceding_NFollowing",
    );
}

#[test]
fn pct_reduce_testRows_NFollowing_NFollowing() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRows_NFollowing_NFollowing",
    );
}

#[test]
fn pct_reduce_testRows_UnboundedPreceding_NPreceding() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRows_UnboundedPreceding_NPreceding",
    );
}

#[test]
fn pct_reduce_testRows_UnboundedPreceding_NFollowing() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRows_UnboundedPreceding_NFollowing",
    );
}

#[test]
fn pct_reduce_testRows_NPreceding_UnboundedFollowing() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRows_NPreceding_UnboundedFollowing",
    );
}

#[test]
fn pct_reduce_testRows_NFollowing_UnboundedFollowing() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRows_NFollowing_UnboundedFollowing",
    );
}

#[test]
fn pct_reduce_testRows_CurrentRow_CurrentRow() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRows_CurrentRow_CurrentRow",
    );
}

#[test]
fn pct_reduce_testRows_NPreceding_CurrentRow() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRows_NPreceding_CurrentRow",
    );
}

#[test]
fn pct_reduce_testRows_CurrentRow_NFollowing() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRows_CurrentRow_NFollowing",
    );
}

#[test]
fn pct_reduce_testRows_UnboundedPreceding_UnboundedFollowing_WithSinglePartition_WithoutOrderBy() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRows_UnboundedPreceding_UnboundedFollowing_WithSinglePartition_WithoutOrderBy",
    );
}

#[test]
fn pct_reduce_testRows_UnboundedPreceding_UnboundedFollowing_WithMultiplePartitions_WithoutOrderBy() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRows_UnboundedPreceding_UnboundedFollowing_WithMultiplePartitions_WithoutOrderBy",
    );
}

#[test]
fn pct_reduce_testRows_UnboundedPreceding_CurrentRow_WithMultiplePartitions_WithSingleOrderBy() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRows_UnboundedPreceding_CurrentRow_WithMultiplePartitions_WithSingleOrderBy",
    );
}

// ---------------------------------------------------------------------------
// PCT tests — OLAP ranking natives (rowNumber / rank / denseRank /
// percentRank / cumulativeDistribution / ntile).
//
// Each test is `extend(over(~grp, ~id->descending()),
//     ~other:{p,w,r| $p->rowNumber($r)})` (or the rank/etc. analogue).
// The FuncColSpec extend passes the sorted partition sub-TDS as `$p`
// and a row tuple carrying its within-partition position; the native
// reads that position (+ sort columns from `$w`) and reproduces the
// Java `TestTDS` ranking formulas. Assertions are `assertEquals` on
// `toString()`, so no `assertTdsEquivalent` reflection chain.
// ---------------------------------------------------------------------------

#[test]
fn pct_rowNumber_testOLAPWithPartitionAndRowNumber() {
    run_pct_test(
        "meta::pure::functions::relation::tests::rowNumber::testOLAPWithPartitionAndRowNumber",
    );
}

#[test]
fn pct_rank_testOLAPWithPartitionAndOrderRank() {
    run_pct_test(
        "meta::pure::functions::relation::tests::rank::testOLAPWithPartitionAndOrderRank",
    );
}

#[test]
fn pct_denseRank_testOLAPWithPartitionAndOrderDenseRank() {
    run_pct_test(
        "meta::pure::functions::relation::tests::denseRank::testOLAPWithPartitionAndOrderDenseRank",
    );
}

#[test]
fn pct_percentRank_testOLAPWithPartitionAndOrderPercentRank() {
    run_pct_test(
        "meta::pure::functions::relation::tests::percentRank::testOLAPWithPartitionAndOrderPercentRank",
    );
}

#[test]
fn pct_cumulativeDistribution_testOLAPWithPartitionAndOrderCummulativeDistribution() {
    run_pct_test(
        "meta::pure::functions::relation::tests::cumulativeDistribution::testOLAPWithPartitionAndOrderCummulativeDistribution",
    );
}

#[test]
fn pct_ntile_testOLAPWithPartitionAndOrderNTile() {
    run_pct_test(
        "meta::pure::functions::relation::tests::ntile::testOLAPWithPartitionAndOrderNTile",
    );
}

// ---------------------------------------------------------------------------
// PCT tests — OLAP multi-column aggregate window
// (`extend(over(...), ~[name:map:reduce, name:map:reduce])` →
//  AggColSpecArray window variant).
// ---------------------------------------------------------------------------

#[test]
fn pct_extend_testOLAPAggWithPartitionWindowMultipleColumns() {
    run_pct_test(
        "meta::pure::functions::relation::tests::extend::testOLAPAggWithPartitionWindowMultipleColumns",
    );
}

#[test]
fn pct_extend_testOLAPAggWithPartitionWindowMultipleColumns_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::extend::testOLAPAggWithPartitionWindowMultipleColumns_MultipleExpressions",
    );
}

#[test]
fn pct_extend_testOLAPAggWithPartitionAndOrderWindowMultipleColumns() {
    run_pct_test(
        "meta::pure::functions::relation::tests::extend::testOLAPAggWithPartitionAndOrderWindowMultipleColumns",
    );
}

#[test]
fn pct_extend_testOLAPAggWithPartitionAndOrderWindowMultipleColumns_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::extend::testOLAPAggWithPartitionAndOrderWindowMultipleColumns_MultipleExpressions",
    );
}

// ---------------------------------------------------------------------------
// PCT tests — slice / row-navigation family.
//
// `offset` (via Pure-defined `lag`/`lead`), `first`/`last`/`nth`
// (frame-relative, called from inside extend(over(...), ~col:{p,w,r|…}))
// and `slice` (whole-relation [start, stop) range). `drop`/`limit` are
// already covered above. `first`/`last`/`nth` rely on the SQL default
// running frame (ORDER BY present, no explicit frame).
// ---------------------------------------------------------------------------

#[test]
fn pct_offset_lag_testOLAPWithPartitionAndOrderWindowUsingLag() {
    run_pct_test(
        "meta::pure::functions::relation::tests::lag::testOLAPWithPartitionAndOrderWindowUsingLag",
    );
}

#[test]
fn pct_offset_lead_testOLAPWithPartitionAndOrderWindow() {
    run_pct_test(
        "meta::pure::functions::relation::tests::lead::testOLAPWithPartitionAndOrderWindow",
    );
}

#[test]
fn pct_first_testOLAPWithPartitionAndOrderFirstWindow() {
    run_pct_test(
        "meta::pure::functions::relation::tests::first::testOLAPWithPartitionAndOrderFirstWindow",
    );
}

#[test]
fn pct_last_testOLAPWithPartitionAndOrderLastWindow() {
    run_pct_test(
        "meta::pure::functions::relation::tests::last::testOLAPWithPartitionAndOrderLastWindow",
    );
}

#[test]
fn pct_nth_testOLAPWithPartitionAndOrderNthWindow() {
    run_pct_test(
        "meta::pure::functions::relation::tests::nth::testOLAPWithPartitionAndOrderNthWindow",
    );
}

#[test]
fn pct_slice_testSimpleSliceShared() {
    run_pct_test("meta::pure::functions::relation::tests::slice::testSimpleSliceShared");
}

#[test]
fn pct_slice_testSimpleSlice_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::slice::testSimpleSlice_MultipleExpressions",
    );
}

// ---------------------------------------------------------------------------
// PCT tests — OLAP multi-column FuncColSpecArray window
// (`extend(over(...), ~[newCol:{p,w,r|$p->lead($r).id}, other:{p,w,r|
//   $p->first($w,$r).name}])`). Unblocked now that the slice-family
// natives (lead/lag/first) exist.
// ---------------------------------------------------------------------------

#[test]
fn pct_extend_testOLAPWithPartitionAndOrderWindowMultipleColumns() {
    run_pct_test(
        "meta::pure::functions::relation::tests::extend::testOLAPWithPartitionAndOrderWindowMultipleColumns",
    );
}

#[test]
fn pct_extend_testOLAPWithPartitionAndOrderWindowMultipleColumns_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::extend::testOLAPWithPartitionAndOrderWindowMultipleColumns_MultipleExpressions",
    );
}

#[test]
fn pct_extend_testOLAPWithPartitionAndMultipleOrderWindowMultipleColumns() {
    run_pct_test(
        "meta::pure::functions::relation::tests::extend::testOLAPWithPartitionAndMultipleOrderWindowMultipleColumns",
    );
}

#[test]
fn pct_extend_testOLAPWithMultiplePartitionsAndOrderWindowMultipleColumns() {
    run_pct_test(
        "meta::pure::functions::relation::tests::extend::testOLAPWithMultiplePartitionsAndOrderWindowMultipleColumns",
    );
}

#[test]
fn pct_extend_testOLAPWithMultiplePartitionsAndOrderWindowMultipleColumns_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::extend::testOLAPWithMultiplePartitionsAndOrderWindowMultipleColumns_MultipleExpressions",
    );
}

// `filter(...)->extend(over(...))` — the filter runs *before* the
// window (filter->extend, not the SQL window-pushdown extend->filter
// case), so the windowed columns see the already-filtered relation.
#[test]
fn pct_extend_testOLAPWithPartitionAndMultipleOrderWindowMultipleColumnsWithFilter() {
    run_pct_test(
        "meta::pure::functions::relation::tests::extend::testOLAPWithPartitionAndMultipleOrderWindowMultipleColumnsWithFilter",
    );
}

// ---------------------------------------------------------------------------
// PCT tests — numeric `_range` window frames (value-based, single sort col).
// over.pure uses AggColSpec + assertEquals; reduce.pure uses the standalone
// reduce + assertTdsEquivalent. RangeInterval (date+duration) is deferred.
// ---------------------------------------------------------------------------

#[test]
fn pct_over_testRange_CurrentRow_UnboundedFollowing_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_CurrentRow_UnboundedFollowing_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_over_testRange_CurrentRow_UnboundedFollowing_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_CurrentRow_UnboundedFollowing_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_over_testRange_ExplicitOffsets_WithNullValues_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_ExplicitOffsets_WithNullValues_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_over_testRange_ExplicitOffsets_WithNullValues_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_ExplicitOffsets_WithNullValues_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_over_testRange_NFollowing_NFollowing_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_NFollowing_NFollowing_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_over_testRange_NFollowing_NFollowing_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_NFollowing_NFollowing_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_over_testRange_NFollowing_UnboundedFollowing_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_NFollowing_UnboundedFollowing_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_over_testRange_NFollowing_UnboundedFollowing_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_NFollowing_UnboundedFollowing_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_over_testRange_NPreceding_NFollowing_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_NPreceding_NFollowing_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_over_testRange_NPreceding_NFollowing_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_NPreceding_NFollowing_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_over_testRange_NPreceding_NPreceding_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_NPreceding_NPreceding_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_over_testRange_NPreceding_NPreceding_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_NPreceding_NPreceding_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_over_testRange_NPreceding_UnboundedFollowing_WithNullValues_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_NPreceding_UnboundedFollowing_WithNullValues_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_over_testRange_NPreceding_UnboundedFollowing_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_NPreceding_UnboundedFollowing_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_over_testRange_NPreceding_UnboundedFollowing_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_NPreceding_UnboundedFollowing_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_over_testRange_UnboundedPreceding_CurrentRow_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_UnboundedPreceding_CurrentRow_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_over_testRange_UnboundedPreceding_CurrentRow_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_UnboundedPreceding_CurrentRow_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_over_testRange_UnboundedPreceding_NFollowing_WithNullValues_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_UnboundedPreceding_NFollowing_WithNullValues_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_over_testRange_UnboundedPreceding_NFollowing_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_UnboundedPreceding_NFollowing_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_over_testRange_UnboundedPreceding_NFollowing_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_UnboundedPreceding_NFollowing_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_over_testRange_UnboundedPreceding_NPreceding_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_UnboundedPreceding_NPreceding_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_over_testRange_UnboundedPreceding_NPreceding_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_UnboundedPreceding_NPreceding_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_over_testRange_WithNumbers_CurrentRow_NFollowing_WithoutPartition_WithSingleOrderBy() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_WithNumbers_CurrentRow_NFollowing_WithoutPartition_WithSingleOrderBy",
    );
}

// `_range(0.5d, 2.5)` mixes a Decimal and a Float frame offset. The
// Decimal/Float comparison gap that blocked this was fixed in
// legend-pure-rust 473942bd926 (numeric_cmp now promotes across all
// Number subtypes via compare_values); the engine Range-frame native
// already handled the value range.
#[test]
fn pct_over_testRange_WithNumbers_NFollowing_NFollowing_WithoutPartition_WithSingleOrderBy() {
    run_pct_test(
        "meta::pure::functions::relation::tests::over::testRange_WithNumbers_NFollowing_NFollowing_WithoutPartition_WithSingleOrderBy",
    );
}

#[test]
fn pct_reduce_testRange_CurrentRow_UnboundedFollowing_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_CurrentRow_UnboundedFollowing_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_reduce_testRange_CurrentRow_UnboundedFollowing_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_CurrentRow_UnboundedFollowing_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_reduce_testRange_ExplicitOffsets_WithNullValues_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_ExplicitOffsets_WithNullValues_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_reduce_testRange_ExplicitOffsets_WithNullValues_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_ExplicitOffsets_WithNullValues_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_reduce_testRange_NFollowing_NFollowing_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_NFollowing_NFollowing_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_reduce_testRange_NFollowing_NFollowing_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_NFollowing_NFollowing_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_reduce_testRange_NFollowing_UnboundedFollowing_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_NFollowing_UnboundedFollowing_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_reduce_testRange_NFollowing_UnboundedFollowing_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_NFollowing_UnboundedFollowing_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_reduce_testRange_NPreceding_NFollowing_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_NPreceding_NFollowing_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_reduce_testRange_NPreceding_NFollowing_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_NPreceding_NFollowing_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_reduce_testRange_NPreceding_NPreceding_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_NPreceding_NPreceding_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_reduce_testRange_NPreceding_NPreceding_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_NPreceding_NPreceding_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_reduce_testRange_NPreceding_UnboundedFollowing_WithNullValues_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_NPreceding_UnboundedFollowing_WithNullValues_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_reduce_testRange_NPreceding_UnboundedFollowing_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_NPreceding_UnboundedFollowing_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_reduce_testRange_NPreceding_UnboundedFollowing_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_NPreceding_UnboundedFollowing_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_reduce_testRange_UnboundedPreceding_CurrentRow_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_UnboundedPreceding_CurrentRow_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_reduce_testRange_UnboundedPreceding_CurrentRow_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_UnboundedPreceding_CurrentRow_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_reduce_testRange_UnboundedPreceding_NFollowing_WithNullValues_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_UnboundedPreceding_NFollowing_WithNullValues_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_reduce_testRange_UnboundedPreceding_NFollowing_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_UnboundedPreceding_NFollowing_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_reduce_testRange_UnboundedPreceding_NFollowing_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_UnboundedPreceding_NFollowing_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_reduce_testRange_UnboundedPreceding_NPreceding_WithSinglePartition_WithOrderByASC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_UnboundedPreceding_NPreceding_WithSinglePartition_WithOrderByASC",
    );
}

#[test]
fn pct_reduce_testRange_UnboundedPreceding_NPreceding_WithSinglePartition_WithOrderByDESC() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_UnboundedPreceding_NPreceding_WithSinglePartition_WithOrderByDESC",
    );
}

#[test]
fn pct_reduce_testRange_WithNumbers_CurrentRow_NFollowing_WithoutPartition_WithSingleOrderBy() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_WithNumbers_CurrentRow_NFollowing_WithoutPartition_WithSingleOrderBy",
    );
}

// Same `_range(0.5d, 2.5)` Decimal/Float case as the over.pure
// counterpart above — unblocked by legend-pure-rust 473942bd926.
#[test]
fn pct_reduce_testRange_WithNumbers_NFollowing_NFollowing_WithoutPartition_WithSingleOrderBy() {
    run_pct_test(
        "meta::pure::functions::relation::tests::reduce::testRange_WithNumbers_NFollowing_NFollowing_WithoutPartition_WithSingleOrderBy",
    );
}


// ---------------------------------------------------------------------------
// PCT tests — `groupBy` (collapse to one row per group key). Four
// overloads: {ColSpec, ColSpecArray} group cols x {AggColSpec,
// AggColSpecArray} aggregates. IGNORED: the assertions normalise
// group order via `chunk(String,Integer)`, a platform string native
// not yet in the runtime — NOT a groupBy gap (the native is verified
// directly in olap_extend_smoke::group_by_*). Filed in scratch_5.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "blocked on platform `chunk(String,Integer)` native (test order-normalisation), not groupBy; see scratch_5"]
fn pct_groupBy_testSimpleGroupBy_SingleSingle() {
    run_pct_test(
        "meta::pure::functions::relation::tests::groupBy::testSimpleGroupBy_SingleSingle",
    );
}

#[test]
#[ignore = "blocked on platform `chunk(String,Integer)` native (test order-normalisation), not groupBy; see scratch_5"]
fn pct_groupBy_testSimpleGroupBy_SingleSingle_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::groupBy::testSimpleGroupBy_SingleSingle_MultipleExpressions",
    );
}

#[test]
#[ignore = "blocked on platform `chunk(String,Integer)` native (test order-normalisation), not groupBy; see scratch_5"]
fn pct_groupBy_testSimpleGroupBy_MultipleSingle() {
    run_pct_test(
        "meta::pure::functions::relation::tests::groupBy::testSimpleGroupBy_MultipleSingle",
    );
}

#[test]
#[ignore = "blocked on platform `chunk(String,Integer)` native (test order-normalisation), not groupBy; see scratch_5"]
fn pct_groupBy_testSimpleGroupBy_MultipleSingle_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::groupBy::testSimpleGroupBy_MultipleSingle_MultipleExpressions",
    );
}

#[test]
#[ignore = "blocked on platform `chunk(String,Integer)` native (test order-normalisation), not groupBy; see scratch_5"]
fn pct_groupBy_testSimpleGroupBy_SingleMultiple() {
    run_pct_test(
        "meta::pure::functions::relation::tests::groupBy::testSimpleGroupBy_SingleMultiple",
    );
}

#[test]
#[ignore = "blocked on platform `chunk(String,Integer)` native (test order-normalisation), not groupBy; see scratch_5"]
fn pct_groupBy_testSimpleGroupBy_SingleMultiple_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::groupBy::testSimpleGroupBy_SingleMultiple_MultipleExpressions",
    );
}

#[test]
#[ignore = "blocked on platform `chunk(String,Integer)` native (test order-normalisation), not groupBy; see scratch_5"]
fn pct_groupBy_testSimpleGroupBy_MultipleMultiple() {
    run_pct_test(
        "meta::pure::functions::relation::tests::groupBy::testSimpleGroupBy_MultipleMultiple",
    );
}

#[test]
#[ignore = "blocked on platform `chunk(String,Integer)` native (test order-normalisation), not groupBy; see scratch_5"]
fn pct_groupBy_testSimpleGroupBy_MultipleMultiple_MultipleExpressions() {
    run_pct_test(
        "meta::pure::functions::relation::tests::groupBy::testSimpleGroupBy_MultipleMultiple_MultipleExpressions",
    );
}


// ---------------------------------------------------------------------------
// PCT tests — `join` (nested-loop, INNER/LEFT/RIGHT/FULL). IGNORED: every
// join test's source TDS carries multi-word unquoted cells (`More George
// 1`, `More David`), which the TDS island-grammar parser strips to
// `MoreGeorge1` — a pure-side whitespace bug, NOT a join gap. The join
// native is verified directly across all four kinds in
// olap_extend_smoke::join_all_kinds_single_word. Filed in scratch_5.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "pure-side: TDS island-grammar strips internal whitespace from unquoted cells"]
fn pct_join_testSimpleJoinShared() {
    run_pct_test("meta::pure::functions::relation::tests::join::testSimpleJoinShared");
}

#[test]
#[ignore = "pure-side: TDS island-grammar strips internal whitespace from unquoted cells"]
fn pct_join_testSimpleJoin_MultipleExpressions() {
    run_pct_test("meta::pure::functions::relation::tests::join::testSimpleJoin_MultipleExpressions");
}

#[test]
#[ignore = "pure-side: TDS island-grammar strips internal whitespace from unquoted cells"]
fn pct_join_testJoin_forFailedJoinWhenNoRowsMatchJoinCondition() {
    run_pct_test("meta::pure::functions::relation::tests::join::testJoin_forFailedJoinWhenNoRowsMatchJoinCondition");
}

#[test]
#[ignore = "pure-side: TDS island-grammar strips internal whitespace from unquoted cells"]
fn pct_join_testFullJoin() {
    run_pct_test("meta::pure::functions::relation::tests::join::testFullJoin");
}

#[test]
#[ignore = "pure-side: TDS island-grammar strips internal whitespace from unquoted cells"]
fn pct_join_testRightJoin() {
    run_pct_test("meta::pure::functions::relation::tests::join::testRightJoin");
}

