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

//! ## Reproducer for the PCT-generic `->sort(~col->ascending())` narrowing gap
//!
//! After the upstream `legend-pure-rust` Z-propagation fix
//! (`983bb31fd9d feat(pure): Z-propagation through bound-T parameter
//! slots`), an engine-side workaround in `crates/pure/src/resolve.rs`
//! (returning `None` for Generic-typed `Variable` inferences) was
//! dropped as redundant. That dropped 351 catalog errors -> 4, but it
//! also resurfaced **343** "Ambiguous function call 'sort'/'extend'"
//! errors that the workaround was load-bearing for. Z-propagation
//! repairs the **binding** path; the **narrowing** path
//! (`narrow_candidates_by_type` Phase-1 filter) still trips on the
//! same scenario.
//!
//! This file isolates the failing dispatch from the 348-error pile so
//! the next pure-side fix has a precise target.
//!
//! ## The Pure source under test
//!
//! ```pure
//! function pct_repro::probeSort<T|m>(
//!     f:Function<{Function<{->T[m]}>[1]->T[m]}>[1]
//! ):Boolean[1]
//! {
//!     let expr = {| #TDS val\n 1\n 2\n# };
//!     let res = $f->eval($expr);
//!     $res->sort(~val->ascending())->toString();
//!     true
//! }
//! ```
//!
//! This mirrors hundreds of `<<PCT.test>>`-annotated functions in
//! `legend-engine/.../core_functions_relation/relation/tests/**` and
//! `core_functions_relation/relation/functions/{transformation,olap,
//! slice,order}/*.pure`. The shape is:
//!
//! 1. A function whose type parameter `T` (and multiplicity `m`)
//!    sit at the outer (function) level.
//! 2. A `let res = $f->eval(λ)` call. Per platform `eval` overloads,
//!    `res` has type `T[m]` (caller's `T`, caller's `m`).
//! 3. A relation native called on `$res`. Here `sort(~val->ascending())`.
//!
//! ## Current behaviour
//!
//! Compilation fails with:
//!
//! ```text
//! Ambiguous function call 'sort': found 2 overloads with 2 args
//! (narrowed from 2 candidates)
//! ```
//!
//! ## Trace of the failing dispatch
//!
//! Two `sort` overloads with 2 args exist in scope when this expression
//! resolves:
//!
//! - `relation::sort<X,T>(rel:Relation<T>[1], sortInfo:SortInfo<X⊆T>[*]):Relation<T>[1]`
//! - `collection::sort<T|m>(col:T[m], comp:Function<{T[1],T[1]->Integer[1]}>[0..1]):T[m]`
//!
//! In `narrow_candidates_by_type` (Phase-1 filter):
//!
//! - **arg0 (`$res`)** is a `Variable` with `var_types[res] = (Generic("T"), m)`.
//!   `infer_type_from_valuespec`'s Variable arm returns `Some(ANY_ID)` for
//!   `Generic(_)` (this is the *upstream* behaviour after my workaround
//!   commit was dropped).
//!
//! - **For `relation::sort`**, param0 is `Named{Relation<T>}`. Compatibility
//!   reduces to `is_subtype(Any, Relation) = false`. **Rejected.**
//! - **For `collection::sort`**, param0 is `Generic("T")`. The Generic arm
//!   of `is_type_compatible` returns `true` unconditionally. **Accepted.**
//!   But param1 is `Named{Function<{T,T->Integer}>}[0..1]`; arg1
//!   (`~val->ascending()`) is `SortInfo[1]`. `is_subtype(SortInfo, Function)
//!    = false`. **Rejected.**
//!
//! Both candidates fail Phase-1. The empty-fallback restores the
//! original set (`candidates.to_vec()`), and Phase-2 reports "narrowed
//! from 2 candidates" — i.e. the ambiguity error.
//!
//! ## Intended behaviour
//!
//! The narrower should select `relation::sort` cleanly:
//!
//! - arg1 (`SortInfo[1]`) is a precise *structural* match for the
//!   `SortInfo<…>[*]` slot on `relation::sort` — the right candidate
//!   to keep.
//! - arg0 being Generic-typed (`Generic("T")[m]`) is "unknown" in the
//!   narrower's sense — it should not eliminate any otherwise-
//!   compatible candidate.
//!
//! Two viable fix directions, both pure-side:
//!
//! 1. **Treat `Generic("T")` at the Variable arm as `None` (unknown)
//!    for narrowing.** The narrower already has the right shape for
//!    `None` args (permissive). This was the workaround I had
//!    (`fix(pure): overload narrowing for Generic-typed receivers`) —
//!    it conflicts with Z-prop (drives drift up, blocks the new
//!    `zp_lambda_body_drives_outer_pct_generic_through_class_typearg`
//!    test) because it intercepts before binding can take effect.
//!
//! 2. **Make `is_type_compatible(Some(ANY_ID), Named{X})` permissive
//!    in the narrower's Phase-1 filter** when `Any` originates from a
//!    Generic-typed source (Java parity — `Any` is a top type at
//!    dispatch sites). Mirrors the existing permissive arms for Nil,
//!    Function refs, and Package values. Doesn't fight Z-prop because
//!    it's downstream of binding.
//!
//! Suggested investigation entry point: `narrow_candidates_by_type` in
//! `legend-pure-rust/crates/pure/src/resolve.rs` Phase-1 filter. Add a
//! sibling permissive arm next to the existing
//! `arg_eid == Some(bootstrap::NIL_ID)` skip, conditioned on Generic
//! origin (detectable by looking at the arg's full `arg_ty.type_expr`
//! or `lowered_args[i].kind`).
//!
//! ## Pure-side test placement
//!
//! A focused pure-side regression test sibling to
//! `crates/pure/tests/z_propagation_probes.rs` (added by the Z-prop
//! commit) would cover this end-to-end. Suggested skeleton at the
//! bottom of this file (`PURE_SIDE_TEST_SKELETON`).
//!
//! ## When this test passes
//!
//! Promote the `#[ignore]` off `probe_sort_on_pct_generic_compiles_cleanly`,
//! and the 333 `Ambiguous 'sort'` + 10 `Ambiguous 'extend'` catalog
//! errors should clear in the same upstream change.

