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
use legend_pure_parser_pure::types::{ColSpecLiteralKind, ExprKind, Multiplicity, ValueSpec};
use smol_str::SmolStr;

use legend_pure_runtime::error::{PureException, PureRuntimeError};
use legend_pure_runtime::m3_paths;
use legend_pure_runtime::native::{EvalContextTrait, Evaluated, NativeFunction, expect_args};
use legend_pure_runtime::heap::ObjectHandle;
use legend_pure_runtime::native::relation::shared::{
    alloc_tds_from_parsed, build_row_tuple, read_parsed_tds, unwrap_instance_value,
};
use legend_pure_runtime::value::Value;

use crate::window_runtime::{
    Frame, attach_row_index, effective_frame, frame_row_indices, partition_row_indices, push_flat,
    read_frame, read_partition_cols, read_sort_keys, resolve_partition_indices,
    resolve_sort_indices, sort_partitions_in_place,
};

/// `(partitions, row_position)` — partition the receiver by the window's
/// partition columns (source order preserved within each group),
/// stable-sort each group by sortInfo, and build the reverse index
/// `source-row -> (partition idx, position-in-sorted-partition)`.
type PartitionLayout = (Vec<(Vec<Option<TypedCell>>, Vec<usize>)>, Vec<(usize, usize)>);

fn partition_layout(
    parsed: &ParsedTDS,
    partition_indices: &[usize],
    sort_indices: &[(usize, crate::window_runtime::SortDir)],
) -> PartitionLayout {
    let mut partitions = partition_row_indices(parsed, partition_indices);
    sort_partitions_in_place(&mut partitions, parsed, sort_indices);
    let mut row_position: Vec<(usize, usize)> = vec![(0, 0); parsed.rows.len()];
    for (p_idx, (_, indices)) in partitions.iter().enumerate() {
        for (pos, &src) in indices.iter().enumerate() {
            row_position[src] = (p_idx, pos);
        }
    }
    (partitions, row_position)
}

/// Compute one OLAP aggregate column: run `map_fn` per source row
/// (`{Relation, _Window, row -> K[0..1]}`), then per row reduce its
/// frame's K collection via `reduce_fn` (`{K[*] -> V[0..1]}`). Returns
/// the column cells in source-row order. Empty-frame rows emit `None`
/// (no `reduce(empty)` call) — see the AggColSpec impl note.
#[allow(clippy::result_large_err, clippy::too_many_arguments)]
fn compute_windowed_agg_column(
    parsed: &ParsedTDS,
    partitions: &[(Vec<Option<TypedCell>>, Vec<usize>)],
    row_position: &[(usize, usize)],
    frame: Option<&Frame>,
    map_fn: &Value,
    reduce_fn: &Value,
    rel_arg: &Value,
    window_arg: &Value,
    ctx: &mut dyn EvalContextTrait,
) -> Result<Vec<Option<TypedCell>>, PureException> {
    // Map is invariant of window position — compute every row's K once,
    // then slice by frame indices in the reduce step.
    let mut map_values: Vec<Value> = Vec::with_capacity(parsed.rows.len());
    for row in &parsed.rows {
        let row_tuple = build_row_tuple(&parsed.columns, row, ctx)?;
        let k = ctx.call_function(
            map_fn,
            &[rel_arg.clone(), window_arg.clone(), Value::Object(row_tuple)],
        )?;
        map_values.push(k);
    }

    let mut cells: Vec<Option<TypedCell>> = Vec::with_capacity(parsed.rows.len());
    for src_idx in 0..parsed.rows.len() {
        let (p_idx, position) = row_position[src_idx];
        let in_frame = frame_row_indices(&partitions[p_idx].1, position, frame);
        let mut collection: PVector<Value> = PVector::new();
        for &i in &in_frame {
            push_flat(&mut collection, &map_values[i]);
        }
        if collection.is_empty() {
            cells.push(None);
            continue;
        }
        let collection_arg = if collection.len() == 1 {
            collection.iter().next().cloned().unwrap_or(Value::Unit)
        } else {
            Value::Collection(Box::new(collection))
        };
        let v = ctx.call_function(reduce_fn, &[collection_arg])?;
        cells.push(value_to_typed_cell(&v));
    }
    Ok(cells)
}

