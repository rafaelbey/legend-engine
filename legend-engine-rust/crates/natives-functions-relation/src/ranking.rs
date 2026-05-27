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

//! OLAP ranking natives, invoked from inside an
//! `extend(Relation, _Window, FuncColSpec)` map lambda:
//!
//! ```pure
//! native rowNumber<T>(rel:Relation<T>[1], row:T[1]):Integer[1];
//! native rank<T>(rel:Relation<T>[1], w:_Window<T>[1], row:T[1]):Integer[1];
//! native denseRank<T>(rel:Relation<T>[1], w:_Window<T>[1], row:T[1]):Integer[1];
//! native percentRank<T>(rel:Relation<T>[1], w:_Window<T>[1], row:T[1]):Float[1];
//! native cumulativeDistribution<T>(rel:Relation<T>[1], w:_Window<T>[1], row:T[1]):Float[1];
//! native ntile<T>(rel:Relation<T>[1], row:T[1], tileCount:Integer[1]):Integer[1];
//! ```
//!
//! The `extend(Relation, _Window, FuncColSpec)` native passes each map
//! lambda **the sorted partition sub-TDS** as `rel` and a row tuple
//! carrying [`ROW_INDEX_SLOT`] (its 0-based position within that sorted
//! partition) as `row`. So each native here reads:
//!
//! - the partition's rows (already in window-sort order) from `rel`,
//! - the current position from `row`'s `__row_index`,
//! - the sort columns from `w` (where present),
//!
//! and reproduces the Java
//! `RelationNativeImplementation` / `TestTDS` formulas verbatim:
//!
//! - `rowNumber` → `position + 1`.
//! - `rank` → `1 + findFirstPrecedentDifferentRow` (SQL `RANK`,
//!   ties share the minimum rank).
//! - `denseRank` → count of distinct sort-key runs up to position.
//! - `percentRank` → `size==1 ? 0 : start_of_run / (size - 1)`.
//! - `cumulativeDistribution` → `(start_of_run + 1) / size`.
//! - `ntile` → `floor(position * tiles / size) + 1`.
//!
//! ("start_of_run" = the 0-based index of the first row sharing the
//! current row's sort-key tuple — Java's `findFirstPrecedentDifferentRow`.)

#![allow(clippy::needless_pass_by_value)]

use legend_pure_dsl_tds::csv::{ParsedTDS, TypedCell};
use legend_pure_parser_pure::types::ValueSpec;

use legend_pure_runtime::error::{PureException, PureRuntimeError};
use legend_pure_runtime::m3_paths;
use legend_pure_runtime::native::relation::shared::{read_parsed_tds, unwrap_instance_value};
use legend_pure_runtime::native::{EvalContextTrait, Evaluated, NativeFunction, expect_args};
use legend_pure_runtime::value::Value;

use crate::window_runtime::{read_row_index, read_sort_keys, resolve_sort_indices, SortDir};

// ---------------------------------------------------------------------------
// Shared decoding
// ---------------------------------------------------------------------------

/// Decode `(partition_tds, row_position)` from `rel` arg + `row` arg.
/// `rel` is the sorted partition sub-TDS; `row` carries the 0-based
/// position via `__row_index`.
#[allow(clippy::result_large_err)]
fn decode_partition_and_position(
    rel_arg: &ValueSpec,
    row_arg: &ValueSpec,
    ctx: &mut dyn EvalContextTrait,
    fn_label: &'static str,
) -> Result<(ParsedTDS, usize), PureException> {
    let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);

    let rel_value = ctx.evaluate(rel_arg)?.into_value();
    let tds_obj = unwrap_instance_value(&rel_value, instance_value_id, ctx)?;
    let parsed = read_parsed_tds(fn_label, &tds_obj, ctx)?;

    let row_value = ctx.evaluate(row_arg)?.into_value();
    let row_obj = unwrap_instance_value(&row_value, instance_value_id, ctx)?;
    let position = read_row_index(&row_obj, ctx).ok_or_else(|| {
        PureException::from(PureRuntimeError::EvaluationError(format!(
            "{fn_label}: row tuple is missing its window position ({}); ranking \
             natives must be called from inside an extend(_, _Window, FuncColSpec) \
             map lambda",
            crate::window_runtime::ROW_INDEX_SLOT
        )))
    })?;
    Ok((parsed, position))
}

/// Resolve the window's sort-column indices against the partition TDS.
#[allow(clippy::result_large_err)]
fn sort_column_indices(
    window_arg: &ValueSpec,
    parsed: &ParsedTDS,
    ctx: &mut dyn EvalContextTrait,
    fn_label: &'static str,
) -> Result<Vec<usize>, PureException> {
    let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);
    let window_value = ctx.evaluate(window_arg)?.into_value();
    let window_obj = unwrap_instance_value(&window_value, instance_value_id, ctx)?;
    let sort_keys = read_sort_keys(&window_obj, ctx)?;
    let resolved: Vec<(usize, SortDir)> = resolve_sort_indices(&sort_keys, parsed, fn_label)?;
    Ok(resolved.into_iter().map(|(i, _)| i).collect())
}

/// The sort-key tuple of a row: the cells at the sort-column indices.
fn key_at(parsed: &ParsedTDS, sort_cols: &[usize], row: usize) -> Vec<Option<TypedCell>> {
    sort_cols.iter().map(|&c| parsed.rows[row][c].clone()).collect()
}