use legend_pure_core_platform::platform::PLATFORM_AUTO_IMPORTS;
use legend_pure_core_platform::repo::{self, OwnedSourceFile, Repo, RepoMeta};
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
        name: "pct_repro",
        pattern: ".*",
        dependencies: USER_DEPS,
    };
    repos.push(Repo::Filesystem {
        prefix: "/pct_repro".into(),
        files: vec![OwnedSourceFile {
            path: format!("/pct_repro/{name}"),
            content: source.into(),
        }],
        meta: Some(meta),
        source_root: None,
    });
    repos
}

/// Collect every error attributed to the user-source URL.
fn user_source_errors(name: &str, source: &str) -> Vec<String> {
    let repos = compose(name, source);
    let url = format!("/pct_repro/{name}");
    let errors = match repo::load(&repos, &auto_imports()) {
        Ok(_) => Vec::new(),
        Err(p) => p.errors,
    };
    errors
        .into_iter()
        .filter(|e| e.source_info.source.as_str() == url)
        .map(|e| {
            format!(
                "{}:{}:{} {}",
                e.source_info.source,
                e.source_info.start_line,
                e.source_info.start_column,
                e.message,
            )
        })
        .collect()
}

#[test]
fn probe_sort_on_pct_generic_compiles_cleanly() {
    // PCT shape: outer function-level <T|m>, `let res = $f->eval(λ)`,
    // then a relation native on `$res`. Expected to compile clean once
    // Phase-1 narrowing handles a Generic-typed receiver permissively.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function pct_repro::probeSort<T|m>(f:Function<{Function<{->T[m]}>[1]->T[m]}>[1]):Boolean[1]
{
    let expr = {|
        #TDS
            val
            1
            2
        #
    };
    let res = $f->eval($expr);
    $res->sort(~val->ascending())->toString();
    true
}";
    let errors = user_source_errors("probe_sort.pure", source);
    assert!(
        errors.is_empty(),
        "expected clean compile; got {} user-source error(s):\n{}",
        errors.len(),
        errors.join("\n"),
    );
}

