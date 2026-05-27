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

//! Standalone `reduce` native:
//!
//! ```pure
//! native function reduce<T, V, U|m>(
//!     rel : Relation<T>[1],
//!     w   : _Window<T>[1],
//!     row : T[1],
//!     map : Function<{T[1]->V[*]}>[1],
//!     agg : Function<{V[*]->U[m]}>[1]
//! ) : U[m];
//! ```
//!
//! Called from inside a `FuncColSpec` map lambda inside `extend`; per-
//! row windowed aggregation. The `row` argument is the row tuple
//! already bound to the FuncColSpec lambda's `r` parameter, so we
//! locate it inside `rel` by content match (see
//! `window_runtime::row_tuple_to_source_index`).
//!
//! Steps:
//! 1. Parse `rel`'s CSV; read `w.partition`, `w.sortInfo`, `w.frame`.
//! 2. Locate `row`'s source index; identify its partition.
//! 3. Sort that partition by `w.sortInfo`; find `row`'s position.
//! 4. Apply `w.frame` to get the in-frame row indices.
//! 5. For each in-frame row, build the row tuple, call `map` → V[*].
//! 6. Flatten V values, call `agg(V[*])` → U[m]. Return.

#![allow(clippy::needless_pass_by_value)]

use im_rc::Vector as PVector;

use legend_pure_parser_pure::types::ValueSpec;
use legend_pure_runtime::error::{PureException, PureRuntimeError};
use legend_pure_runtime::m3_paths;
use legend_pure_runtime::native::relation::shared::{
    build_row_tuple, read_parsed_tds, unwrap_instance_value,
};
use legend_pure_runtime::native::{EvalContextTrait, Evaluated, NativeFunction, expect_args};
use legend_pure_runtime::value::Value;

use crate::window_runtime::{
    effective_frame, frame_indices_for_row, partition_row_indices, push_flat, read_frame,
    read_partition_cols, read_row_index, read_sort_keys, resolve_partition_indices,
    resolve_sort_indices, row_tuple_to_source_index, sort_partitions_in_place,
};

/// Pure `reduce<T,V,U|m>(rel:Relation<T>, w:_Window<T>, row:T, map, agg):U[m]`.
#[derive(Debug)]
pub struct Reduce;

impl NativeFunction for Reduce {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("reduce (Relation, _Window, row, map, agg)", args, 5)?;
        let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);

        // -- args ------------------------------------------------------
        let rel_value = ctx.evaluate(&args[0])?.into_value();
        let tds_obj = unwrap_instance_value(&rel_value, instance_value_id, ctx)?;
        let parsed = read_parsed_tds("reduce", &tds_obj, ctx)?;

        let window_value = ctx.evaluate(&args[1])?.into_value();
        let window_obj = unwrap_instance_value(&window_value, instance_value_id, ctx)?;
        let partition_cols = read_partition_cols(&window_obj, ctx)?;
        let sort_keys = read_sort_keys(&window_obj, ctx)?;
        // No explicit frame + ORDER BY -> default to a running frame
        // (UNBOUNDED PRECEDING .. CURRENT ROW); no ORDER BY -> full
        // partition. Matches SQL window-default semantics.
        let frame = effective_frame(read_frame(&window_obj, ctx)?, !sort_keys.is_empty());

        let row_value = ctx.evaluate(&args[2])?.into_value();
        let row_obj = unwrap_instance_value(&row_value, instance_value_id, ctx)?;

        let map_fn = ctx.evaluate(&args[3])?.into_value();
        let agg_fn = ctx.evaluate(&args[4])?.into_value();

        // -- locate the row's partition + position ----------------------
        //
        // Preferred path: when `reduce` is called from inside
        // `extend(Relation, _Window, FuncColSpec)`, `rel` is already the
        // sorted partition sub-TDS and `row` carries its within-partition
        // position via `__row_index` (mirrors Java's
        // `RowContainer(winTDS, i)`). The partition is the whole `rel`,
        // already in sort order — no re-partition / content match needed.
        //
        // Fallback path (`rel` is a full relation, `row` has no index):
        // re-partition `rel` by the window, stable-sort each partition,
        // then content-match the row to find its (partition, position).
        // Sort columns are needed both for the fallback re-sort and for
        // Range frame value comparison.
        let sort_indices = resolve_sort_indices(&sort_keys, &parsed, "reduce")?;
        let (partition_rows, position) = if let Some(idx) = read_row_index(&row_obj, ctx) {
            ((0..parsed.rows.len()).collect::<Vec<usize>>(), idx)
        } else {
            let partition_indices = resolve_partition_indices(&partition_cols, &parsed, "reduce")?;
            let mut partitions = partition_row_indices(&parsed, &partition_indices);
            sort_partitions_in_place(&mut partitions, &parsed, &sort_indices);

            let row_src_idx =
                row_tuple_to_source_index(&row_obj, &parsed, ctx)?.ok_or_else(|| {
                    PureException::from(PureRuntimeError::EvaluationError(
                        "reduce: input row not found in receiver relation".into(),
                    ))
                })?;
            let partition_rows = partitions
                .iter()
                .find(|(_, rows)| rows.contains(&row_src_idx))
                .map(|(_, rows)| rows.clone())
                .ok_or_else(|| {
                    PureException::from(PureRuntimeError::EvaluationError(
                        "reduce: failed to locate row's partition (internal)".into(),
                    ))
                })?;
            let position = partition_rows
                .iter()
                .position(|&i| i == row_src_idx)
                .expect("row known to be in partition");
            (partition_rows, position)
        };

        // -- frame ------------------------------------------------------
        let in_frame =
            frame_indices_for_row(&parsed, &partition_rows, position, frame.as_ref(), &sort_indices);

        // -- per-row map -----------------------------------------------
        let mut v_values: PVector<Value> = PVector::new();
        for &src_idx in &in_frame {
            let row_tuple = build_row_tuple(&parsed.columns, &parsed.rows[src_idx], ctx)?;
            let v = ctx.call_function(&map_fn, &[Value::Object(row_tuple)])?;
            push_flat(&mut v_values, &v);
        }

        // -- agg --------------------------------------------------------
        // Empty in-frame (no rows, or every map output was Unit) ->
        // emit `Unit` without calling `agg`. Mirrors
        // `AggregationShared.processAggregation` in Java Pure: when the
        // aggregation collection is empty, the result slot is null
        // rather than `reduce(empty)` (which for `plus` would
        // synthesise 0, for `joinStrings` would produce ""). The PCT
        // corpus expects null in both cases.
        if v_values.is_empty() {
            return Ok(Evaluated::new(Value::Unit));
        }
        // Pure: `agg : Function<{V[*]->U[m]}>` — pass the V collection
        // as a single arg (Collection, or scalar for the degenerate
        // 1-element case).
        let agg_arg = if v_values.len() == 1 {
            v_values.iter().next().cloned().unwrap_or(Value::Unit)
        } else {
            Value::Collection(Box::new(v_values))
        };
        let result = ctx.call_function(&agg_fn, &[agg_arg])?;
        Ok(Evaluated::new(result))
    }
}
