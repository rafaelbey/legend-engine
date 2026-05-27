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

//! OLAP window-extend natives:
//!
//! - `extend<T,K,V,R>(r:Relation<T>[1], window:_Window<T>[1],
//!     agg:AggColSpec<{Relation<T>[1],_Window<T>[1],T[1]->K[0..1]},
//!                    {K[*]->V[0..1]}, R>[1]):Relation<T+R>[1]`
//!
//! Per-partition semantics:
//!
//! 1. Partition source rows by the window's `partition: String[*]`
//!    column names (rows whose partition-key tuple matches share a
//!    partition).
//! 2. For each row, evaluate the **map** lambda with
//!    `(receiver_relation, window, row_tuple)` → a per-row K value.
//! 3. For each row, evaluate the **reduce** lambda with the
//!    K-collection from the row's partition → a per-row V value.
//! 4. Append V as the new column on the source TDS.
//!
//! The `_Window<T>` object is materialised by the Pure-defined
//! `over(...)` helpers (over.pure) via `^_Window<T>(...)`. We read
//! its `partition` slot here; `sortInfo` / `frame` are not yet
//! consumed by this native (partition-only windows match every
//! current PCT test in `composition.pure`).

#![allow(clippy::needless_pass_by_value)]

use im_rc::Vector as PVector;
use legend_pure_dsl_tds::csv::{ColumnType, ParsedColumn, ParsedTDS, TypedCell};
use legend_pure_parser_pure::types::{Multiplicity, ValueSpec};
use smol_str::SmolStr;

use legend_pure_runtime::error::{PureException, PureRuntimeError};
use legend_pure_runtime::m3_paths;
use legend_pure_runtime::native::{EvalContextTrait, Evaluated, NativeFunction, expect_args};
use legend_pure_runtime::native::relation::shared::{
    alloc_tds_from_parsed, build_row_tuple, read_parsed_tds, unwrap_instance_value,
};
use legend_pure_runtime::value::Value;

use crate::window_runtime::{
    attach_row_index, frame_row_indices, partition_row_indices, read_frame, read_partition_cols,
    read_sort_keys, resolve_partition_indices, resolve_sort_indices, sort_partitions_in_place,
};

/// `extend(Relation, _Window, AggColSpec)`.
#[derive(Debug)]
pub struct ExtendWindowAggColSpec;