/// Java `findFirstPrecedentDifferentRow`: scan back from `row` while the
/// preceding row's sort-key tuple equals `row`'s, returning the 0-based
/// index of the first row in the current key-run.
fn start_of_run(parsed: &ParsedTDS, sort_cols: &[usize], row: usize) -> usize {
    let base = key_at(parsed, sort_cols, row);
    let mut rank = row as isize;
    loop {
        rank -= 1;
        if rank < 0 || key_at(parsed, sort_cols, rank as usize) != base {
            break;
        }
    }
    (rank + 1) as usize
}

// ---------------------------------------------------------------------------
// rowNumber
// ---------------------------------------------------------------------------

/// `rowNumber<T>(rel:Relation<T>[1], row:T[1]):Integer[1]`.
#[derive(Debug)]
pub struct RowNumber;

impl NativeFunction for RowNumber {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("rowNumber (Relation, row)", args, 2)?;
        let (_parsed, position) =
            decode_partition_and_position(&args[0], &args[1], ctx, "rowNumber")?;
        Ok(Evaluated::new(Value::Integer(position as i64 + 1)))
    }
}

// ---------------------------------------------------------------------------
// rank / denseRank
// ---------------------------------------------------------------------------

/// `rank<T>(rel:Relation<T>[1], w:_Window<T>[1], row:T[1]):Integer[1]`.
#[derive(Debug)]
pub struct Rank;

impl NativeFunction for Rank {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("rank (Relation, _Window, row)", args, 3)?;
        let (parsed, position) =
            decode_partition_and_position(&args[0], &args[2], ctx, "rank")?;
        let sort_cols = sort_column_indices(&args[1], &parsed, ctx, "rank")?;
        let rank = start_of_run(&parsed, &sort_cols, position) + 1;
        Ok(Evaluated::new(Value::Integer(rank as i64)))
    }
}

/// `denseRank<T>(rel:Relation<T>[1], w:_Window<T>[1], row:T[1]):Integer[1]`.
#[derive(Debug)]
pub struct DenseRank;

impl NativeFunction for DenseRank {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("denseRank (Relation, _Window, row)", args, 3)?;
        let (parsed, position) =
            decode_partition_and_position(&args[0], &args[2], ctx, "denseRank")?;
        let sort_cols = sort_column_indices(&args[1], &parsed, ctx, "denseRank")?;

        // Count distinct sort-key runs from row 0 up to `position`.
        let mut rank: i64 = 1;
        if position > 0 {
            let mut prev = key_at(&parsed, &sort_cols, 0);
            for i in 1..=position {
                let cur = key_at(&parsed, &sort_cols, i);
                if cur != prev {
                    rank += 1;
                    prev = cur;
                }
            }
        }
        Ok(Evaluated::new(Value::Integer(rank)))
    }
}

// ---------------------------------------------------------------------------
// percentRank / cumulativeDistribution
// ---------------------------------------------------------------------------

/// `percentRank<T>(rel:Relation<T>[1], w:_Window<T>[1], row:T[1]):Float[1]`.
#[derive(Debug)]
pub struct PercentRank;

impl NativeFunction for PercentRank {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("percentRank (Relation, _Window, row)", args, 3)?;
        let (parsed, position) =
            decode_partition_and_position(&args[0], &args[2], ctx, "percentRank")?;
        let sort_cols = sort_column_indices(&args[1], &parsed, ctx, "percentRank")?;
        let size = parsed.rows.len();
        let value = if size == 1 {
            0.0
        } else {
            start_of_run(&parsed, &sort_cols, position) as f64 / (size as f64 - 1.0)
        };
        Ok(Evaluated::new(Value::Float(value)))
    }
}

/// `cumulativeDistribution<T>(rel:Relation<T>[1], w:_Window<T>[1], row:T[1]):Float[1]`.
#[derive(Debug)]
pub struct CumulativeDistribution;

impl NativeFunction for CumulativeDistribution {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("cumulativeDistribution (Relation, _Window, row)", args, 3)?;
        let (parsed, position) =
            decode_partition_and_position(&args[0], &args[2], ctx, "cumulativeDistribution")?;
        let sort_cols = sort_column_indices(&args[1], &parsed, ctx, "cumulativeDistribution")?;
        let size = parsed.rows.len();
        let value = (start_of_run(&parsed, &sort_cols, position) as f64 + 1.0) / size as f64;
        Ok(Evaluated::new(Value::Float(value)))
    }
}

// ---------------------------------------------------------------------------
// ntile
// ---------------------------------------------------------------------------

/// `ntile<T>(rel:Relation<T>[1], row:T[1], tileCount:Integer[1]):Integer[1]`.
#[derive(Debug)]
pub struct Ntile;

impl NativeFunction for Ntile {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("ntile (Relation, row, tileCount)", args, 3)?;
        let (parsed, position) =
            decode_partition_and_position(&args[0], &args[1], ctx, "ntile")?;
        let tiles = match ctx.evaluate(&args[2])?.into_value() {
            Value::Integer(n) => n,
            other => {
                return Err(PureException::from(PureRuntimeError::type_mismatch(
                    "Integer", &other,
                )));
            }
        };
        let size = parsed.rows.len() as i64;
        // Java: floor(row * tiles / size) + 1.
        let tile = if size == 0 {
            1
        } else {
            (position as i64 * tiles) / size + 1
        };
        Ok(Evaluated::new(Value::Integer(tile)))
    }
}
