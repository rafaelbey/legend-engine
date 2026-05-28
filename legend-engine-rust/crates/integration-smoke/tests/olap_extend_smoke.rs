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

//! Smoke tests for the OLAP `extend(Relation, _Window, AggColSpec)`
//! native — partition by a column, map per row, reduce per partition,
//! append the per-row reduced value as a new column.
//!
//! These tests deliberately avoid the `null` literal in the source
//! TDS so they're independent of the pure-side TDS-parser concern
//! about `null` tokens being treated as `String("null")` rather than
//! empty cells. The PCT tests in `composition.pure` that use `null`
//! (`testExtendAddOnNull` / `testExtendFilterOutNull` /
//! `testExtendJoinStringOnNull`) stay `#[ignore]`d on that concern.

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

fn compose(name: &str, source: &str) -> Vec<Repo> {
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
        name: "olap_probe",
        pattern: ".*",
        dependencies: USER_DEPS,
    };
    repos.push(Repo::Filesystem {
        prefix: "/olap_probe".into(),
        files: vec![OwnedSourceFile {
            path: format!("/olap_probe/{name}"),
            content: source.into(),
        }],
        meta: Some(meta),
        source_root: None,
    });
    repos
}

fn compile_user(name: &str, source: &str) -> PureModel {
    let repos = compose(name, source);
    let url = format!("/olap_probe/{name}");
    match repo::load(&repos, &auto_imports()) {
        Ok(model) => model,
        Err(p) => {
            let user_errors: Vec<_> = p
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
    let registry = NativeRegistry::with_extensions(&[
        &legend_engine_rust_natives_functions_relation::RelationFunctionsExtension,
    ]);
    let mut eval = Evaluator::new(&model, &registry);
    let path: Vec<SmolStr> = fn_fqn.iter().map(|&s| SmolStr::new(s)).collect();
    let id = model
        .resolve_function_by_path(&path)
        .unwrap_or_else(|| panic!("function `{}` not found in model", fn_fqn.join("::")));
    eval.call_user_function_by_id(id)
        .unwrap_or_else(|e| panic!("evaluation of `{}` failed: {e}", fn_fqn.join("::")))
}

/// Partition by `grp`, sum the `val` per partition, assert the
/// extended TDS contains the expected per-row sums.
#[test]
fn olap_extend_sum_per_partition() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function olap_probe::sumPerGrp(): String[1]
{
    let r = #TDS
       grp, val
       1,   10
       1,   20
       2,   30
       2,   40
       2,   50
       3,   100
    #;
    $r->extend(over(~grp), ~total:{p,w,r|$r.val}:y|$y->plus())->toString()
}";
    let result = eval_function(
        "olap_sum.pure",
        source,
        &["olap_probe", "sumPerGrp"],
    );
    let s = match &result {
        Value::String(s) => s.to_string(),
        other => panic!("expected String, got {other:?}"),
    };
    let expected = "#TDS\n   grp,val,total\n   1,10,30\n   1,20,30\n   2,30,120\n   2,40,120\n   2,50,120\n   3,100,100\n#";
    assert_eq!(s, expected, "OLAP extend sum-per-partition mismatch");
}

/// Partition by `grp`, joinStrings the `name` per partition.
#[test]
fn olap_extend_join_strings_per_partition() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function olap_probe::joinNames(): String[1]
{
    let r = #TDS
       grp, name
       1,   alice
       1,   bob
       2,   carol
       2,   dan
    #;
    $r->extend(over(~grp), ~combined:{p,w,r|$r.name}:y|$y->joinStrings(','))->toString()
}";
    let result = eval_function(
        "olap_join.pure",
        source,
        &["olap_probe", "joinNames"],
    );
    let s = match &result {
        Value::String(s) => s.to_string(),
        other => panic!("expected String, got {other:?}"),
    };
    let expected = "#TDS\n   grp,name,combined\n   1,alice,alice,bob\n   1,bob,alice,bob\n   2,carol,carol,dan\n   2,dan,carol,dan\n#";
    assert_eq!(s, expected, "OLAP extend joinStrings-per-partition mismatch");
}

/// `groupBy(~grp, ~total:x|$x.val:y|$y->plus())` — collapse to one row
/// per `grp`, summing `val`. Result schema is `[grp, total]` (group col
/// + agg col), one row per group in first-occurrence order. Asserts the
/// collapsed TDS directly (no `chunk`, which the PCT tests use for
/// order-normalisation but isn't yet a runtime native — see scratch_5).
#[test]
fn group_by_single_single_sum() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function olap_probe::groupBySum(): String[1]
{
    let r = #TDS
       grp, val
       1,   10
       1,   20
       2,   30
       2,   40
       2,   50
       3,   100
    #;
    $r->groupBy(~grp, ~total : x | $x.val : y | $y->plus())->sort(~grp->ascending())->toString()
}";
    let result = eval_function("group_by_sum.pure", source, &["olap_probe", "groupBySum"]);
    let s = match &result {
        Value::String(s) => s.to_string(),
        other => panic!("expected String, got {other:?}"),
    };
    let expected = "#TDS\n   grp,total\n   1,30\n   2,120\n   3,100\n#";
    assert_eq!(s, expected, "groupBy single/single sum mismatch");
}

