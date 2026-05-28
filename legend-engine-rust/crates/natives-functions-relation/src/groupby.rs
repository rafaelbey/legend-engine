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

//! `groupBy` natives — collapse a relation to one row per group key:
//!
//! ```pure
//! native groupBy<T,Z,K,V,R>(r:Relation<T>[1], cols:ColSpec<Z⊆T>[1],
//!     agg:AggColSpec<{T[1]->K[0..1]},{K[*]->V[0..1]}, R>[1]):Relation<Z+R>[1];
//! native groupBy<...>(r, cols:ColSpecArray<Z⊆T>[1], agg:AggColSpec<...>):Relation<Z+R>[1];
//! native groupBy<...>(r, cols:ColSpec<Z⊆T>[1], agg:AggColSpecArray<...>):Relation<Z+R>[1];
//! native groupBy<...>(r, cols:ColSpecArray<Z⊆T>[1], agg:AggColSpecArray<...>):Relation<Z+R>[1];
//! ```
//!
//! Partition the source rows by the group-column value tuple (first-
//! occurrence order preserved); for each group, run each agg's `map`
//! (`{T[1]->K[0..1]}`) over its rows, then `reduce` (`{K[*]->V[0..1]}`)
//! the collected K values to one V. Emit one result row per group:
//! the group-key columns followed by the aggregate columns. Result
//! schema is `Z + R` (group cols + agg cols).
//!
//! Group-column names come from the heap: `ColSpec.name` (single) or
//! `ColSpecArray.names` (array). Single `AggColSpec`s carry their
//! `map`/`reduce` lambdas on the heap; `AggColSpecArray` keeps only
//! names on the heap, so its per-column lambdas are read from the AST
//! (`ColSpecArrayLiteral`, `Agg` kind) — same constraint as the
//! windowed array extends (direct `~[…]` literal only).

#![allow(clippy::needless_pass_by_value)]

use im_rc::Vector as PVector;
use legend_pure_dsl_tds::csv::{ColumnType, ParsedColumn, ParsedTDS, TypedCell};
use legend_pure_parser_pure::types::{ColSpecLiteralKind, ExprKind, Multiplicity, ValueSpec};
use smol_str::SmolStr;

use legend_pure_runtime::error::{PureException, PureRuntimeError};
use legend_pure_runtime::heap::ObjectHandle;
use legend_pure_runtime::m3_paths;
use legend_pure_runtime::native::relation::shared::{
    alloc_tds_from_parsed, build_row_tuple, read_parsed_tds, unwrap_instance_value,
};
use legend_pure_runtime::native::{EvalContextTrait, Evaluated, NativeFunction, expect_args};
use legend_pure_runtime::value::Value;

/// One aggregate column: its result name + the already-evaluated
/// `map` / `reduce` lambdas.
struct AggSpec {
    name: SmolStr,
    map: Value,
    reduce: Value,
}

// ---------------------------------------------------------------------------
// Native structs — one per (group-col shape, agg shape) overload.
// ---------------------------------------------------------------------------

/// `groupBy(Relation, ColSpec, AggColSpec)`.
#[derive(Debug)]
pub struct GroupByColSpecAgg;

impl NativeFunction for GroupByColSpecAgg {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        const LABEL: &str = "groupBy (Relation, ColSpec, AggColSpec)";
        expect_args(LABEL, args, 3)?;
        let parsed = source_relation(&args[0], ctx)?;
        let group_cols = group_names_from_colspec(&args[1], ctx, LABEL)?;
        let aggs = vec![agg_from_heap(&args[2], ctx, LABEL)?];
        run_group_by(&parsed, &group_cols, &aggs, ctx, LABEL)
    }
}

/// `groupBy(Relation, ColSpecArray, AggColSpec)`.
#[derive(Debug)]
pub struct GroupByColSpecArrayAgg;

impl NativeFunction for GroupByColSpecArrayAgg {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        const LABEL: &str = "groupBy (Relation, ColSpecArray, AggColSpec)";
        expect_args(LABEL, args, 3)?;
        let parsed = source_relation(&args[0], ctx)?;
        let group_cols = group_names_from_colspec_array(&args[1], ctx, LABEL)?;
        let aggs = vec![agg_from_heap(&args[2], ctx, LABEL)?];
        run_group_by(&parsed, &group_cols, &aggs, ctx, LABEL)
    }
}

/// `groupBy(Relation, ColSpec, AggColSpecArray)`.
#[derive(Debug)]
pub struct GroupByColSpecAggArray;

impl NativeFunction for GroupByColSpecAggArray {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        const LABEL: &str = "groupBy (Relation, ColSpec, AggColSpecArray)";
        expect_args(LABEL, args, 3)?;
        let parsed = source_relation(&args[0], ctx)?;
        let group_cols = group_names_from_colspec(&args[1], ctx, LABEL)?;
        let aggs = aggs_from_ast(&args[2], ctx, LABEL)?;
        run_group_by(&parsed, &group_cols, &aggs, ctx, LABEL)
    }
}