impl NativeFunction for ExtendWindowAggColSpec {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("extend (Relation, _Window, AggColSpec)", args, 3)?;
        let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);

        // -- Source TDS --------------------------------------------------
        let rel_value = ctx.evaluate(&args[0])?.into_value();
        let tds_obj = unwrap_instance_value(&rel_value, instance_value_id, ctx)?;
        let parsed = read_parsed_tds("extend", &tds_obj, ctx)?;

        // -- Window: partition + sortInfo + frame ------------------------
        let window_value = ctx.evaluate(&args[1])?.into_value();
        let window_obj = unwrap_instance_value(&window_value, instance_value_id, ctx)?;
        let partition_cols = read_partition_cols(&window_obj, ctx)?;
        let sort_keys = read_sort_keys(&window_obj, ctx)?;
        let frame = read_frame(&window_obj, ctx)?;

        let partition_indices = resolve_partition_indices(
            &partition_cols,
            &parsed,
            "extend (Relation, _Window, AggColSpec)",
        )?;
        let sort_indices = resolve_sort_indices(
            &sort_keys,
            &parsed,
            "extend (Relation, _Window, AggColSpec)",
        )?;

        // -- AggColSpec: name + map + reduce -----------------------------
        let acs_value = ctx.evaluate(&args[2])?.into_value();
        let acs_obj = unwrap_instance_value(&acs_value, instance_value_id, ctx)?;
        let name_values = ctx
            .heap()
            .get_property_values(&acs_obj, "name")
            .map_err(PureException::from)?;
        let new_col_name: SmolStr = name_values
            .iter()
            .find_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                PureException::from(PureRuntimeError::EvaluationError(
                    "extend (Relation, _Window, AggColSpec): AggColSpec.name slot missing"
                        .into(),
                ))
            })?;
        let map_fn = read_function_slot(&acs_obj, "map", ctx, "AggColSpec.map")?;
        let reduce_fn = read_function_slot(&acs_obj, "reduce", ctx, "AggColSpec.reduce")?;

        // -- Receiver-Relation value passed as map-lambda arg 0.
        //    Map lambda signature: {Relation, _Window, T-row -> K[0..1]}.
        //    Pass the source TDS heap object (post InstanceValue unwrap)
        //    so the lambda can `.col` access the relation's reflective
        //    slots if it wants — most PCT bodies ignore arg0/arg1 and
        //    only use the row.
        let rel_arg = Value::Object(tds_obj.clone());
        let window_arg = Value::Object(window_obj.clone());

        // -- Per-row map: compute K once per source row. Map is invariant
        //    of window position (it operates on the row, not the frame),
        //    so we can compute every row's K value upfront and slice
        //    these by frame indices in the reduce step.
        let mut map_values: Vec<Value> = Vec::with_capacity(parsed.rows.len());
        for row in &parsed.rows {
            let row_tuple = build_row_tuple(&parsed.columns, row, ctx)?;
            let k = ctx.call_function(
                &map_fn,
                &[rel_arg.clone(), window_arg.clone(), Value::Object(row_tuple)],
            )?;
            map_values.push(k);
        }

        // -- Partition + sort + per-row position --------------------------
        // Group row indices by partition key (preserves source order
        // within each group), then stable-sort each group by sortInfo.
        // After this, partitions[i].1[j] = source-row index at sorted
        // position j of partition i.
        let mut partitions = partition_row_indices(&parsed, &partition_indices);
        sort_partitions_in_place(&mut partitions, &parsed, &sort_indices);

        // Reverse-index: source-row index -> (partition idx, position-in-partition).
        let mut row_position: Vec<(usize, usize)> = vec![(0, 0); parsed.rows.len()];
        for (p_idx, (_, indices)) in partitions.iter().enumerate() {
            for (pos, &src) in indices.iter().enumerate() {
                row_position[src] = (p_idx, pos);
            }
        }

        // -- Per-row reduce: each row gets V from its frame's K values --
        //
        // Empty-frame (no non-null map results) -> emit `None` without
        // calling reduce. Mirrors Java
        // `AggregationShared.processAggregation`: when the per-row
        // aggregation collection is empty (e.g. every map output was
        // dropped as Unit by push_flat), the setter is given `null`
        // directly rather than dispatching `reduce(empty)` which —
        // depending on the reduce body — would synthesise a zero
        // (e.g. `plus()` of nothing -> 0, `joinStrings()` of nothing
        // -> ""). The PCT corpus expects null in both cases.
        let mut new_cells: Vec<Option<TypedCell>> = Vec::with_capacity(parsed.rows.len());
        for src_idx in 0..parsed.rows.len() {
            let (p_idx, position) = row_position[src_idx];
            let partition_indices_sorted = &partitions[p_idx].1;
            let in_frame = frame_row_indices(partition_indices_sorted, position, frame.as_ref());

            let mut collection: PVector<Value> = PVector::new();
            for &i in &in_frame {
                push_flat(&mut collection, &map_values[i]);
            }
            if collection.is_empty() {
                new_cells.push(None);
                continue;
            }
            let collection_arg = if collection.len() == 1 {
                collection.iter().next().cloned().unwrap_or(Value::Unit)
            } else {
                Value::Collection(Box::new(collection))
            };
            let v = ctx.call_function(&reduce_fn, &[collection_arg])?;
            new_cells.push(value_to_typed_cell(&v));
        }

        // -- New column metadata + extended rows -------------------------
        let (inferred_type, inferred_mult) = infer_column_type_and_mult(&new_cells);
        let mut new_columns = parsed.columns.clone();
        new_columns.push(ParsedColumn {
            name: new_col_name,
            type_tag: inferred_type,
            multiplicity: inferred_mult,
        });
        let mut new_rows: Vec<Vec<Option<TypedCell>>> = Vec::with_capacity(parsed.rows.len());
        for (row, new_cell) in parsed.rows.iter().zip(new_cells.into_iter()) {
            let mut extended = row.clone();
            extended.push(new_cell);
            new_rows.push(extended);
        }

        let result = ParsedTDS {
            csv: String::new(),
            columns: new_columns,
            rows: new_rows,
        };
        let new_tds = alloc_tds_from_parsed(ctx, &result)?;
        Ok(Evaluated::new(Value::Object(new_tds)))
    }
}