/// Append a set of computed columns (each `(name, source-order cells)`)
/// to `parsed`'s rows and re-materialise as a fresh TDS heap object.
#[allow(clippy::result_large_err)]
fn append_columns_and_alloc(
    parsed: &ParsedTDS,
    new_cols: Vec<(SmolStr, Vec<Option<TypedCell>>)>,
    ctx: &mut dyn EvalContextTrait,
) -> Result<ObjectHandle, PureException> {
    let mut columns = parsed.columns.clone();
    for (name, cells) in &new_cols {
        let (inferred_type, inferred_mult) = infer_column_type_and_mult(cells);
        columns.push(ParsedColumn {
            name: name.clone(),
            type_tag: inferred_type,
            multiplicity: inferred_mult,
        });
    }
    let mut rows: Vec<Vec<Option<TypedCell>>> = Vec::with_capacity(parsed.rows.len());
    for (row_idx, row) in parsed.rows.iter().enumerate() {
        let mut extended = row.clone();
        for (_, cells) in &new_cols {
            extended.push(cells[row_idx].clone());
        }
        rows.push(extended);
    }
    let result = ParsedTDS {
        csv: String::new(),
        columns,
        rows,
    };
    alloc_tds_from_parsed(ctx, &result)
}

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
        // Map lambda signature: {Relation, _Window, T-row -> K[0..1]}.
        // Pass the full source TDS as arg0 (most PCT bodies ignore
        // arg0/arg1 and only read the row); reduce frames per row.
        let rel_arg = Value::Object(tds_obj.clone());
        let window_arg = Value::Object(window_obj.clone());

        let frame = effective_frame(frame, !sort_indices.is_empty());
        let (partitions, row_position) =
            partition_layout(&parsed, &partition_indices, &sort_indices);
        let new_cells = compute_windowed_agg_column(
            &parsed,
            &partitions,
            &row_position,
            frame.as_ref(),
            &map_fn,
            &reduce_fn,
            &rel_arg,
            &window_arg,
            ctx,
        )?;

        let new_tds = append_columns_and_alloc(&parsed, vec![(new_col_name, new_cells)], ctx)?;
        Ok(Evaluated::new(Value::Object(new_tds)))
    }
}

/// `extend(Relation, _Window, AggColSpecArray)` — multi-column OLAP
/// aggregate. Same per-column semantics as [`ExtendWindowAggColSpec`],
/// applied to each `~[name:map:reduce, …]` entry, sharing one
/// partition + sort layout across all columns.
///
/// Reads the column triples from the AST (`ColSpecArrayLiteral`,
/// `Agg` kind) — the `~[…]` heap form retains only column names, not
/// the map/reduce lambdas, so (like the non-window
/// `ExtendFuncColSpecArray`) only a direct `~[…]` literal argument is
/// supported, not a let-bound / function-returned ColSpecArray.
#[derive(Debug)]
pub struct ExtendWindowAggColSpecArray;

impl NativeFunction for ExtendWindowAggColSpecArray {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        const LABEL: &str = "extend (Relation, _Window, AggColSpecArray)";
        expect_args(LABEL, args, 3)?;
        let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);

        let rel_value = ctx.evaluate(&args[0])?.into_value();
        let tds_obj = unwrap_instance_value(&rel_value, instance_value_id, ctx)?;
        let parsed = read_parsed_tds("extend", &tds_obj, ctx)?;

        let window_value = ctx.evaluate(&args[1])?.into_value();
        let window_obj = unwrap_instance_value(&window_value, instance_value_id, ctx)?;
        let partition_cols = read_partition_cols(&window_obj, ctx)?;
        let sort_keys = read_sort_keys(&window_obj, ctx)?;
        let frame = read_frame(&window_obj, ctx)?;
        let partition_indices = resolve_partition_indices(&partition_cols, &parsed, LABEL)?;
        let sort_indices = resolve_sort_indices(&sort_keys, &parsed, LABEL)?;

        // -- AggColSpecArray columns from the AST: (name, map, reduce) --
        let cols_ast = match args[2].kind.as_ref() {
            ExprKind::ColSpecArrayLiteral {
                columns,
                kind: ColSpecLiteralKind::Agg,
            } => columns,
            other => {
                return Err(PureException::from(PureRuntimeError::EvaluationError(format!(
                    "{LABEL}: arg 3 must be a direct ~[name:map:reduce, …] literal; got {other:?}"
                ))));
            }
        };
        let mut columns: Vec<(SmolStr, Value, Value)> = Vec::with_capacity(cols_ast.len());
        for col in cols_ast {
            let (Some(map_vs), Some(reduce_vs)) =
                (col.init_lambda.as_ref(), col.reduce_lambda.as_ref())
            else {
                return Err(PureException::from(PureRuntimeError::EvaluationError(format!(
                    "{LABEL}: column '{}' must carry both a map and a reduce lambda",
                    col.name
                ))));
            };
            let map_fn = ctx.evaluate(map_vs)?.into_value();
            let reduce_fn = ctx.evaluate(reduce_vs)?.into_value();
            columns.push((col.name.clone(), map_fn, reduce_fn));
        }

        let rel_arg = Value::Object(tds_obj.clone());
        let window_arg = Value::Object(window_obj.clone());
        let frame = effective_frame(frame, !sort_indices.is_empty());
        let (partitions, row_position) =
            partition_layout(&parsed, &partition_indices, &sort_indices);

        let mut new_cols: Vec<(SmolStr, Vec<Option<TypedCell>>)> =
            Vec::with_capacity(columns.len());
        for (name, map_fn, reduce_fn) in &columns {
            let cells = compute_windowed_agg_column(
                &parsed,
                &partitions,
                &row_position,
                frame.as_ref(),
                map_fn,
                reduce_fn,
                &rel_arg,
                &window_arg,
                ctx,
            )?;
            new_cols.push((name.clone(), cells));
        }

        let new_tds = append_columns_and_alloc(&parsed, new_cols, ctx)?;
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
        let (partitions, _) = partition_layout(&parsed, &partition_indices, &sort_indices);
        let mut cols = dispatch_windowed_func_columns(
            &parsed,
            &partitions,
            &[function_value],
            &window_arg,
            ctx,
        )?;
        let new_cells = cols.pop().unwrap_or_default();

        let new_tds = append_columns_and_alloc(&parsed, vec![(new_col_name, new_cells)], ctx)?;
        Ok(Evaluated::new(Value::Object(new_tds)))
    }
}