/// `groupBy(Relation, ColSpecArray, AggColSpecArray)`.
#[derive(Debug)]
pub struct GroupByColSpecArrayAggArray;

impl NativeFunction for GroupByColSpecArrayAggArray {
    fn execute(
        &self,
        args: &[ValueSpec],
        ctx: &mut dyn EvalContextTrait,
    ) -> Result<Evaluated, PureException> {
        const LABEL: &str = "groupBy (Relation, ColSpecArray, AggColSpecArray)";
        expect_args(LABEL, args, 3)?;
        let parsed = source_relation(&args[0], ctx)?;
        let group_cols = group_names_from_colspec_array(&args[1], ctx, LABEL)?;
        let aggs = aggs_from_ast(&args[2], ctx, LABEL)?;
        run_group_by(&parsed, &group_cols, &aggs, ctx, LABEL)
    }
}

// ---------------------------------------------------------------------------
// Argument decoding
// ---------------------------------------------------------------------------

#[allow(clippy::result_large_err)]
fn source_relation(
    arg: &ValueSpec,
    ctx: &mut dyn EvalContextTrait,
) -> Result<ParsedTDS, PureException> {
    let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);
    let rel_value = ctx.evaluate(arg)?.into_value();
    let tds_obj = unwrap_instance_value(&rel_value, instance_value_id, ctx)?;
    read_parsed_tds("groupBy", &tds_obj, ctx)
}

#[allow(clippy::result_large_err)]
fn group_names_from_colspec(
    arg: &ValueSpec,
    ctx: &mut dyn EvalContextTrait,
    label: &str,
) -> Result<Vec<SmolStr>, PureException> {
    let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);
    let cs_value = ctx.evaluate(arg)?.into_value();
    let cs_obj = unwrap_instance_value(&cs_value, instance_value_id, ctx)?;
    let names = ctx
        .heap()
        .get_property_values(&cs_obj, "name")
        .map_err(PureException::from)?;
    let name = names.iter().find_map(|v| match v {
        Value::String(s) => Some(s.clone()),
        _ => None,
    });
    name.map(|n| vec![n]).ok_or_else(|| {
        PureException::from(PureRuntimeError::EvaluationError(format!(
            "{label}: ColSpec.name slot missing"
        )))
    })
}