/// `extend<T,V,Z>(r:Relation<T>[1], w:_Window<T>[1],
///     fcs:FuncColSpec<{Relation<T>[1],_Window<T>[1],T[1]->V[*]},Z>[1]
/// ):Relation<T+Z>[1]`.
///
/// Per-row dispatch: the FuncColSpec lambda receives `(rel, window,
/// row_tuple)` and returns a value the new column will carry. Unlike
/// the AggColSpec variant, there is no reduce step — the lambda body
/// is responsible for any aggregation it wants (e.g. by calling the
/// standalone `reduce` native, which is the canonical reduce.pure PCT
/// shape).
#[derive(Debug)]
pub struct ExtendWindowFuncColSpec;

impl NativeFunction for ExtendWindowFuncColSpec {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        expect_args("extend (Relation, _Window, FuncColSpec)", args, 3)?;
        let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);

        // -- Source TDS --------------------------------------------------
        let rel_value = ctx.evaluate(&args[0])?.into_value();
        let tds_obj = unwrap_instance_value(&rel_value, instance_value_id, ctx)?;
        let parsed = read_parsed_tds("extend", &tds_obj, ctx)?;

        // -- Window: partition + sortInfo. The map lambda receives the
        //    *sorted partition sub-TDS* as arg0 and a row tuple carrying
        //    its within-partition position — the calling convention the
        //    ranking natives (`rowNumber`, `rank`, …) and the standalone
        //    `reduce` rely on (mirrors Java `RowContainer(winTDS, i)`).
        let window_value = ctx.evaluate(&args[1])?.into_value();
        let window_obj = unwrap_instance_value(&window_value, instance_value_id, ctx)?;
        let partition_cols = read_partition_cols(&window_obj, ctx)?;
        let sort_keys = read_sort_keys(&window_obj, ctx)?;
        let partition_indices = resolve_partition_indices(
            &partition_cols,
            &parsed,
            "extend (Relation, _Window, FuncColSpec)",
        )?;
        let sort_indices = resolve_sort_indices(
            &sort_keys,
            &parsed,
            "extend (Relation, _Window, FuncColSpec)",
        )?;

        // -- FuncColSpec: name + function -------------------------------
        let fcs_value = ctx.evaluate(&args[2])?.into_value();
        let fcs_obj = unwrap_instance_value(&fcs_value, instance_value_id, ctx)?;
        let name_values = ctx
            .heap()
            .get_property_values(&fcs_obj, "name")
            .map_err(PureException::from)?;
        let new_col_name: SmolStr = name_values
            .iter()
            .find_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                PureException::from(PureRuntimeError::EvaluationError(
                    "extend (Relation, _Window, FuncColSpec): FuncColSpec.name slot missing"
                        .into(),
                ))
            })?;
        let function_value = read_function_slot(&fcs_obj, "function", ctx, "FuncColSpec.function")?;

        let window_arg = Value::Object(window_obj.clone());

        // -- Partition + sort, then per-partition / per-row dispatch -----
        // Group row indices by partition key (source order preserved),
        // stable-sort each group by sortInfo.
        let mut partitions = partition_row_indices(&parsed, &partition_indices);
        sort_partitions_in_place(&mut partitions, &parsed, &sort_indices);

        // Result column, placed back at each row's ORIGINAL source index.
        let mut new_cells: Vec<Option<TypedCell>> = vec![None; parsed.rows.len()];
        for (_key, sorted_indices) in &partitions {
            // Partition sub-TDS: this partition's rows in sort order.
            let partition_parsed = ParsedTDS {
                csv: String::new(),
                columns: parsed.columns.clone(),
                rows: sorted_indices.iter().map(|&i| parsed.rows[i].clone()).collect(),
            };
            let partition_tds = alloc_tds_from_parsed(ctx, &partition_parsed)?;
            let partition_arg = Value::Object(partition_tds);

            for (pos, &src_idx) in sorted_indices.iter().enumerate() {
                let row_tuple = build_row_tuple(&parsed.columns, &parsed.rows[src_idx], ctx)?;
                attach_row_index(&row_tuple, pos, ctx)?;
                let val = ctx.call_function(
                    &function_value,
                    &[
                        partition_arg.clone(),
                        window_arg.clone(),
                        Value::Object(row_tuple),
                    ],
                )?;
                new_cells[src_idx] = value_to_typed_cell(&val);
            }
        }

        let (inferred_type, inferred_mult) = infer_column_type_and_mult(&new_cells);
        let mut new_columns = parsed.columns.clone();
        new_columns.push(ParsedColumn {
            name: new_col_name,
            type_tag: inferred_type,
            multiplicity: inferred_mult,
        });
        let mut new_rows: Vec<Vec<Option<TypedCell>>> = Vec::with_capacity(parsed.rows.len());
        for (row, new_cell) in parsed.rows.iter().zip(new_cells.into_iter()) {
            let mut extended = row.clone();
            extended.push(new_cell);
            new_rows.push(extended);
        }

        let result = ParsedTDS {
            csv: String::new(),
            columns: new_columns,
            rows: new_rows,
        };
        let new_tds = alloc_tds_from_parsed(ctx, &result)?;
        Ok(Evaluated::new(Value::Object(new_tds)))
    }
}