/// `groupBy(~[grp, grp2], ~[sumVal:..., cnt:...])` — multi group cols +
/// multi aggregates (the ColSpecArray x AggColSpecArray overload).
#[test]
fn group_by_multiple_multiple() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function olap_probe::groupByMulti(): String[1]
{
    let r = #TDS
       grp, grp2, val
       1,   9,    10
       1,   9,    20
       1,   8,    5
       2,   7,    30
       2,   7,    40
    #;
    $r->groupBy(~[grp, grp2], ~[sumVal : x | $x.val : y | $y->plus(), cnt : x | $x.val : y | $y->count()])
      ->sort([~grp->ascending(), ~grp2->ascending()])->toString()
}";
    let result = eval_function("group_by_multi.pure", source, &["olap_probe", "groupByMulti"]);
    let s = match &result {
        Value::String(s) => s.to_string(),
        other => panic!("expected String, got {other:?}"),
    };
    // grp=1,grp2=8 -> [5,1]; grp=1,grp2=9 -> [30,2]; grp=2,grp2=7 -> [70,2]
    let expected =
        "#TDS\n   grp,grp2,sumVal,cnt\n   1,8,5,1\n   1,9,30,2\n   2,7,70,2\n#";
    assert_eq!(s, expected, "groupBy multiple/multiple mismatch");
}

/// Regression guard for a pure-side TDS-parser bug: a `#TDS` literal
/// cell with internal spaces (`More George 1`) loses them during
/// island-grammar parsing (→ `MoreGeorge1`). Independent of any
/// relation native. #[ignore]d until legend-pure-rust preserves
/// inter-token whitespace in unquoted TDS cells (filed in scratch_5);
/// the `join` PCT tests are blocked on the same gap.
#[test]
#[ignore = "pure-side: TDS island-grammar strips internal whitespace from unquoted cells"]
fn tds_internal_spaces_probe() {
    let source = r"###Pure
import meta::pure::functions::relation::*;

function olap_probe::spaces(): String[1]
{
    #TDS
       id, name
       1, More George 1
       2, David
    #->toString()
}";
    let result = eval_function("tds_spaces.pure", source, &["olap_probe", "spaces"]);
    let s = match &result {
        Value::String(s) => s.to_string(),
        other => panic!("{other:?}"),
    };
    assert!(s.contains("More George 1"), "internal spaces lost: {s}");
}

/// `join` correctness over single-word cells (independent of the TDS
/// internal-whitespace bug that blocks the join PCT tests). Covers all
/// four `JoinKind`s. Result = left cols ++ right cols; unmatched rows
/// emit `null` on the other side.
#[test]
fn join_all_kinds_single_word() {
    let preamble = r"###Pure
import meta::pure::functions::relation::*;

function olap_probe::doJoin(): String[1]
{
    let t1 = #TDS
       id, name
       1, a
       2, b
       3, c
       4, d
    #;
    let t2 = #TDS
       id2, col
       1, x
       1, y
       4, z
       6, w
    #;
    $t1->join($t2, JoinKind.KIND, {x,y| $x.id == $y.id2})
       ->sort([~id->ascending(), ~col->ascending()])->toString()
}";
    let cases = [
        (
            "INNER",
            "#TDS\n   id,name,id2,col\n   1,a,1,x\n   1,a,1,y\n   4,d,4,z\n#",
        ),
        (
            "LEFT",
            "#TDS\n   id,name,id2,col\n   1,a,1,x\n   1,a,1,y\n   2,b,null,null\n   3,c,null,null\n   4,d,4,z\n#",
        ),
        (
            // id-null row sorts last (ASC NULLS LAST).
            "RIGHT",
            "#TDS\n   id,name,id2,col\n   1,a,1,x\n   1,a,1,y\n   4,d,4,z\n   null,null,6,w\n#",
        ),
        (
            "FULL",
            "#TDS\n   id,name,id2,col\n   1,a,1,x\n   1,a,1,y\n   2,b,null,null\n   3,c,null,null\n   4,d,4,z\n   null,null,6,w\n#",
        ),
    ];
    for (kind, expected) in cases {
        let source = preamble.replace("JoinKind.KIND", &format!("JoinKind.{kind}"));
        let result = eval_function("join_probe.pure", &source, &["olap_probe", "doJoin"]);
        let s = match &result {
            Value::String(s) => s.to_string(),
            other => panic!("expected String, got {other:?}"),
        };
        assert_eq!(s, expected, "join {kind} mismatch");
    }
}