#[allow(clippy::result_large_err)]
fn group_names_from_colspec_array(
    arg: &ValueSpec,
    ctx: &mut dyn EvalContextTrait,
    label: &str,
) -> Result<Vec<SmolStr>, PureException> {
    let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);
    let csa_value = ctx.evaluate(arg)?.into_value();
    let csa_obj = unwrap_instance_value(&csa_value, instance_value_id, ctx)?;
    let name_values = ctx
        .heap()
        .get_property_values(&csa_obj, "names")
        .map_err(PureException::from)?;
    let names: Vec<SmolStr> = name_values
        .iter()
        .filter_map(|v| match v {
            Value::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    if names.is_empty() {
        return Err(PureException::from(PureRuntimeError::EvaluationError(
            format!("{label}: ColSpecArray.names slot empty"),
        )));
    }
    Ok(names)
}

/// Read a single `AggColSpec` (name + map + reduce) off the heap.
#[allow(clippy::result_large_err)]
fn agg_from_heap(
    arg: &ValueSpec,
    ctx: &mut dyn EvalContextTrait,
    label: &str,
) -> Result<AggSpec, PureException> {
    let instance_value_id = m3_paths::resolve(ctx.model(), m3_paths::INSTANCE_VALUE);
    let acs_value = ctx.evaluate(arg)?.into_value();
    let acs_obj = unwrap_instance_value(&acs_value, instance_value_id, ctx)?;
    let name = ctx
        .heap()
        .get_property_values(&acs_obj, "name")
        .map_err(PureException::from)?
        .iter()
        .find_map(|v| match v {
            Value::String(s) => Some(s.clone()),
            _ => None,
        })
        .ok_or_else(|| {
            PureException::from(PureRuntimeError::EvaluationError(format!(
                "{label}: AggColSpec.name slot missing"
            )))
        })?;
    let map = function_slot(&acs_obj, "map", ctx, label)?;
    let reduce = function_slot(&acs_obj, "reduce", ctx, label)?;
    Ok(AggSpec { name, map, reduce })
}

/// Read `AggColSpecArray` columns from the AST (the `~[…]` heap form
/// keeps only names; the lambdas live on the lowered `ColSpecArrayLiteral`).
#[allow(clippy::result_large_err)]
fn aggs_from_ast(
    arg: &ValueSpec,
    ctx: &mut dyn EvalContextTrait,
    label: &str,
) -> Result<Vec<AggSpec>, PureException> {
    let cols_ast = match arg.kind.as_ref() {
        ExprKind::ColSpecArrayLiteral {
            columns,
            kind: ColSpecLiteralKind::Agg,
        } => columns,
        other => {
            return Err(PureException::from(PureRuntimeError::EvaluationError(format!(
                "{label}: arg 3 must be a direct ~[name:map:reduce, …] literal; got {other:?}"
            ))));
        }
    };
    let mut out = Vec::with_capacity(cols_ast.len());
    for col in cols_ast {
        let (Some(map_vs), Some(reduce_vs)) =
            (col.init_lambda.as_ref(), col.reduce_lambda.as_ref())
        else {
            return Err(PureException::from(PureRuntimeError::EvaluationError(format!(
                "{label}: column '{}' must carry both a map and a reduce lambda",
                col.name
            ))));
        };
        let map = ctx.evaluate(map_vs)?.into_value();
        let reduce = ctx.evaluate(reduce_vs)?.into_value();
        out.push(AggSpec {
            name: col.name.clone(),
            map,
            reduce,
        });
    }
    Ok(out)
}

#[allow(clippy::result_large_err)]
fn function_slot(
    obj: &ObjectHandle,
    slot: &str,
    ctx: &mut dyn EvalContextTrait,
    label: &str,
) -> Result<Value, PureException> {
    ctx.heap()
        .get_property_values(obj, slot)
        .map_err(PureException::from)?
        .iter()
        .find(|v| matches!(v, Value::Function(_)))
        .cloned()
        .ok_or_else(|| {
            PureException::from(PureRuntimeError::EvaluationError(format!(
                "{label}: AggColSpec.{slot} slot missing or not a Function"
            )))
        })
}

// ---------------------------------------------------------------------------
// Core
// ---------------------------------------------------------------------------

#[allow(clippy::result_large_err)]
fn run_group_by(
    parsed: &ParsedTDS,
    group_cols: &[SmolStr],
    aggs: &[AggSpec],
    ctx: &mut dyn EvalContextTrait,
    label: &str,
) -> Result<Evaluated, PureException> {
    // Resolve group columns to source indices.
    let group_indices: Vec<usize> = group_cols
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
                        "{label}: group column '{name}' not present; have {available:?}"
                    )))
                })
        })
        .collect::<Result<_, _>>()?;

    // Group row indices by group-key tuple, first-occurrence order.
    let mut groups: Vec<(Vec<Option<TypedCell>>, Vec<usize>)> = Vec::new();
    for (row_idx, row) in parsed.rows.iter().enumerate() {
        let key: Vec<Option<TypedCell>> =
            group_indices.iter().map(|&i| row[i].clone()).collect();
        if let Some(slot) = groups.iter_mut().find(|(k, _)| k == &key) {
            slot.1.push(row_idx);
        } else {
            groups.push((key, vec![row_idx]));
        }
    }

    // Per group, per agg: map each row -> K, reduce -> V.
    // agg_cells[a] = column a's cells, one per group (group order).
    let mut agg_cells: Vec<Vec<Option<TypedCell>>> =
        vec![Vec::with_capacity(groups.len()); aggs.len()];
    for (_key, rows) in &groups {
        for (a, agg) in aggs.iter().enumerate() {
            let mut collection: PVector<Value> = PVector::new();
            for &row_idx in rows {
                let row_tuple = build_row_tuple(&parsed.columns, &parsed.rows[row_idx], ctx)?;
                let k = ctx.call_function(&agg.map, &[Value::Object(row_tuple)])?;
                push_flat(&mut collection, &k);
            }
            // Empty aggregation collection -> null (no reduce(empty)),
            // parity with the OLAP aggregate path.
            let cell = if collection.is_empty() {
                None
            } else {
                let arg = if collection.len() == 1 {
                    collection.iter().next().cloned().unwrap_or(Value::Unit)
                } else {
                    Value::Collection(Box::new(collection))
                };
                let v = ctx.call_function(&agg.reduce, &[arg])?;
                value_to_typed_cell(&v)
            };
            agg_cells[a].push(cell);
        }
    }

    // Build result schema: group columns (carry source metadata) + agg columns.
    let mut columns: Vec<ParsedColumn> = group_indices
        .iter()
        .map(|&i| parsed.columns[i].clone())
        .collect();
    for (a, agg) in aggs.iter().enumerate() {
        let (type_tag, multiplicity) = infer_column_type_and_mult(&agg_cells[a]);
        columns.push(ParsedColumn {
            name: agg.name.clone(),
            type_tag,
            multiplicity,
        });
    }

    // Build result rows: one per group (key cells + agg cells).
    let mut rows: Vec<Vec<Option<TypedCell>>> = Vec::with_capacity(groups.len());
    for (g_idx, (key, _)) in groups.iter().enumerate() {
        let mut row: Vec<Option<TypedCell>> = key.clone();
        for cells in &agg_cells {
            row.push(cells[g_idx].clone());
        }
        rows.push(row);
    }

    let result = ParsedTDS {
        csv: String::new(),
        columns,
        rows,
    };
    let new_tds = alloc_tds_from_parsed(ctx, &result)?;
    Ok(Evaluated::new(Value::Object(new_tds)))
}

/// Flatten one Collection layer, drop Unit (shared with the OLAP path).
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