#[allow(clippy::result_large_err)]
fn read_function_slot(
    obj: &legend_pure_runtime::heap::ObjectHandle,
    slot: &str,
    ctx: &mut dyn EvalContextTrait,
    label: &'static str,
) -> Result<Value, PureException> {
    let values = ctx
        .heap()
        .get_property_values(obj, slot)
        .map_err(PureException::from)?;
    values
        .iter()
        .find(|v| matches!(v, Value::Function(_)))
        .cloned()
        .ok_or_else(|| {
            PureException::from(PureRuntimeError::EvaluationError(format!(
                "extend (Relation, _Window, AggColSpec): {label} slot missing or not a Function"
            )))
        })
}

/// Append a Value (possibly a Collection) to `out`, flattening one
/// Collection layer and dropping Unit. Mirrors the same shape natives
/// like `filter` / `map` rely on.
fn push_flat(out: &mut PVector<Value>, v: &Value) {
    match v {
        Value::Collection(inner) => {
            for item in inner.iter() {
                out.push_back(item.clone());
            }
        }
        Value::Unit => {}
        scalar => out.push_back(scalar.clone()),
    }
}

fn value_to_typed_cell(value: &Value) -> Option<TypedCell> {
    match value {
        Value::Unit => None,
        Value::Integer(i) => Some(TypedCell::Integer(*i)),
        Value::Float(f) => Some(TypedCell::Float(*f)),
        Value::Boolean(b) => Some(TypedCell::Boolean(*b)),
        Value::String(s) => Some(TypedCell::String(s.clone())),
        _ => None,
    }
}

fn infer_column_type_and_mult(cells: &[Option<TypedCell>]) -> (ColumnType, Multiplicity) {
    let mut tag: Option<ColumnType> = None;
    let mut any_empty = false;
    for cell in cells {
        let Some(cell) = cell else {
            any_empty = true;
            continue;
        };
        let cell_tag = match cell {
            TypedCell::Integer(_) => ColumnType::Integer,
            TypedCell::Float(_) => ColumnType::Float,
            TypedCell::Boolean(_) => ColumnType::Boolean,
            TypedCell::String(_) => ColumnType::String,
            TypedCell::Decimal(_) => ColumnType::Decimal,
            TypedCell::StrictDate(_) => ColumnType::StrictDate,
            TypedCell::DateTime(_) => ColumnType::DateTime,
        };
        tag = Some(match tag {
            None => cell_tag,
            Some(prev) if prev == cell_tag => prev,
            _ => ColumnType::String,
        });
    }
    let mult = if any_empty {
        Multiplicity::ZeroOrOne
    } else {
        Multiplicity::PureOne
    };
    (tag.unwrap_or(ColumnType::String), mult)
}
