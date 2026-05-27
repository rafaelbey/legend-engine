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

//! `filter(rel:Relation<T>[1], f:Function<{T[1]→Boolean[1]}>[1]):Relation<T>[1]`.
//!
//! Lambda-per-row template: invoke the predicate with `$x` bound to a
//! synthetic heap object that carries the row's column values as named
//! slots. Survivors are re-emitted as a fresh `TDS` carrying the
//! surviving rows plus the *source* column metadata — the column types
//! are carried forward verbatim (via [`alloc_tds_from_parsed`]'s
//! `classifierGenericType`), never re-inferred, so a filtered subset
//! can't shift a column's type (e.g. all-Integer rows surviving from a
//! Float column stay Float).
//!
//! This is the row-binding template that `sort`, `extend`, `groupBy`,
//! and any other lambda-per-row relation native will share.

#![allow(clippy::needless_pass_by_value)]

use legend_pure_parser_pure::types::ValueSpec;

use legend_pure_runtime::error::PureException;
use legend_pure_runtime::m3_paths;
use legend_pure_runtime::native::{EvalContextTrait, Evaluated, NativeFunction, expect_args};
use legend_pure_runtime::value::Value;

use legend_pure_dsl_tds::csv::ParsedTDS;
use legend_pure_runtime::native::relation::shared::{
    alloc_tds_from_parsed, build_row_tuple, read_parsed_tds, unwrap_instance_value,
};

/// Pure
/// `filter<T>(rel:Relation<T>[1], f:Function<{T[1]→Boolean[1]}>[1])
///   :Relation<T>[1]`.
///
/// Per-row evaluation:
/// 1. Re-parse the receiver TDS (`read_parsed_tds`) so we have typed
///    column metadata + materialised cells.
/// 2. For each row, allocate a synthetic `Any`-classified heap object
///    (`row_tuple`) and `mutate_add` each column's value into a slot
///    named after the column. Empty cells are simply omitted, so
///    `$x.col` returns `Value::Unit` — matching `[0..1]` semantics.
/// 3. Invoke the lambda via `ctx.call_function(&f, &[row_tuple])` and
///    coerce the result to `Boolean`.
/// 4. Collect surviving rows and re-emit them as a fresh `TDS` via
///    [`alloc_tds_from_parsed`], carrying the *source* column metadata
///    forward unchanged.
///
/// Carrying the source columns forward (rather than re-inferring from
/// the surviving subset) is what keeps a column's type stable: a filter
/// that leaves only whole-numbered rows of a `Float` column must not
/// re-classify it as `Integer`. The row cells are already typed
/// ([`read_parsed_tds`] materialised them), so no CSV round-trip is
/// involved.
#[derive(Debug)]
pub struct Filter;

impl NativeFunction for Filter {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("filter (Relation)", args, 2)?;
        let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);

        // -- Source TDS --------------------------------------------------
        let rel_value = ctx.evaluate(&args[0])?.into_value();
        let tds_obj = unwrap_instance_value(&rel_value, instance_value_id, ctx)?;
        let parsed = read_parsed_tds("filter", &tds_obj, ctx)?;

        // -- Predicate lambda --------------------------------------------
        let lambda_val = ctx.evaluate(&args[1])?.into_value();

        // -- Per-row evaluation ------------------------------------------
        let mut survivors: Vec<usize> = Vec::with_capacity(parsed.rows.len());
        for (row_idx, row) in parsed.rows.iter().enumerate() {
            let row_tuple = build_row_tuple(&parsed.columns, row, ctx)?;
            let pred = ctx.call_function(&lambda_val, &[Value::Object(row_tuple)])?;
            if pred.as_boolean()? {
                survivors.push(row_idx);
            }
        }

        // -- Re-emit the surviving rows ----------------------------------
        // Carry the source columns forward unchanged (types preserved).
        let kept_rows: Vec<_> = survivors.iter().map(|&i| parsed.rows[i].clone()).collect();
        let result = ParsedTDS {
            csv: String::new(),
            columns: parsed.columns,
            rows: kept_rows,
        };
        let new_tds = alloc_tds_from_parsed(ctx, &result)?;
        Ok(Evaluated::new(Value::Object(new_tds)))
    }
}
