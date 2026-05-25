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
use legend_pure_dsl_tds::csv::{ColumnType, ParsedColumn, TypedCell};
use legend_pure_parser_pure::types::{Multiplicity, ValueSpec};
use smol_str::SmolStr;

use legend_pure_runtime::error::{PureException, PureRuntimeError};
use legend_pure_runtime::m3_paths;
use legend_pure_runtime::native::{EvalContextTrait, Evaluated, NativeFunction, expect_args};
use legend_pure_runtime::native::relation::shared::{
    build_row_tuple, read_parsed_tds, render_csv_from_columns_and_rows, unwrap_instance_value,
};
use legend_pure_runtime::value::Value;

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

        // -- Window: partition column names ------------------------------
        let window_value = ctx.evaluate(&args[1])?.into_value();
        let window_obj = unwrap_instance_value(&window_value, instance_value_id, ctx)?;
        let partition_values = ctx
            .heap()
            .get_property_values(&window_obj, "partition")
            .map_err(PureException::from)?;
        let partition_cols: Vec<SmolStr> = partition_values
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect();

        // Resolve partition column names to their indices in the source
        // TDS so partition-key tuples are cheap to assemble per row.
        let partition_indices: Vec<usize> = partition_cols
            .iter()
            .map(|name| {
                parsed
                    .columns
                    .iter()
                    .position(|c| c.name.as_str() == name.as_str())
                    .ok_or_else(|| {
                        let available: Vec<&str> =
                            parsed.columns.iter().map(|c| c.name.as_str()).collect();
                        PureException::from(PureRuntimeError::EvaluationError(format!(
                            "extend (Relation, _Window, AggColSpec): partition column \
                             '{name}' not present in receiver; have {available:?}"
                        )))
                    })
            })
            .collect::<Result<_, _>>()?;

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

        // -- Per-row map: assemble partition key + compute K value ------
        //    Partition-key tuple uses Debug repr of each cell — String /
        //    Integer / Float / Boolean / Date / None all formatted
        //    uniquely. Same partition-key shape we use for distinct
        //    dedup (linear equality over Option<TypedCell>).
        let mut partition_keys: Vec<Vec<Option<TypedCell>>> = Vec::with_capacity(parsed.rows.len());
        let mut map_values: Vec<Value> = Vec::with_capacity(parsed.rows.len());
        for row in &parsed.rows {
            let key: Vec<Option<TypedCell>> =
                partition_indices.iter().map(|&i| row[i].clone()).collect();
            partition_keys.push(key);
            let row_tuple = build_row_tuple(&parsed.columns, row, ctx)?;
            let k = ctx.call_function(
                &map_fn,
                &[rel_arg.clone(), window_arg.clone(), Value::Object(row_tuple)],
            )?;
            map_values.push(k);
        }

        // -- Per-partition group: collect K values per unique partition --
        //    Quadratic on row count (linear scan to find the matching
        //    partition group). Fine for current PCT sizes; switch to a
        //    hash-keyed group when a workload needs it.
        let mut partition_groups: Vec<(Vec<Option<TypedCell>>, PVector<Value>)> = Vec::new();
        for (key, kval) in partition_keys.iter().zip(map_values.iter()) {
            if let Some(slot) = partition_groups
                .iter_mut()
                .find(|(k, _)| k == key)
                .map(|(_, vs)| vs)
            {
                push_flat(slot, kval);
            } else {
                let mut vs: PVector<Value> = PVector::new();
                push_flat(&mut vs, kval);
                partition_groups.push((key.clone(), vs));
            }
        }

        // -- Per-row reduce: each row gets the V from its partition -----
        let mut new_cells: Vec<Option<TypedCell>> = Vec::with_capacity(parsed.rows.len());
        for key in &partition_keys {
            let collection = partition_groups
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, vs)| vs.clone())
                .unwrap_or_default();
            // Reduce lambda is `K[*]->V[0..1]`. Pass a Collection
            // (or scalar/Unit for the degenerate 1/0-element cases).
            let collection_arg = if collection.is_empty() {
                Value::Unit
            } else if collection.len() == 1 {
                collection
                    .iter()
                    .next()
                    .cloned()
                    .unwrap_or(Value::Unit)
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

        let new_csv = render_csv_from_columns_and_rows(&new_columns, &new_rows);
        let new_tds = ctx.heap_mut().alloc_dynamic(m3_paths::TDS);
        ctx.heap_mut()
            .mutate_add(&new_tds, "csv", &[Value::String(new_csv.into())])
            .map_err(PureException::from)?;
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