#[test]
fn probe_extend_on_pct_generic_compiles_cleanly() {
    // Same shape as `probe_sort`, but exercising the `Ambiguous
    // 'extend': found 4 overloads with 2 args` sibling case. Listed
    // here because the root cause is the same Phase-1 filter eliminating
    // the right candidate when arg0 is Generic-typed.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function pct_repro::probeExtend<T|m>(f:Function<{Function<{->T[m]}>[1]->T[m]}>[1]):Boolean[1]
{
    let expr = {|
        #TDS
            val
            1
            2
        #
    };
    let res = $f->eval($expr);
    $res->extend(~doubled:c|$c.val->toOne() * 2)->toString();
    true
}";
    let errors = user_source_errors("probe_extend.pure", source);
    assert!(
        errors.is_empty(),
        "expected clean compile; got {} user-source error(s):\n{}",
        errors.len(),
        errors.join("\n"),
    );
}

#[test]
fn baseline_concrete_receiver_sort_compiles_cleanly() {
    // Sibling control case: same `->sort(~val->ascending())` pattern
    // but on a concretely-typed receiver (no outer-Generic flow).
    // This already passes today — it's here so a regression in the
    // baseline path shows up alongside the probe failures.
    let source = r"###Pure
import meta::pure::functions::relation::*;

function pct_repro::baselineSort(): Integer[1]
{
    let r = #TDS
       val
       3
       1
       2
    #;
    $r->sort(~val->ascending())->size()
}";
    let errors = user_source_errors("baseline_sort.pure", source);
    assert!(
        errors.is_empty(),
        "baseline regressed; got {} user-source error(s):\n{}",
        errors.len(),
        errors.join("\n"),
    );
}

// ---------------------------------------------------------------------------
// Suggested Pure-side test skeleton (for the expert to drop into
// `legend-pure-rust/crates/pure/tests/z_propagation_probes.rs` or a
// new file sibling to it).
// ---------------------------------------------------------------------------
//
// Self-contained — uses a synthetic mini-platform that declares the
// two `sort` overloads inline, so it doesn't depend on the engine
// repos at all. Currently fails with the same "narrowed from 2
// candidates" ambiguity; passes once the Phase-1 filter handles a
// Generic-typed receiver.
//
// ```rust
// #[test]
// fn narrow_keeps_relation_sort_when_receiver_is_pct_generic() {
//     // Two `sort` overloads, matching the platform shape:
//     //   relation::sort<X,T>(Relation<T>[1], SortInfo<X>[*]):Relation<T>[1]
//     //   collection::sort<T|m>(T[m], Function<{T,T->Integer}>[0..1]):T[m]
//     // and a caller-PCT function `f<T|m>(...)` that does
//     //   let res = $f->eval($expr); $res->sort(~col->ascending())
//     //
//     // Assert: the compiled body has a single resolved sort call,
//     // routed to relation::sort. NO "Ambiguous function call 'sort'"
//     // diagnostic.
//     //
//     // The "Variable Generic -> Some(ANY_ID)" return shape from
//     // `infer_type_from_valuespec` is the upstream behaviour after
//     // the dropped workaround commit; with that, Phase-1's
//     // `is_subtype(Any, Relation)` rejects `relation::sort` falsely.
//     // The fix candidate lands either at the Variable arm
//     // (Generic -> None) or in `is_type_compatible` (Some(ANY_ID) as
//     // permissive at narrowing time).
// }
// ```