/// `extend(Relation, _Window, FuncColSpecArray)` — multi-column
/// windowed per-row dispatch. Same partition-scoped calling convention
/// as [`ExtendWindowFuncColSpec`] (each map lambda receives the sorted
/// partition sub-TDS + the row's within-partition position), applied to
/// every `~[name:{p,w,r|…}, …]` entry, sharing one partition + sort.
///
/// Reads column lambdas from the AST (`ColSpecArrayLiteral`, `Func`
/// kind) — the `~[…]` heap form keeps only names, so only a direct
/// `~[…]` literal argument is supported.
#[derive(Debug)]
pub struct ExtendWindowFuncColSpecArray;

impl NativeFunction for ExtendWindowFuncColSpecArray {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        const LABEL: &str = "extend (Relation, _Window, FuncColSpecArray)";
        expect_args(LABEL, args, 3)?;
        let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);

        let rel_value = ctx.evaluate(&args[0])?.into_value();
        let tds_obj = unwrap_instance_value(&rel_value, instance_value_id, ctx)?;
        let parsed = read_parsed_tds("extend", &tds_obj, ctx)?;

        let window_value = ctx.evaluate(&args[1])?.into_value();
        let window_obj = unwrap_instance_value(&window_value, instance_value_id, ctx)?;
        let partition_cols = read_partition_cols(&window_obj, ctx)?;
        let sort_keys = read_sort_keys(&window_obj, ctx)?;
        let partition_indices = resolve_partition_indices(&partition_cols, &parsed, LABEL)?;
        let sort_indices = resolve_sort_indices(&sort_keys, &parsed, LABEL)?;

        // -- FuncColSpecArray columns from the AST: (name, function) ----
        let cols_ast = match args[2].kind.as_ref() {
            ExprKind::ColSpecArrayLiteral {
                columns,
                kind: ColSpecLiteralKind::Func,
            } => columns,
            other => {
                return Err(PureException::from(PureRuntimeError::EvaluationError(format!(
                    "{LABEL}: arg 3 must be a direct ~[name:{{p,w,r|…}}, …] literal; got {other:?}"
                ))));
            }
        };
        let mut names: Vec<SmolStr> = Vec::with_capacity(cols_ast.len());
        let mut fns: Vec<Value> = Vec::with_capacity(cols_ast.len());
        for col in cols_ast {
            let Some(init) = col.init_lambda.as_ref() else {
                return Err(PureException::from(PureRuntimeError::EvaluationError(format!(
                    "{LABEL}: column '{}' has no init lambda",
                    col.name
                ))));
            };
            names.push(col.name.clone());
            fns.push(ctx.evaluate(init)?.into_value());
        }

        let window_arg = Value::Object(window_obj.clone());
        let (partitions, _) = partition_layout(&parsed, &partition_indices, &sort_indices);
        let cols = dispatch_windowed_func_columns(&parsed, &partitions, &fns, &window_arg, ctx)?;

        let new_cols: Vec<(SmolStr, Vec<Option<TypedCell>>)> =
            names.into_iter().zip(cols).collect();
        let new_tds = append_columns_and_alloc(&parsed, new_cols, ctx)?;
        Ok(Evaluated::new(Value::Object(new_tds)))
    }
}

/// Per-partition / per-row dispatch for windowed `FuncColSpec` columns.
/// For each partition (already sorted) builds a sub-TDS, then for each
/// row at position `pos` builds a row tuple tagged with `__row_index`
/// and invokes every column lambda with `(partition_tds, window, row)`.
/// Returns one source-order cell vec per column (same order as
/// `column_fns`).
#[allow(clippy::result_large_err)]
fn dispatch_windowed_func_columns(
    parsed: &ParsedTDS,
    partitions: &[(Vec<Option<TypedCell>>, Vec<usize>)],
    column_fns: &[Value],
    window_arg: &Value,
    ctx: &mut dyn EvalContextTrait,
) -> Result<Vec<Vec<Option<TypedCell>>>, PureException> {
    let mut cols: Vec<Vec<Option<TypedCell>>> =
        vec![vec![None; parsed.rows.len()]; column_fns.len()];
    for (_key, sorted_indices) in partitions {
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
            let row_arg = Value::Object(row_tuple);
            for (col_idx, func) in column_fns.iter().enumerate() {
                let val = ctx.call_function(
                    func,
                    &[partition_arg.clone(), window_arg.clone(), row_arg.clone()],
                )?;
                cols[col_idx][src_idx] = value_to_typed_cell(&val);
            }
        }
    }
    Ok(cols)
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
